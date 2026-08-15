use std::{
    convert::Infallible,
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post, put},
};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::services::ServeDir;
use uuid::Uuid;

use crate::{
    adapters::{Provider, RunRequest, adapter},
    config::ProjectConfig,
    process::{self, ProcessEvent},
    repository::{self, RegisteredProject},
};

const STATE_FILE: &str = "dashboard-state.json";

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub bind: SocketAddr,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl RunStatus {
    fn is_active(&self) -> bool {
        matches!(self, Self::Running)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputLine {
    pub source: OutputSource,
    pub text: String,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputSource {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredRun {
    pub id: Uuid,
    pub project_id: String,
    pub provider: Provider,
    pub prompt: String,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub output: Vec<OutputLine>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRun {
    pub project_id: String,
    pub provider: Option<Provider>,
    pub prompt: String,
    pub saved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedDashboard {
    #[serde(default)]
    runs: Vec<StoredRun>,
    #[serde(default)]
    pending: Option<PendingRun>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectView {
    id: String,
    path: PathBuf,
    default_provider: Provider,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardResponse {
    projects: Vec<ProjectView>,
    runs: Vec<StoredRun>,
    pending: Option<PendingRun>,
    active_run_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StartRunRequest {
    project_id: String,
    provider: Option<Provider>,
    prompt: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartRunResponse {
    disposition: &'static str,
    run: Option<StoredRun>,
    pending: Option<PendingRun>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventMessage<'a> {
    kind: &'a str,
    run_id: Uuid,
    line: Option<&'a OutputLine>,
}

struct ActiveRun {
    id: Uuid,
    cancellation: Arc<AtomicBool>,
}

struct AppState {
    token: String,
    state_path: PathBuf,
    dashboard: Mutex<PersistedDashboard>,
    active: Mutex<Option<ActiveRun>>,
    events: broadcast::Sender<String>,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "a valid bearer token is required".into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn internal(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

type ApiResult<T> = std::result::Result<T, ApiError>;

pub async fn serve(options: ServeOptions) -> Result<()> {
    let token = Uuid::new_v4().to_string();
    let state = Arc::new(AppState::load(token.clone())?);
    let app = Router::new()
        .route("/api/dashboard", get(dashboard))
        .route("/api/runs", post(start_run))
        .route("/api/pending", put(update_pending).delete(delete_pending))
        .route("/api/pending/start", post(start_pending_run))
        .route("/api/runs/{id}/cancel", post(cancel_run))
        .route("/api/events", get(events))
        .fallback_service(ServeDir::new("web/dist").append_index_html_on_directories(true))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(options.bind)
        .await
        .with_context(|| format!("bind dashboard to {}", options.bind))?;
    println!("Agent Loop Dashboard listening at http://{}", options.bind);
    println!("Access token: {token}");
    axum::serve(listener, app).await.context("serve dashboard")
}

impl AppState {
    fn load(token: String) -> Result<Self> {
        let directory = repository::data_directory()?;
        fs::create_dir_all(&directory)?;
        let state_path = directory.join(STATE_FILE);
        let mut dashboard = if state_path.exists() {
            serde_json::from_slice(&fs::read(&state_path)?)
                .with_context(|| format!("parse {}", state_path.display()))?
        } else {
            PersistedDashboard::default()
        };
        for run in &mut dashboard.runs {
            if run.status.is_active() {
                run.status = RunStatus::Interrupted;
                run.finished_at = Some(Utc::now());
                run.error =
                    Some("the dashboard service restarted while this run was active".into());
            }
        }
        let (events, _) = broadcast::channel(256);
        let state = Self {
            token,
            state_path,
            dashboard: Mutex::new(dashboard),
            active: Mutex::new(None),
            events,
        };
        state.save()?;
        Ok(state)
    }

    fn save(&self) -> Result<()> {
        let dashboard = self
            .dashboard
            .lock()
            .map_err(|_| anyhow::anyhow!("dashboard state lock poisoned"))?;
        fs::write(&self.state_path, serde_json::to_vec_pretty(&*dashboard)?)
            .with_context(|| format!("write {}", self.state_path.display()))
    }

    fn emit(&self, event: impl Serialize) {
        if let Ok(event) = serde_json::to_string(&event) {
            let _ = self.events.send(event);
        }
    }

    fn append_output(&self, run_id: Uuid, source: OutputSource, text: String) {
        let line = OutputLine {
            source,
            text,
            received_at: Utc::now(),
        };
        if let Ok(mut dashboard) = self.dashboard.lock() {
            if let Some(run) = dashboard.runs.iter_mut().find(|run| run.id == run_id) {
                run.output.push(line.clone());
            }
        }
        let _ = self.save();
        self.emit(EventMessage {
            kind: "output",
            run_id,
            line: Some(&line),
        });
    }

    fn finish_run(&self, run_id: Uuid, status: RunStatus, error: Option<String>) {
        if let Ok(mut dashboard) = self.dashboard.lock() {
            if let Some(run) = dashboard.runs.iter_mut().find(|run| run.id == run_id) {
                run.status = status;
                run.finished_at = Some(Utc::now());
                run.error = error;
            }
        }
        if let Ok(mut active) = self.active.lock() {
            *active = None;
        }
        let _ = self.save();
        self.emit(EventMessage {
            kind: "state",
            run_id,
            line: None,
        });
    }
}

fn authorize(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    let provided = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if provided == Some(state.token.as_str()) {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}

async fn dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<DashboardResponse>> {
    authorize(&headers, &state)?;
    let dashboard = state
        .dashboard
        .lock()
        .map_err(|_| ApiError::internal(anyhow::anyhow!("dashboard state lock poisoned")))?
        .clone();
    let active_run_id = dashboard
        .runs
        .iter()
        .find(|run| run.status.is_active())
        .map(|run| run.id);
    Ok(Json(DashboardResponse {
        projects: project_views().map_err(ApiError::internal)?,
        runs: dashboard.runs.into_iter().rev().collect(),
        pending: dashboard.pending,
        active_run_id,
    }))
}

async fn start_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<StartRunRequest>,
) -> ApiResult<Json<StartRunResponse>> {
    authorize(&headers, &state)?;
    validate_start_request(&request)?;
    if state
        .active
        .lock()
        .map_err(|_| ApiError::internal(anyhow::anyhow!("active run lock poisoned")))?
        .is_some()
    {
        let pending = save_pending_request(&state, request)?;
        return Ok(Json(StartRunResponse {
            disposition: "saved",
            run: None,
            pending: Some(pending),
        }));
    }
    match launch(Arc::clone(&state), request.clone()) {
        Ok(run) => Ok(Json(StartRunResponse {
            disposition: "started",
            run: Some(run),
            pending: None,
        })),
        Err(error) if error.status == StatusCode::CONFLICT => {
            let pending = save_pending_request(&state, request)?;
            Ok(Json(StartRunResponse {
                disposition: "saved",
                run: None,
                pending: Some(pending),
            }))
        }
        Err(error) => Err(error),
    }
}

async fn update_pending(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<StartRunRequest>,
) -> ApiResult<Json<PendingRun>> {
    authorize(&headers, &state)?;
    validate_start_request(&request)?;
    let pending = save_pending_request(&state, request)?;
    Ok(Json(pending))
}

async fn delete_pending(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    authorize(&headers, &state)?;
    state
        .dashboard
        .lock()
        .map_err(|_| ApiError::internal(anyhow::anyhow!("dashboard state lock poisoned")))?
        .pending = None;
    state.save().map_err(ApiError::internal)?;
    state.emit(serde_json::json!({ "kind": "state" }));
    Ok(StatusCode::NO_CONTENT)
}

async fn start_pending_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<StoredRun>> {
    authorize(&headers, &state)?;
    if state
        .active
        .lock()
        .map_err(|_| ApiError::internal(anyhow::anyhow!("active run lock poisoned")))?
        .is_some()
    {
        return Err(ApiError::conflict(
            "an active run must finish before the pending run can start",
        ));
    }
    let pending = state
        .dashboard
        .lock()
        .map_err(|_| ApiError::internal(anyhow::anyhow!("dashboard state lock poisoned")))?
        .pending
        .clone()
        .ok_or_else(|| ApiError::not_found("there is no pending run"))?;
    let request = StartRunRequest {
        project_id: pending.project_id,
        provider: pending.provider,
        prompt: pending.prompt,
    };
    let run = launch(Arc::clone(&state), request)?;
    state
        .dashboard
        .lock()
        .map_err(|_| ApiError::internal(anyhow::anyhow!("dashboard state lock poisoned")))?
        .pending = None;
    state.save().map_err(ApiError::internal)?;
    state.emit(serde_json::json!({ "kind": "state" }));
    Ok(Json(run))
}

async fn cancel_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(run_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    authorize(&headers, &state)?;
    let active = state
        .active
        .lock()
        .map_err(|_| ApiError::internal(anyhow::anyhow!("active run lock poisoned")))?;
    let active = active
        .as_ref()
        .ok_or_else(|| ApiError::conflict("there is no active run"))?;
    if active.id != run_id {
        return Err(ApiError::conflict("only the active run can be cancelled"));
    }
    active.cancellation.store(true, Ordering::Relaxed);
    Ok(StatusCode::ACCEPTED)
}

async fn events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>>> {
    authorize(&headers, &state)?;
    let stream = BroadcastStream::new(state.events.subscribe()).filter_map(|message| async move {
        message
            .ok()
            .map(|message| Ok(Event::default().event("update").data(message)))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn validate_start_request(request: &StartRunRequest) -> ApiResult<()> {
    if request.project_id.trim().is_empty() {
        return Err(ApiError::bad_request("select a registered project"));
    }
    if request.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("prompt cannot be empty"));
    }
    Ok(())
}

fn launch(state: Arc<AppState>, request: StartRunRequest) -> ApiResult<StoredRun> {
    let project = registered_project(&request.project_id)?;
    let config = ProjectConfig::load(&project.repository_root).map_err(ApiError::internal)?;
    let provider = request.provider.unwrap_or(config.agent.provider);
    let command_adapter = adapter(provider);
    let command = command_adapter
        .command(
            &config,
            &RunRequest {
                repository_root: &project.repository_root,
                prompt: &request.prompt,
                resume_session: None,
                model_override: None,
                effort_override: None,
            },
        )
        .map_err(ApiError::internal)?;
    let run = StoredRun {
        id: Uuid::new_v4(),
        project_id: project.id,
        provider,
        prompt: request.prompt,
        status: RunStatus::Running,
        started_at: Utc::now(),
        finished_at: None,
        output: Vec::new(),
        error: None,
    };
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut active = state
        .active
        .lock()
        .map_err(|_| ApiError::internal(anyhow::anyhow!("active run lock poisoned")))?;
    if active.is_some() {
        return Err(ApiError::conflict("another run became active"));
    }
    *active = Some(ActiveRun {
        id: run.id,
        cancellation: Arc::clone(&cancellation),
    });
    drop(active);
    state
        .dashboard
        .lock()
        .map_err(|_| ApiError::internal(anyhow::anyhow!("dashboard state lock poisoned")))?
        .runs
        .push(run.clone());
    state.save().map_err(ApiError::internal)?;
    state.emit(EventMessage {
        kind: "state",
        run_id: run.id,
        line: None,
    });

    let run_id = run.id;
    let run_directory = project
        .repository_root
        .join(".agent-loop/runs")
        .join(run_id.to_string());
    let timeout = Duration::from_secs(config.agent.max_duration_seconds);
    thread::spawn(move || {
        let adapter = adapter(provider);
        let worker_state = Arc::clone(&state);
        let result = process::execute_observed(
            adapter.as_ref(),
            &command,
            &run_directory,
            timeout,
            Some(&cancellation),
            move |event| match event {
                ProcessEvent::Stdout(line) => {
                    worker_state.append_output(run_id, OutputSource::Stdout, line)
                }
                ProcessEvent::Stderr(line) => {
                    worker_state.append_output(run_id, OutputSource::Stderr, line)
                }
            },
        );
        let status = if cancellation.load(Ordering::Relaxed) {
            RunStatus::Cancelled
        } else if result.is_ok() {
            RunStatus::Completed
        } else {
            RunStatus::Failed
        };
        let error = result.err().map(|error| error.to_string());
        state.finish_run(run_id, status, error);
    });
    Ok(run)
}

fn registered_project(project_id: &str) -> ApiResult<RegisteredProject> {
    repository::list_registered_projects()
        .map_err(ApiError::internal)?
        .into_iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| {
            ApiError::not_found(format!("registered project `{project_id}` was not found"))
        })
}

fn save_pending_request(state: &AppState, request: StartRunRequest) -> ApiResult<PendingRun> {
    let pending = PendingRun {
        project_id: request.project_id,
        provider: request.provider,
        prompt: request.prompt,
        saved_at: Utc::now(),
    };
    state
        .dashboard
        .lock()
        .map_err(|_| ApiError::internal(anyhow::anyhow!("dashboard state lock poisoned")))?
        .pending = Some(pending.clone());
    state.save().map_err(ApiError::internal)?;
    state.emit(serde_json::json!({ "kind": "state" }));
    Ok(pending)
}

fn project_views() -> Result<Vec<ProjectView>> {
    repository::list_registered_projects()?
        .into_iter()
        .map(|project| {
            let config = ProjectConfig::load(&project.repository_root)?;
            Ok(ProjectView {
                id: project.id,
                path: project.repository_root,
                default_provider: config.agent.provider,
            })
        })
        .collect()
}
