use std::{
    convert::Infallible,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
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
    routing::{get, post},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::services::ServeDir;
use uuid::Uuid;

use crate::{
    adapters::Provider,
    config::ProjectConfig,
    execution::{
        CreateWorkItem, DecisionRequest, ExecutionOutputLine, ExecutionService, LocalRun, WorkItem,
    },
    repository::{self, RegisteredProject},
};

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub bind: SocketAddr,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectView {
    id: String,
    path: PathBuf,
    default_provider: Provider,
    target_branch: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardResponse {
    projects: Vec<ProjectView>,
    work_items: Vec<WorkItem>,
    runs: Vec<LocalRun>,
    active_run_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateWorkItemRequest {
    project_id: String,
    title: String,
    prompt: String,
    #[serde(default = "default_scope")]
    declared_scope: Vec<String>,
    baseline_ref: Option<String>,
    target_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartWorkItemRequest {
    provider: Option<Provider>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartWorkItemResponse {
    disposition: &'static str,
    work_item_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecisionBody {
    decision: LocalDecision,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LocalDecision {
    Approve,
    Reject,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventMessage {
    kind: &'static str,
    run_id: Option<Uuid>,
    line: Option<ExecutionOutputLine>,
}

struct ActiveRun {
    work_item_id: Uuid,
    cancellation: Arc<AtomicBool>,
}

struct AppState {
    token: String,
    data_root: PathBuf,
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
    let data_root = repository::data_directory()?;
    ExecutionService::load(&data_root)?.recover_interrupted_runs()?;
    let (event_sender, _) = broadcast::channel(256);
    let state = Arc::new(AppState {
        token: token.clone(),
        data_root,
        active: Mutex::new(None),
        events: event_sender,
    });
    let app = Router::new()
        .route("/api/dashboard", get(dashboard))
        .route("/api/work-items", post(create_work_item))
        .route("/api/work-items/{id}/start", post(start_work_item))
        .route("/api/runs/{id}/decision", post(decide_run))
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

async fn dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<DashboardResponse>> {
    authorize(&headers, &state)?;
    let snapshot = service(&state)?.snapshot();
    let active_work_item_id = state
        .active
        .lock()
        .map_err(|_| ApiError::internal(anyhow::anyhow!("active execution lock poisoned")))?
        .as_ref()
        .map(|active| active.work_item_id);
    let in_memory_run_id = active_work_item_id.and_then(|work_item_id| {
        snapshot
            .work_items
            .iter()
            .find(|item| item.id == work_item_id)
            .and_then(|item| item.run_id)
    });
    let active_run_id = in_memory_run_id.or_else(|| {
        snapshot
            .runs
            .iter()
            .rev()
            .find(|run| run.status.blocks_new_run())
            .map(|run| run.id)
    });
    Ok(Json(DashboardResponse {
        projects: project_views().map_err(ApiError::internal)?,
        work_items: snapshot.work_items.into_iter().rev().collect(),
        runs: snapshot.runs.into_iter().rev().collect(),
        active_run_id,
    }))
}

async fn create_work_item(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateWorkItemRequest>,
) -> ApiResult<Json<WorkItem>> {
    authorize(&headers, &state)?;
    if request.project_id.trim().is_empty() {
        return Err(ApiError::bad_request("select a registered project"));
    }
    if request.title.trim().is_empty() || request.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("title and prompt cannot be empty"));
    }
    let project = registered_project(&request.project_id)?;
    let config = ProjectConfig::load(&project.repository_root).map_err(ApiError::internal)?;
    let target_branch = request
        .target_branch
        .unwrap_or(config.execution.target_branch);
    let baseline_ref = request
        .baseline_ref
        .unwrap_or_else(|| target_branch.clone());
    let item = service(&state)?
        .create_work_item(CreateWorkItem {
            project,
            title: request.title,
            prompt: request.prompt,
            declared_scope: request.declared_scope,
            baseline_ref,
            target_branch,
        })
        .map_err(ApiError::internal)?;
    state.emit(EventMessage {
        kind: "state",
        run_id: None,
        line: None,
    });
    Ok(Json(item))
}

async fn start_work_item(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(work_item_id): Path<Uuid>,
    Json(request): Json<StartWorkItemRequest>,
) -> ApiResult<(StatusCode, Json<StartWorkItemResponse>)> {
    authorize(&headers, &state)?;
    let snapshot = service(&state)?.snapshot();
    if snapshot.runs.iter().any(|run| run.status.blocks_new_run()) {
        return Err(ApiError::conflict("another local run is active"));
    }
    let work_item = snapshot
        .work_items
        .into_iter()
        .find(|item| item.id == work_item_id)
        .ok_or_else(|| ApiError::not_found(format!("work item {work_item_id} was not found")))?;
    let config = ProjectConfig::load(&work_item.repository_root).map_err(ApiError::internal)?;
    let provider = request.provider.unwrap_or(config.agent.provider);
    let cancellation = Arc::new(AtomicBool::new(false));
    {
        let mut active = state
            .active
            .lock()
            .map_err(|_| ApiError::internal(anyhow::anyhow!("active execution lock poisoned")))?;
        if active.is_some() {
            return Err(ApiError::conflict("another local run is active"));
        }
        *active = Some(ActiveRun {
            work_item_id,
            cancellation: Arc::clone(&cancellation),
        });
    }
    let worker_state = Arc::clone(&state);
    thread::spawn(move || {
        let result = ExecutionService::load(&worker_state.data_root).and_then(|mut service| {
            service.run_work_item(&work_item_id, provider, Some(&cancellation), |event| {
                let run_id = run_id_for_work_item(&worker_state.data_root, work_item_id);
                let line = event.into();
                worker_state.emit(EventMessage {
                    kind: "output",
                    run_id,
                    line: Some(line),
                });
            })
        });
        if let Err(error) = result {
            eprintln!("run work item {work_item_id}: {error}");
        }
        if let Ok(mut active) = worker_state.active.lock() {
            *active = None;
        }
        worker_state.emit(EventMessage {
            kind: "state",
            run_id: run_id_for_work_item(&worker_state.data_root, work_item_id),
            line: None,
        });
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(StartWorkItemResponse {
            disposition: "started",
            work_item_id,
        }),
    ))
}

async fn decide_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(run_id): Path<Uuid>,
    Json(body): Json<DecisionBody>,
) -> ApiResult<Json<LocalRun>> {
    authorize(&headers, &state)?;
    let decision = match body.decision {
        LocalDecision::Approve => DecisionRequest::Approve {
            actor: "local-dashboard".into(),
            reason: body.reason,
        },
        LocalDecision::Reject => DecisionRequest::Reject {
            actor: "local-dashboard".into(),
            reason: body.reason,
        },
    };
    let run = service(&state)?
        .decide(run_id, decision)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    state.emit(EventMessage {
        kind: "state",
        run_id: Some(run_id),
        line: None,
    });
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
        .map_err(|_| ApiError::internal(anyhow::anyhow!("active execution lock poisoned")))?;
    let active = active
        .as_ref()
        .ok_or_else(|| ApiError::conflict("there is no active run"))?;
    let active_run_id = run_id_for_work_item(&state.data_root, active.work_item_id);
    if active_run_id != Some(run_id) {
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

impl AppState {
    fn emit(&self, event: impl Serialize) {
        if let Ok(event) = serde_json::to_string(&event) {
            let _ = self.events.send(event);
        }
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

fn service(state: &AppState) -> ApiResult<ExecutionService> {
    ExecutionService::load(&state.data_root).map_err(ApiError::internal)
}

fn run_id_for_work_item(data_root: &std::path::Path, work_item_id: Uuid) -> Option<Uuid> {
    ExecutionService::load(data_root)
        .ok()?
        .snapshot()
        .work_items
        .into_iter()
        .find(|item| item.id == work_item_id)?
        .run_id
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

fn project_views() -> Result<Vec<ProjectView>> {
    repository::list_registered_projects()?
        .into_iter()
        .map(|project| {
            let config = ProjectConfig::load(&project.repository_root)?;
            Ok(ProjectView {
                id: project.id,
                path: project.repository_root,
                default_provider: config.agent.provider,
                target_branch: config.execution.target_branch,
            })
        })
        .collect()
}

fn default_scope() -> Vec<String> {
    vec![".".into()]
}
