use std::{
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    adapters::{Provider, RunRequest, adapter},
    config::{EnvironmentProfile, ProjectConfig},
    contracts::{
        self, AcceptanceCriterion, Attempt, AttemptOutcome, Authority, Baseline, Candidate,
        CandidateIdentity, CandidateKind, CheckOutcome, CheckResult, ComponentSet,
        ConventionSelection, Decision, DecisionOutcome, ExpectedCapabilityState,
        HandoffRequirements, NetworkAuthority, NetworkMode, Publication, PublicationKind,
        PublicationStatus, Run, RunAgent, RunProject, RunState, RunWorkItem, TaskPacket,
    },
    process::{self, CommandSpec, ProcessEvent},
    repository::RegisteredProject,
};

const STATE_FILE: &str = "execution-state.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
    pub id: Uuid,
    pub project_id: String,
    pub repository_root: PathBuf,
    pub title: String,
    pub prompt: String,
    pub declared_scope: Vec<String>,
    pub baseline: Baseline,
    pub target_branch: String,
    pub status: WorkItemStatus,
    pub run_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CreateWorkItem {
    pub project: RegisteredProject,
    pub title: String,
    pub prompt: String,
    pub declared_scope: Vec<String>,
    pub baseline_ref: String,
    pub target_branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemStatus {
    Open,
    Running,
    AwaitingDecision,
    Approved,
    Rejected,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalRunStatus {
    Preparing,
    Running,
    Evaluating,
    AwaitingDecision,
    Integrating,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionOutputLine {
    pub source: ExecutionOutputSource,
    pub text: String,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionOutputSource {
    Stdout,
    Stderr,
}

impl From<ProcessEvent> for ExecutionOutputLine {
    fn from(event: ProcessEvent) -> Self {
        let (source, text) = match event {
            ProcessEvent::Stdout(text) => (ExecutionOutputSource::Stdout, text),
            ProcessEvent::Stderr(text) => (ExecutionOutputSource::Stderr, text),
        };
        Self {
            source,
            text,
            received_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRun {
    pub id: Uuid,
    pub work_item_id: Uuid,
    pub project_id: String,
    pub provider: Provider,
    pub status: LocalRunStatus,
    pub target_branch: String,
    pub worktree_path: PathBuf,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub output: Vec<ExecutionOutputLine>,
    pub error: Option<String>,
    pub contract: Run,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSnapshot {
    pub work_items: Vec<WorkItem>,
    pub runs: Vec<LocalRun>,
}

#[derive(Debug, Clone)]
pub enum DecisionRequest {
    Approve {
        actor: String,
        reason: Option<String>,
    },
    Reject {
        actor: String,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionOverrides {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub resume_session: Option<String>,
    pub objective: Option<String>,
    pub acceptance: Option<Vec<AcceptanceCriterion>>,
    pub dependencies: Option<Vec<String>>,
    pub check_tier: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedState {
    #[serde(default)]
    work_items: Vec<WorkItem>,
    #[serde(default)]
    runs: Vec<LocalRun>,
}

pub struct ExecutionService {
    data_root: PathBuf,
    state_path: PathBuf,
    lock_path: PathBuf,
    state: PersistedState,
}

impl ExecutionService {
    pub fn load(data_root: impl Into<PathBuf>) -> Result<Self> {
        let data_root = data_root.into();
        fs::create_dir_all(&data_root)
            .with_context(|| format!("create {}", data_root.display()))?;
        let state_path = data_root.join(STATE_FILE);
        let state = if state_path.exists() {
            serde_json::from_slice(&fs::read(&state_path)?)
                .with_context(|| format!("parse {}", state_path.display()))?
        } else {
            PersistedState::default()
        };
        Ok(Self {
            lock_path: data_root.join("execution-state.lock"),
            data_root,
            state_path,
            state,
        })
    }

    pub fn snapshot(&self) -> ExecutionSnapshot {
        ExecutionSnapshot {
            work_items: self.state.work_items.clone(),
            runs: self.state.runs.clone(),
        }
    }

    pub fn create_work_item(&mut self, request: CreateWorkItem) -> Result<WorkItem> {
        validate_nonempty("title", &request.title)?;
        validate_nonempty("prompt", &request.prompt)?;
        validate_nonempty("baseline ref", &request.baseline_ref)?;
        validate_nonempty("target branch", &request.target_branch)?;
        validate_scope(&request.declared_scope)?;
        let baseline_sha = git(
            &request.project.repository_root,
            &["rev-parse", &format!("{}^{{commit}}", request.baseline_ref)],
        )?;
        let work_item = WorkItem {
            id: Uuid::new_v4(),
            project_id: request.project.id,
            repository_root: request.project.repository_root,
            title: request.title,
            prompt: request.prompt,
            declared_scope: request.declared_scope,
            baseline: Baseline {
                git_sha: baseline_sha,
                r#ref: Some(request.baseline_ref),
            },
            target_branch: request.target_branch,
            status: WorkItemStatus::Open,
            run_id: None,
            created_at: Utc::now(),
        };
        let lock = self.lock_exclusive()?;
        self.reload()?;
        self.state.work_items.push(work_item.clone());
        self.write_state()?;
        FileExt::unlock(&lock)?;
        Ok(work_item)
    }

    pub fn run_work_item(
        &mut self,
        work_item_id: &Uuid,
        provider: Provider,
        cancellation: Option<&AtomicBool>,
        observe: impl FnMut(ProcessEvent),
    ) -> Result<LocalRun> {
        self.run_work_item_with_overrides(
            work_item_id,
            provider,
            ExecutionOverrides::default(),
            cancellation,
            observe,
        )
    }

    pub fn run_work_item_with_overrides(
        &mut self,
        work_item_id: &Uuid,
        provider: Provider,
        overrides: ExecutionOverrides,
        cancellation: Option<&AtomicBool>,
        observe: impl FnMut(ProcessEvent),
    ) -> Result<LocalRun> {
        self.refresh()?;
        let work_item = self
            .state
            .work_items
            .iter()
            .find(|item| &item.id == work_item_id)
            .cloned()
            .with_context(|| format!("work item {work_item_id} was not found"))?;
        let config = ProjectConfig::load(&work_item.repository_root)?;
        self.run_work_item_with_config(
            work_item_id,
            provider,
            &config,
            overrides,
            cancellation,
            observe,
        )
    }

    pub fn run_work_item_with_config(
        &mut self,
        work_item_id: &Uuid,
        provider: Provider,
        config: &ProjectConfig,
        overrides: ExecutionOverrides,
        cancellation: Option<&AtomicBool>,
        mut observe: impl FnMut(ProcessEvent),
    ) -> Result<LocalRun> {
        self.refresh()?;
        let work_item = self
            .state
            .work_items
            .iter()
            .find(|item| &item.id == work_item_id)
            .cloned()
            .with_context(|| format!("work item {work_item_id} was not found"))?;
        if work_item.status != WorkItemStatus::Open {
            bail!("work item {work_item_id} is not open");
        }
        let run_id = Uuid::new_v4();
        let run_directory = self.data_root.join("runs").join(run_id.to_string());
        let worktree_path = self
            .data_root
            .join("worktrees")
            .join(format!("{run_id}-attempt-1"));
        fs::create_dir_all(&run_directory)?;
        fs::create_dir_all(self.data_root.join("worktrees"))?;
        let (contract, packet) = build_contracts(
            run_id,
            &work_item,
            config,
            provider,
            &overrides,
            &worktree_path,
            &run_directory,
        )?;
        let mut run = LocalRun {
            id: run_id,
            work_item_id: work_item.id,
            project_id: work_item.project_id.clone(),
            provider,
            status: LocalRunStatus::Preparing,
            target_branch: work_item.target_branch.clone(),
            worktree_path: worktree_path.clone(),
            started_at: Utc::now(),
            finished_at: None,
            output: Vec::new(),
            error: None,
            contract,
        };
        write_contract_artifacts(&run, &packet, &run_directory)?;
        let _run_lease = self.acquire_run_lease(run_id)?;
        self.claim_run(&run)?;

        if let Err(error) = create_worktree(
            &work_item.repository_root,
            &worktree_path,
            &work_item.baseline.git_sha,
        ) {
            let message = failure_with_cleanup(
                &work_item.repository_root,
                &worktree_path,
                format!("create worktree: {error}"),
            );
            return self.finish_failed(run, &run_directory, message);
        }
        if let Err(error) = verify_environment(
            &config.execution.coding_tooling_executable,
            config.execution.environment_profile,
            &worktree_path,
            &run_directory,
        ) {
            let message = failure_with_cleanup(
                &work_item.repository_root,
                &worktree_path,
                format!("environment verification failed before provider launch: {error}"),
            );
            return self.finish_failed(run, &run_directory, message);
        }
        run.contract.state = RunState::Running;
        run.status = LocalRunStatus::Running;
        self.replace_run(&run)?;

        let provider_adapter = adapter(provider);
        let candidate_object_directory = run_directory.join("candidate-objects");
        if let Err(error) = fs::create_dir_all(&candidate_object_directory).with_context(|| {
            format!(
                "create candidate object directory {}",
                candidate_object_directory.display()
            )
        }) {
            let message = failure_with_cleanup(
                &work_item.repository_root,
                &worktree_path,
                error.to_string(),
            );
            return self.finish_failed(run, &run_directory, message);
        }
        let provider_prompt = format!(
            "Implement the work described by this canonical agent.task-packet/v1. Leave the worktree clean and commit the completed candidate. You have no authority to integrate, push, publish, or otherwise mutate a remote system.\n\n{}",
            serde_json::to_string_pretty(&packet)?
        );
        let command = match provider_adapter.command(
            config,
            &RunRequest {
                repository_root: &worktree_path,
                prompt: &provider_prompt,
                resume_session: overrides.resume_session.as_deref(),
                model_override: overrides.model.as_deref(),
                effort_override: overrides.effort.as_deref(),
            },
        ) {
            Ok(command) => command,
            Err(error) => {
                let message = failure_with_cleanup(
                    &work_item.repository_root,
                    &worktree_path,
                    format!("configure provider: {error}"),
                );
                return self.finish_failed(run, &run_directory, message);
            }
        };
        let command = if matches!(provider, Provider::Codex)
            && matches!(
                config.providers.codex.sandbox,
                crate::config::CodexSandbox::DangerFullAccess
            ) {
            command
        } else {
            match sandbox_provider_command(
                &command,
                &worktree_path,
                &run_directory,
                &candidate_object_directory,
            ) {
                Ok(command) => command,
                Err(error) => {
                    let message = failure_with_cleanup(
                        &work_item.repository_root,
                        &worktree_path,
                        format!("enforce provider authority: {error}"),
                    );
                    return self.finish_failed(run, &run_directory, message);
                }
            }
        };
        let mut observed = Vec::new();
        let mut output_persistence_error = None;
        let result = process::execute_observed(
            provider_adapter.as_ref(),
            &command,
            &run_directory,
            Duration::from_secs(config.agent.max_duration_seconds),
            cancellation,
            |event| {
                observe(event.clone());
                let line = event.into();
                if output_persistence_error.is_none()
                    && let Err(error) = self.append_output(run_id, &line)
                {
                    output_persistence_error = Some(error);
                }
                observed.push(line);
            },
        );
        run.output = observed;
        let attempt = run
            .contract
            .attempts
            .first_mut()
            .context("run has no attempt")?;
        attempt.finished_at = Some(Utc::now());
        attempt.provider_session_id = result
            .as_ref()
            .ok()
            .and_then(|outcome| outcome.provider_session_id.clone());
        attempt.evidence = contracts::evidence_from_directory(&run_directory)?;
        attempt
            .evidence
            .extend(environment_verification_evidence(&run_directory)?);
        if let Some(error) = output_persistence_error {
            attempt.outcome = AttemptOutcome::Failed;
            let message = failure_with_cleanup(
                &work_item.repository_root,
                &worktree_path,
                format!("persist provider output: {error}"),
            );
            return self.finish_failed(run, &run_directory, message);
        }
        if let Err(error) = result {
            attempt.outcome = if cancellation
                .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
            {
                AttemptOutcome::Cancelled
            } else {
                AttemptOutcome::Failed
            };
            let cleanup = cleanup_worktree(&work_item.repository_root, &worktree_path);
            if matches!(attempt.outcome, AttemptOutcome::Cancelled) {
                run.status = LocalRunStatus::Cancelled;
                run.contract.state = RunState::Cancelled;
                run.finished_at = Some(Utc::now());
                run.error = Some(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => format!("{error}; cleanup failed: {cleanup_error}"),
                });
                self.update_work_item(work_item.id, WorkItemStatus::Cancelled, Some(run_id));
                self.replace_run(&run)?;
                return Ok(run);
            }
            let message = match cleanup {
                Ok(()) => format!("provider failed: {error}"),
                Err(cleanup_error) => {
                    format!("provider failed: {error}; cleanup failed: {cleanup_error}")
                }
            };
            return self.finish_failed(run, &run_directory, message);
        }

        let candidate = match validate_candidate(
            &work_item,
            &worktree_path,
            &candidate_object_directory,
            run_id,
        ) {
            Ok(candidate) => candidate,
            Err(error) => {
                let message = failure_with_cleanup(
                    &work_item.repository_root,
                    &worktree_path,
                    error.to_string(),
                );
                return self.finish_failed(run, &run_directory, message);
            }
        };
        let candidate_sha = candidate
            .git_sha
            .clone()
            .context("candidate has no Git SHA")?;
        if let Err(error) = create_candidate_ref(
            &work_item.repository_root,
            &worktree_path,
            &run_directory,
            &candidate_object_directory,
            run_id,
            &candidate_sha,
        ) {
            let message = failure_with_cleanup(
                &work_item.repository_root,
                &worktree_path,
                format!("record candidate: {error}"),
            );
            return self.finish_failed(run, &run_directory, message);
        }
        run.contract.attempts[0].outcome = AttemptOutcome::Candidate;
        run.contract.candidates.push(candidate);
        run.contract.state = RunState::Evaluating;
        run.status = LocalRunStatus::Evaluating;
        self.replace_run(&run)?;

        let checks = CodingToolingAdapter {
            executable: &config.execution.coding_tooling_executable,
            tier: overrides
                .check_tier
                .as_deref()
                .unwrap_or(&config.execution.check_tier),
        }
        .run(&worktree_path, &run_directory, &candidate_sha);
        run.contract.checks = checks;
        self.replace_run(&run)?;
        if let Err(error) = cleanup_worktree(&work_item.repository_root, &worktree_path) {
            return self.finish_failed(
                run,
                &run_directory,
                format!("clean up attempt worktree: {error}"),
            );
        }
        if run.contract.checks.is_empty()
            || !run.contract.checks.iter().any(|check| check.required)
            || run
                .contract
                .checks
                .iter()
                .any(|check| check.required && check.outcome != CheckOutcome::Passed)
            || overrides.acceptance.as_ref().is_some_and(|acceptance| {
                !required_acceptance_is_satisfied(acceptance, &run.contract.checks)
            })
        {
            return self.finish_failed(
                run,
                &run_directory,
                "deterministic checks did not pass".into(),
            );
        }
        run.contract.state = RunState::AwaitingDecision;
        run.status = LocalRunStatus::AwaitingDecision;
        self.update_work_item(work_item.id, WorkItemStatus::AwaitingDecision, Some(run_id));
        self.replace_run(&run)?;
        Ok(run)
    }

    pub fn decide(&mut self, run_id: Uuid, decision: DecisionRequest) -> Result<LocalRun> {
        let lock = self.lock_exclusive()?;
        self.reload()?;
        let mut run = self
            .state
            .runs
            .iter()
            .find(|run| run.id == run_id)
            .cloned()
            .with_context(|| format!("run {run_id} was not found"))?;
        if run.status != LocalRunStatus::AwaitingDecision {
            bail!("run {run_id} is not awaiting a decision");
        }
        let mut work_item = self
            .state
            .work_items
            .iter()
            .find(|item| item.id == run.work_item_id)
            .cloned()
            .context("run work item was not found")?;
        let candidate_sha = run
            .contract
            .candidates
            .last()
            .and_then(|candidate| candidate.git_sha.clone())
            .context("run has no Git candidate")?;
        match decision {
            DecisionRequest::Reject { actor, reason } => {
                run.contract.decisions.push(Decision {
                    decision_id: Uuid::new_v4().to_string(),
                    candidate_identity: candidate_sha,
                    decision: DecisionOutcome::Rejected,
                    actor,
                    occurred_at: Utc::now(),
                    reason,
                });
                run.contract.state = RunState::Completed;
                run.status = LocalRunStatus::Completed;
                run.finished_at = Some(Utc::now());
                work_item.status = WorkItemStatus::Rejected;
                work_item.run_id = Some(run_id);
                self.store_locked(&run, &work_item)?;
                FileExt::unlock(&lock)?;
                Ok(run)
            }
            DecisionRequest::Approve { actor, reason } => {
                verify_integration_preconditions(&work_item, run_id, &candidate_sha)?;
                run.contract.decisions.push(Decision {
                    decision_id: Uuid::new_v4().to_string(),
                    candidate_identity: candidate_sha.clone(),
                    decision: DecisionOutcome::Approved,
                    actor,
                    occurred_at: Utc::now(),
                    reason,
                });
                run.contract.state = RunState::Integrating;
                run.status = LocalRunStatus::Integrating;
                self.store_locked(&run, &work_item)?;
                if let Err(error) = integrate_candidate(&work_item, &candidate_sha) {
                    run.contract.publications.push(Publication {
                        publication_id: Uuid::new_v4().to_string(),
                        kind: PublicationKind::LocalIntegration,
                        candidate_identity: candidate_sha,
                        status: PublicationStatus::Failed,
                        external_id: Some(work_item.target_branch.clone()),
                        occurred_at: Utc::now(),
                        evidence: Vec::new(),
                    });
                    run.contract.state = RunState::Failed;
                    run.status = LocalRunStatus::Failed;
                    run.finished_at = Some(Utc::now());
                    run.error = Some(format!("local integration failed: {error}"));
                    work_item.status = WorkItemStatus::Failed;
                    work_item.run_id = Some(run_id);
                    self.store_locked(&run, &work_item)?;
                    FileExt::unlock(&lock)?;
                    return Ok(run);
                }
                run.contract.publications.push(Publication {
                    publication_id: Uuid::new_v4().to_string(),
                    kind: PublicationKind::LocalIntegration,
                    candidate_identity: candidate_sha,
                    status: PublicationStatus::Succeeded,
                    external_id: Some(work_item.target_branch.clone()),
                    occurred_at: Utc::now(),
                    evidence: Vec::new(),
                });
                run.contract.state = RunState::Completed;
                run.status = LocalRunStatus::Completed;
                run.finished_at = Some(Utc::now());
                work_item.status = WorkItemStatus::Approved;
                work_item.run_id = Some(run_id);
                self.store_locked(&run, &work_item)?;
                FileExt::unlock(&lock)?;
                Ok(run)
            }
        }
    }

    pub fn record_publication(
        &mut self,
        run_id: Uuid,
        publication: Publication,
    ) -> Result<LocalRun> {
        let lock = self.lock_exclusive()?;
        self.reload()?;
        let mut run = self
            .state
            .runs
            .iter()
            .find(|run| run.id == run_id)
            .cloned()
            .with_context(|| format!("run {run_id} was not found"))?;
        if run.status != LocalRunStatus::Completed {
            bail!("run {run_id} is not completed");
        }
        let candidate_sha = run
            .contract
            .candidates
            .last()
            .and_then(|candidate| candidate.git_sha.as_deref())
            .context("run has no Git candidate")?;
        if publication.candidate_identity != candidate_sha {
            bail!("publication candidate does not match run candidate");
        }
        if run.contract.publications.iter().any(|stored| {
            stored.kind == publication.kind
                && stored.candidate_identity == publication.candidate_identity
                && stored.external_id == publication.external_id
                && stored.status == publication.status
        }) {
            FileExt::unlock(&lock)?;
            return Ok(run);
        }
        run.contract.publications.push(publication);
        let stored = self
            .state
            .runs
            .iter_mut()
            .find(|stored| stored.id == run_id)
            .context("stored run was not found")?;
        *stored = run.clone();
        let run_directory = self.data_root.join("runs").join(run.id.to_string());
        run.contract.write_to(&run_directory.join("run.json"))?;
        self.write_state()?;
        FileExt::unlock(&lock)?;
        Ok(run)
    }

    fn finish_failed(
        &mut self,
        mut run: LocalRun,
        run_directory: &Path,
        message: String,
    ) -> Result<LocalRun> {
        let failure_path = run_directory.join("orchestrator-error.json");
        fs::write(
            &failure_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "occurredAt": Utc::now(),
                "error": message,
            }))?,
        )?;
        if let Some(attempt) = run.contract.attempts.first_mut() {
            attempt.finished_at.get_or_insert_with(Utc::now);
            if attempt.outcome == AttemptOutcome::Running {
                attempt.outcome = AttemptOutcome::Failed;
            }
            attempt.evidence = contracts::evidence_from_directory(run_directory)?;
            attempt
                .evidence
                .extend(environment_verification_evidence(run_directory)?);
            attempt.evidence.push(contracts::evidence_for_file(
                "orchestrator-error",
                &failure_path,
                "application/json",
            )?);
        }
        run.contract.state = RunState::Failed;
        run.status = LocalRunStatus::Failed;
        run.finished_at = Some(Utc::now());
        run.error = Some(message);
        self.update_work_item(run.work_item_id, WorkItemStatus::Failed, Some(run.id));
        self.replace_run(&run)?;
        Ok(run)
    }

    fn update_work_item(&mut self, id: Uuid, status: WorkItemStatus, run_id: Option<Uuid>) {
        if let Some(item) = self.state.work_items.iter_mut().find(|item| item.id == id) {
            item.status = status;
            item.run_id = run_id;
        }
    }

    fn append_output(&mut self, run_id: Uuid, line: &ExecutionOutputLine) -> Result<()> {
        let lock = self.lock_exclusive()?;
        self.reload()?;
        let run = self
            .state
            .runs
            .iter_mut()
            .find(|run| run.id == run_id)
            .context("stored run was not found while recording output")?;
        run.output.push(line.clone());
        self.write_state()?;
        FileExt::unlock(&lock)?;
        Ok(())
    }

    fn replace_run(&mut self, run: &LocalRun) -> Result<()> {
        let work_item = self
            .state
            .work_items
            .iter()
            .find(|item| item.id == run.work_item_id)
            .cloned()
            .context("stored work item was not found")?;
        let lock = self.lock_exclusive()?;
        self.reload()?;
        self.store_locked(run, &work_item)?;
        FileExt::unlock(&lock)?;
        Ok(())
    }

    fn store_locked(&mut self, run: &LocalRun, work_item: &WorkItem) -> Result<()> {
        let stored = self
            .state
            .runs
            .iter_mut()
            .find(|stored| stored.id == run.id)
            .context("stored run was not found")?;
        *stored = run.clone();
        let stored_item = self
            .state
            .work_items
            .iter_mut()
            .find(|item| item.id == work_item.id)
            .context("stored work item was not found")?;
        *stored_item = work_item.clone();
        let run_directory = self.data_root.join("runs").join(run.id.to_string());
        fs::create_dir_all(&run_directory)?;
        run.contract.write_to(&run_directory.join("run.json"))?;
        self.write_state()?;
        Ok(())
    }

    fn claim_run(&mut self, run: &LocalRun) -> Result<()> {
        let lock = self.lock_exclusive()?;
        self.reload()?;
        if self
            .state
            .runs
            .iter()
            .any(|stored| stored.status.blocks_new_run())
        {
            bail!("another local run is active");
        }
        let item = self
            .state
            .work_items
            .iter_mut()
            .find(|item| item.id == run.work_item_id)
            .context("work item was not found while claiming run")?;
        if item.status != WorkItemStatus::Open {
            bail!("work item {} is not open", item.id);
        }
        item.status = WorkItemStatus::Running;
        item.run_id = Some(run.id);
        self.state.runs.push(run.clone());
        self.write_state()?;
        FileExt::unlock(&lock)?;
        Ok(())
    }

    pub fn recover_interrupted_runs(&mut self) -> Result<()> {
        let lock = self.lock_exclusive()?;
        self.reload()?;
        let candidates = self
            .state
            .runs
            .iter()
            .filter(|run| run.status.is_executing())
            .map(|run| {
                let work_item = self
                    .state
                    .work_items
                    .iter()
                    .find(|item| item.id == run.work_item_id)
                    .cloned();
                (
                    run.id,
                    run.work_item_id,
                    run.worktree_path.clone(),
                    work_item,
                    run.status.clone(),
                )
            })
            .collect::<Vec<_>>();
        let mut recovery_leases = Vec::new();
        let mut interrupted = Vec::new();
        for candidate in candidates {
            if let Some(lease) = self.try_acquire_run_lease(candidate.0)? {
                recovery_leases.push(lease);
                interrupted.push(candidate);
            }
        }
        for (run_id, work_item_id, _, work_item, status) in &interrupted {
            let integration_completed = if *status == LocalRunStatus::Integrating {
                work_item.as_ref().is_some_and(|item| {
                    let candidate = self
                        .state
                        .runs
                        .iter()
                        .find(|run| run.id == *run_id)
                        .and_then(|run| run.contract.candidates.last())
                        .and_then(|candidate| candidate.git_sha.as_deref());
                    let target = git(
                        &item.repository_root,
                        &["rev-parse", &format!("refs/heads/{}", item.target_branch)],
                    )
                    .ok();
                    candidate.is_some() && target.as_deref() == candidate
                })
            } else {
                false
            };
            if let Some(run) = self.state.runs.iter_mut().find(|run| run.id == *run_id) {
                if integration_completed {
                    let candidate_identity = run
                        .contract
                        .candidates
                        .last()
                        .and_then(|candidate| candidate.git_sha.clone())
                        .context("integrating run has no Git candidate")?;
                    if run.contract.publications.is_empty() {
                        run.contract.publications.push(Publication {
                            publication_id: Uuid::new_v4().to_string(),
                            kind: PublicationKind::LocalIntegration,
                            candidate_identity,
                            status: PublicationStatus::Succeeded,
                            external_id: work_item.as_ref().map(|item| item.target_branch.clone()),
                            occurred_at: Utc::now(),
                            evidence: Vec::new(),
                        });
                    }
                    run.status = LocalRunStatus::Completed;
                    run.contract.state = RunState::Completed;
                    run.finished_at = Some(Utc::now());
                } else {
                    run.status = LocalRunStatus::Failed;
                    run.contract.state = RunState::Failed;
                    run.finished_at = Some(Utc::now());
                    run.error = Some("the orchestrator service restarted during this run".into());
                    if let Some(attempt) = run.contract.attempts.first_mut() {
                        attempt.finished_at = Some(Utc::now());
                        if attempt.outcome == AttemptOutcome::Running {
                            attempt.outcome = AttemptOutcome::Failed;
                        }
                    }
                }
            }
            if let Some(item) = self
                .state
                .work_items
                .iter_mut()
                .find(|item| item.id == *work_item_id)
            {
                item.status = if integration_completed {
                    WorkItemStatus::Approved
                } else {
                    WorkItemStatus::Failed
                };
            }
        }
        self.write_state()?;
        FileExt::unlock(&lock)?;
        let mut cleanup_errors = Vec::new();
        for (run_id, _, worktree, work_item, _) in interrupted {
            if let Some(work_item) = work_item
                && let Err(error) = cleanup_worktree(&work_item.repository_root, &worktree)
            {
                cleanup_errors.push((run_id, format!("cleanup failed: {error}")));
            }
            if let Some(run) = self.state.runs.iter().find(|run| run.id == run_id) {
                let run_directory = self.data_root.join("runs").join(run_id.to_string());
                fs::create_dir_all(&run_directory)?;
                run.contract.write_to(&run_directory.join("run.json"))?;
            }
        }
        if !cleanup_errors.is_empty() {
            let lock = self.lock_exclusive()?;
            self.reload()?;
            for (run_id, error) in cleanup_errors {
                if let Some(run) = self.state.runs.iter_mut().find(|run| run.id == run_id) {
                    let recovery = run.error.get_or_insert_default();
                    if !recovery.is_empty() {
                        recovery.push_str("; ");
                    }
                    recovery.push_str(&error);
                    let run_directory = self.data_root.join("runs").join(run_id.to_string());
                    run.contract.write_to(&run_directory.join("run.json"))?;
                }
            }
            self.write_state()?;
            FileExt::unlock(&lock)?;
        }
        drop(recovery_leases);
        Ok(())
    }

    fn refresh(&mut self) -> Result<()> {
        let lock = self.lock_shared()?;
        self.reload()?;
        FileExt::unlock(&lock)?;
        Ok(())
    }

    fn reload(&mut self) -> Result<()> {
        self.state = if self.state_path.exists() {
            serde_json::from_slice(&fs::read(&self.state_path)?)
                .with_context(|| format!("parse {}", self.state_path.display()))?
        } else {
            PersistedState::default()
        };
        Ok(())
    }

    fn lock_shared(&self) -> Result<File> {
        let file = self.open_lock()?;
        FileExt::lock_shared(&file)?;
        Ok(file)
    }

    fn lock_exclusive(&self) -> Result<File> {
        let file = self.open_lock()?;
        FileExt::lock_exclusive(&file)?;
        Ok(file)
    }

    fn open_lock(&self) -> Result<File> {
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.lock_path)
            .with_context(|| format!("open {}", self.lock_path.display()))
    }

    fn acquire_run_lease(&self, run_id: Uuid) -> Result<File> {
        let lease = self.open_run_lease(run_id)?;
        FileExt::lock_exclusive(&lease)?;
        Ok(lease)
    }

    fn try_acquire_run_lease(&self, run_id: Uuid) -> Result<Option<File>> {
        let lease = self.open_run_lease(run_id)?;
        match FileExt::try_lock_exclusive(&lease) {
            Ok(()) => Ok(Some(lease)),
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error).context("acquire run ownership lease"),
        }
    }

    fn open_run_lease(&self, run_id: Uuid) -> Result<File> {
        let directory = self.data_root.join("run-leases");
        fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{run_id}.lock"));
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("open {}", path.display()))
    }

    fn write_state(&self) -> Result<()> {
        let temporary = self.state_path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(&self.state)?)
            .with_context(|| format!("write {}", temporary.display()))?;
        fs::rename(&temporary, &self.state_path)
            .with_context(|| format!("replace {}", self.state_path.display()))
    }
}

impl LocalRunStatus {
    pub(crate) fn is_executing(&self) -> bool {
        matches!(
            self,
            Self::Preparing | Self::Running | Self::Evaluating | Self::Integrating
        )
    }

    pub(crate) fn blocks_new_run(&self) -> bool {
        self.is_executing() || *self == Self::AwaitingDecision
    }
}

fn build_contracts(
    run_id: Uuid,
    work_item: &WorkItem,
    config: &ProjectConfig,
    provider: Provider,
    overrides: &ExecutionOverrides,
    worktree_path: &Path,
    run_directory: &Path,
) -> Result<(Run, TaskPacket)> {
    let lock = contracts::component_lock(&work_item.repository_root, &work_item.baseline.git_sha);
    contracts::write_component_lock(&lock, &run_directory.join("component-lock.json"))?;
    let common_directory = PathBuf::from(git(
        &work_item.repository_root,
        &["rev-parse", "--git-common-dir"],
    )?);
    let common_directory = if common_directory.is_absolute() {
        common_directory
    } else {
        work_item
            .repository_root
            .join(common_directory)
            .canonicalize()?
    };
    let authority = Authority {
        schema_version: 1,
        read_roots: vec!["/".into()],
        write_roots: if matches!(provider, Provider::Codex)
            && matches!(
                config.providers.codex.sandbox,
                crate::config::CodexSandbox::DangerFullAccess
            ) {
            vec!["/".into()]
        } else {
            vec![
                worktree_path.display().to_string(),
                run_directory.display().to_string(),
                common_directory.join("worktrees").display().to_string(),
            ]
        },
        network: NetworkAuthority {
            mode: NetworkMode::Unrestricted,
            allowed_domains: Vec::new(),
        },
        tools: vec![format!("provider:{provider}"), "git".into()],
        secret_refs: vec![format!("provider-auth:{provider}")],
        max_duration_seconds: config.agent.max_duration_seconds,
        max_attempts: 1,
        may_integrate: false,
        may_publish: false,
    };
    let convention_selection = ConventionSelection {
        component_version: None,
        convention_ids: vec!["repository-default".into()],
        digest: contracts::digest_bytes(b"repository-default"),
    };
    let adapter_name = provider.to_string();
    let attempt_id = format!("{run_id}-attempt-1");
    let contract = Run {
        schema_version: 1,
        run_id: run_id.to_string(),
        state: RunState::Preparing,
        project: RunProject {
            id: work_item.project_id.clone(),
            repository: contracts::repository_identity(&work_item.repository_root),
            local_root: Some(work_item.repository_root.display().to_string()),
        },
        work_item: RunWorkItem {
            id: work_item.id.to_string(),
            version: "1".into(),
            title: Some(work_item.title.clone()),
            dependencies: overrides.dependencies.clone().unwrap_or_default(),
            declared_scope: work_item.declared_scope.clone(),
        },
        baseline: work_item.baseline.clone(),
        convention_selection,
        tooling_manifest: None,
        component_set: ComponentSet {
            lock_digest: contracts::digest_json(&lock)?,
            lock_uri: Some("component-lock.json".into()),
        },
        agent: RunAgent {
            adapter: adapter_name.clone(),
            identity: format!("{adapter_name}-provider"),
            model: overrides
                .model
                .clone()
                .or_else(|| contracts::selected_model(config, provider)),
            prompt_digest: Some(contracts::digest_bytes(work_item.prompt.as_bytes())),
            configuration_digest: contracts::digest_bytes(config.to_toml()?.as_bytes()),
        },
        authority: authority.clone(),
        attempts: vec![Attempt {
            attempt_id: attempt_id.clone(),
            number: 1,
            workspace: worktree_path.display().to_string(),
            started_at: Utc::now(),
            finished_at: None,
            outcome: AttemptOutcome::Running,
            provider_session_id: None,
            evidence: Vec::new(),
        }],
        candidates: Vec::new(),
        checks: Vec::new(),
        evaluations: Vec::new(),
        decisions: Vec::new(),
        publications: Vec::new(),
    };
    let packet = TaskPacket {
        schema_version: 1,
        slice_id: run_id.to_string(),
        work_item_id: work_item.id.to_string(),
        baseline: work_item.baseline.clone(),
        primary_convention: "repository-default".into(),
        convention_refs: vec!["repository-default".into()],
        stage: "implementation".into(),
        target_surfaces: work_item.declared_scope.clone(),
        behavioral_scope: vec![
            overrides
                .objective
                .clone()
                .unwrap_or_else(|| work_item.prompt.clone()),
        ],
        write_scope: work_item.declared_scope.clone(),
        protected_behavior: Vec::new(),
        excluded_capabilities: vec!["remote-publication".into()],
        dependencies: overrides.dependencies.clone().unwrap_or_default(),
        acceptance: overrides.acceptance.clone().unwrap_or_else(|| {
            vec![AcceptanceCriterion {
                id: "deterministic-checks".into(),
                capability: format!("validation-tier:{}", config.execution.check_tier),
                component: None,
                required: true,
            }]
        }),
        expected_capability_state: ExpectedCapabilityState::Satisfied,
        handoff: HandoffRequirements {
            candidate_required: true,
            changed_paths_required: true,
            evidence_required: true,
            unresolved_dependencies_required: true,
        },
        authority,
    };
    Ok((contract, packet))
}

fn required_acceptance_is_satisfied(
    acceptance: &[AcceptanceCriterion],
    checks: &[CheckResult],
) -> bool {
    acceptance
        .iter()
        .filter(|criterion| criterion.required)
        .all(|criterion| {
            checks.iter().any(|check| {
                check.required
                    && check.outcome == CheckOutcome::Passed
                    && check.check_id == criterion.id
                    && check.capability == criterion.capability
                    && criterion
                        .component
                        .as_ref()
                        .is_none_or(|component| check.component.as_ref() == Some(component))
            })
        })
}

fn write_contract_artifacts(run: &LocalRun, packet: &TaskPacket, directory: &Path) -> Result<()> {
    run.contract.write_to(&directory.join("run.json"))?;
    fs::write(
        directory.join("task-packet.json"),
        serde_json::to_vec_pretty(packet)?,
    )?;
    Ok(())
}

fn create_worktree(repository: &Path, worktree: &Path, baseline: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(worktree)
        .arg(baseline)
        .current_dir(repository)
        .output()
        .context("start git worktree add")?;
    if !status.status.success() {
        bail!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
    let actual = git(worktree, &["rev-parse", "HEAD"])?;
    if actual != baseline {
        bail!("worktree baseline mismatch: expected {baseline}, got {actual}");
    }
    if !git(worktree, &["status", "--porcelain"])?.is_empty() {
        bail!("new worktree is not clean");
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentVerificationEnvelope {
    schema_version: u8,
    operation: String,
    status: String,
    data: EnvironmentVerificationData,
    #[serde(default)]
    diagnostics: Vec<EnvironmentVerificationDiagnostic>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentVerificationData {
    action: String,
    fingerprint_version: String,
    profile: String,
    expected_fingerprint: Option<String>,
    verified_fingerprint: Option<String>,
}

#[derive(Deserialize)]
struct EnvironmentVerificationDiagnostic {
    #[serde(default)]
    code: Option<String>,
    message: String,
}

fn verify_environment(
    executable: &str,
    profile: EnvironmentProfile,
    worktree: &Path,
    run_directory: &Path,
) -> Result<()> {
    let stdout_path = run_directory.join("environment-verification.json");
    let stderr_path = run_directory.join("environment-verification.stderr.log");
    let output = Command::new(executable)
        .args([
            "environment",
            "verify",
            "--profile",
            profile.as_cli_value(),
            "--json",
        ])
        .current_dir(worktree)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            fs::write(&stderr_path, error.to_string())?;
            bail!("coding-tooling environment verification unavailable: {error}");
        }
    };
    fs::write(&stdout_path, &output.stdout)?;
    fs::write(&stderr_path, &output.stderr)?;
    let envelope: EnvironmentVerificationEnvelope = serde_json::from_slice(&output.stdout)
        .context("parse coding-tooling environment verification receipt")?;
    let diagnostics = environment_diagnostic_reason(&envelope.diagnostics);
    if !output.status.success() {
        bail!(
            "coding-tooling environment verification exited with {}{}",
            output.status,
            diagnostics
        );
    }
    if envelope.schema_version != 1
        || envelope.operation != "environment"
        || envelope.data.action != "verify"
        || envelope.data.fingerprint_version != "environment-fingerprint-v1"
    {
        bail!("unsupported coding-tooling environment verification receipt");
    }
    if envelope.data.profile != profile.as_cli_value() {
        bail!(
            "environment verification profile mismatch: expected {}, got {}",
            profile.as_cli_value(),
            envelope.data.profile
        );
    }
    if envelope.status != "passed" {
        bail!("environment did not verify{}", diagnostics);
    }
    let expected = envelope
        .data
        .expected_fingerprint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .context("environment verification receipt has no expectedFingerprint")?;
    let verified = envelope
        .data
        .verified_fingerprint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .context("environment verification receipt has no verifiedFingerprint")?;
    if expected != verified {
        bail!("environment fingerprint mismatch: expected {expected}, verified {verified}");
    }
    Ok(())
}

fn environment_diagnostic_reason(diagnostics: &[EnvironmentVerificationDiagnostic]) -> String {
    if diagnostics.is_empty() {
        return String::new();
    }
    let joined = diagnostics
        .iter()
        .map(|diagnostic| match diagnostic.code.as_deref() {
            Some(code) => format!("[{code}] {}", diagnostic.message),
            None => diagnostic.message.clone(),
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(": {joined}")
}

fn environment_verification_evidence(run_directory: &Path) -> Result<Vec<contracts::Evidence>> {
    let mut evidence = Vec::new();
    let stdout_path = run_directory.join("environment-verification.json");
    if stdout_path.exists() {
        evidence.push(contracts::evidence_for_file(
            "environment-verification",
            &stdout_path,
            "application/json",
        )?);
    }
    let stderr_path = run_directory.join("environment-verification.stderr.log");
    if stderr_path.exists() {
        evidence.push(contracts::evidence_for_file(
            "environment-verification-stderr",
            &stderr_path,
            "text/plain",
        )?);
    }
    Ok(evidence)
}

#[cfg(target_os = "linux")]
fn sandbox_provider_command(
    command: &CommandSpec,
    worktree: &Path,
    run_directory: &Path,
    candidate_object_directory: &Path,
) -> Result<CommandSpec> {
    let git_directory = PathBuf::from(git(worktree, &["rev-parse", "--absolute-git-dir"])?);
    let common_directory = PathBuf::from(git(worktree, &["rev-parse", "--git-common-dir"])?);
    let common_directory = if common_directory.is_absolute() {
        common_directory
    } else {
        worktree.join(common_directory).canonicalize()?
    };
    let baseline_object_directory = common_directory.join("objects");
    let scratch = run_directory.join("sandbox-tmp");
    let empty_hooks = run_directory.join("empty-hooks");
    fs::create_dir_all(&scratch)?;
    fs::create_dir_all(candidate_object_directory)?;
    fs::create_dir_all(&empty_hooks)?;
    for path in [
        worktree,
        git_directory.as_path(),
        baseline_object_directory.as_path(),
    ] {
        if !path.exists() {
            bail!("provider sandbox path {} does not exist", path.display());
        }
    }
    let mut args = vec![
        "--die-with-parent".into(),
        "--new-session".into(),
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
        "--dev".into(),
        "/dev".into(),
        "--proc".into(),
        "/proc".into(),
    ];
    for path in [
        worktree,
        git_directory.as_path(),
        scratch.as_path(),
        candidate_object_directory,
    ] {
        args.extend([
            "--bind".into(),
            path.as_os_str().to_owned(),
            path.as_os_str().to_owned(),
        ]);
    }
    args.extend([
        "--setenv".into(),
        "TMPDIR".into(),
        scratch.as_os_str().to_owned(),
        "--setenv".into(),
        "GIT_OBJECT_DIRECTORY".into(),
        candidate_object_directory.as_os_str().to_owned(),
        "--setenv".into(),
        "GIT_ALTERNATE_OBJECT_DIRECTORIES".into(),
        baseline_object_directory.as_os_str().to_owned(),
        "--setenv".into(),
        "GIT_CONFIG_COUNT".into(),
        "2".into(),
        "--setenv".into(),
        "GIT_CONFIG_KEY_0".into(),
        "core.hooksPath".into(),
        "--setenv".into(),
        "GIT_CONFIG_VALUE_0".into(),
        empty_hooks.as_os_str().to_owned(),
        "--setenv".into(),
        "GIT_CONFIG_KEY_1".into(),
        "credential.helper".into(),
        "--setenv".into(),
        "GIT_CONFIG_VALUE_1".into(),
        "".into(),
        "--setenv".into(),
        "GIT_SSH_COMMAND".into(),
        "/bin/false".into(),
        "--setenv".into(),
        "GIT_ASKPASS".into(),
        "/bin/false".into(),
        "--setenv".into(),
        "GIT_TERMINAL_PROMPT".into(),
        "0".into(),
        "--chdir".into(),
        worktree.as_os_str().to_owned(),
        "--".into(),
        command.program.clone(),
    ]);
    args.extend(command.args.iter().cloned());
    Ok(CommandSpec {
        program: "bwrap".into(),
        args,
        current_dir: worktree.to_owned(),
    })
}

#[cfg(target_os = "macos")]
fn sandbox_provider_command(
    command: &CommandSpec,
    worktree: &Path,
    run_directory: &Path,
    candidate_object_directory: &Path,
) -> Result<CommandSpec> {
    let git_directory = PathBuf::from(git(worktree, &["rev-parse", "--absolute-git-dir"])?);
    let common_directory = PathBuf::from(git(worktree, &["rev-parse", "--git-common-dir"])?);
    let common_directory = if common_directory.is_absolute() {
        common_directory
    } else {
        worktree.join(common_directory).canonicalize()?
    };
    let scratch = run_directory.join("sandbox-tmp");
    let empty_hooks = run_directory.join("empty-hooks");
    fs::create_dir_all(&scratch)?;
    fs::create_dir_all(candidate_object_directory)?;
    fs::create_dir_all(&empty_hooks)?;
    let escape = |path: &Path| path.display().to_string().replace('"', "\\\"");
    let profile = format!(
        "(version 1) (deny default) (allow process*) (allow network*) (allow file-read*) (allow sysctl-read) (allow mach-lookup) (allow file-write* (subpath \"{}\") (subpath \"{}\") (subpath \"{}\") (subpath \"{}\"))",
        escape(worktree),
        escape(&git_directory),
        escape(candidate_object_directory),
        escape(&scratch),
    );
    let mut args = vec![
        "-p".into(),
        profile.into(),
        "/usr/bin/env".into(),
        format!("TMPDIR={}", scratch.display()).into(),
        format!(
            "GIT_OBJECT_DIRECTORY={}",
            candidate_object_directory.display()
        )
        .into(),
        format!(
            "GIT_ALTERNATE_OBJECT_DIRECTORIES={}",
            common_directory.join("objects").display()
        )
        .into(),
        "GIT_CONFIG_COUNT=2".into(),
        "GIT_CONFIG_KEY_0=core.hooksPath".into(),
        format!("GIT_CONFIG_VALUE_0={}", empty_hooks.display()).into(),
        "GIT_CONFIG_KEY_1=credential.helper".into(),
        "GIT_CONFIG_VALUE_1=".into(),
        "GIT_SSH_COMMAND=/bin/false".into(),
        "GIT_ASKPASS=/bin/false".into(),
        "GIT_TERMINAL_PROMPT=0".into(),
        command.program.clone(),
    ];
    args.extend(command.args.iter().cloned());
    Ok(CommandSpec {
        program: "sandbox-exec".into(),
        args,
        current_dir: worktree.to_owned(),
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn sandbox_provider_command(
    _command: &CommandSpec,
    _worktree: &Path,
    _run_directory: &Path,
    _candidate_object_directory: &Path,
) -> Result<CommandSpec> {
    bail!("no supported fail-closed provider sandbox is available on this platform")
}

fn cleanup_worktree(repository: &Path, worktree: &Path) -> Result<()> {
    if !worktree.exists() {
        return Ok(());
    }
    let output = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(worktree)
        .current_dir(repository)
        .output()
        .context("start git worktree remove")?;
    if !output.status.success() {
        bail!(
            "git worktree remove failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if worktree.exists() {
        bail!(
            "git reported success but {} still exists",
            worktree.display()
        );
    }
    Ok(())
}

fn failure_with_cleanup(repository: &Path, worktree: &Path, message: String) -> String {
    match cleanup_worktree(repository, worktree) {
        Ok(()) => message,
        Err(error) => format!("{message}; cleanup failed: {error}"),
    }
}

fn validate_candidate(
    work_item: &WorkItem,
    worktree: &Path,
    candidate_object_directory: &Path,
    run_id: Uuid,
) -> Result<Candidate> {
    let candidate_sha =
        git_with_objects(worktree, candidate_object_directory, &["rev-parse", "HEAD"])?;
    if candidate_sha == work_item.baseline.git_sha {
        bail!("candidate missing: provider did not create a commit");
    }
    if !git_with_objects(
        worktree,
        candidate_object_directory,
        &["status", "--porcelain"],
    )?
    .is_empty()
    {
        bail!("candidate invalid: provider left uncommitted changes");
    }
    git_with_objects(
        worktree,
        candidate_object_directory,
        &[
            "merge-base",
            "--is-ancestor",
            &work_item.baseline.git_sha,
            &candidate_sha,
        ],
    )?;
    let changed_paths: Vec<String> = git_with_objects(
        worktree,
        candidate_object_directory,
        &[
            "diff",
            "--name-only",
            &work_item.baseline.git_sha,
            &candidate_sha,
        ],
    )?
    .lines()
    .map(str::to_owned)
    .collect();
    if changed_paths.is_empty() {
        bail!("candidate missing: commit has no changes from the baseline");
    }
    if let Some(path) = changed_paths
        .iter()
        .find(|path| !in_declared_scope(path, &work_item.declared_scope))
    {
        bail!("candidate scope mismatch: `{path}` is outside the declared write scope");
    }
    Ok(Candidate {
        schema_version: 1,
        candidate_id: format!("{run_id}-candidate-1"),
        kind: CandidateKind::GitCommit,
        baseline_git_sha: work_item.baseline.git_sha.clone(),
        git_sha: Some(candidate_sha),
        patch_digest: None,
        produced_by_attempt_id: format!("{run_id}-attempt-1"),
        changed_paths,
    })
}

fn create_candidate_ref(
    repository: &Path,
    worktree: &Path,
    run_directory: &Path,
    candidate_object_directory: &Path,
    run_id: Uuid,
    candidate: &str,
) -> Result<()> {
    let bundle = run_directory.join("candidate.bundle");
    git_with_objects(
        worktree,
        candidate_object_directory,
        &["bundle", "create", &bundle.display().to_string(), "HEAD"],
    )?;
    git_ok(
        repository,
        &[
            "fetch",
            "--no-tags",
            &bundle.display().to_string(),
            &format!("HEAD:refs/agent-loop/candidates/{run_id}"),
        ],
    )?;
    let recorded = git(
        repository,
        &["rev-parse", &format!("refs/agent-loop/candidates/{run_id}")],
    )?;
    if recorded != candidate {
        bail!("persisted candidate ref does not match validated candidate");
    }
    Ok(())
}

struct CodingToolingAdapter<'a> {
    executable: &'a str,
    tier: &'a str,
}

impl CodingToolingAdapter<'_> {
    fn run(&self, worktree: &Path, run_directory: &Path, candidate_sha: &str) -> Vec<CheckResult> {
        let started_at = Utc::now();
        match self.try_run(worktree, run_directory, candidate_sha, started_at) {
            Ok(results) if !results.is_empty() => results,
            Ok(_) => vec![synthetic_check(
                CheckOutcome::Error,
                "coding-tooling returned no check results".into(),
                started_at,
                candidate_sha,
                Vec::new(),
            )],
            Err(error) => vec![synthetic_check(
                CheckOutcome::Error,
                format!("coding-tooling response malformed or evidence unavailable: {error}"),
                started_at,
                candidate_sha,
                Vec::new(),
            )],
        }
    }

    fn try_run(
        &self,
        worktree: &Path,
        run_directory: &Path,
        candidate_sha: &str,
        started_at: DateTime<Utc>,
    ) -> Result<Vec<CheckResult>> {
        let output = Command::new(self.executable)
            .args(["run", "--tier", self.tier, "--strict", "--json"])
            .current_dir(worktree)
            .output();
        match output {
            Err(error) => {
                let reason = format!("coding-tooling unavailable: {error}");
                let path = run_directory.join("coding-tooling-error.json");
                fs::write(
                    &path,
                    serde_json::to_vec_pretty(&serde_json::json!({ "error": reason }))?,
                )?;
                Ok(vec![synthetic_check(
                    CheckOutcome::Unavailable,
                    reason,
                    started_at,
                    candidate_sha,
                    vec![contracts::evidence_for_file(
                        "coding-tooling-error",
                        &path,
                        "application/json",
                    )?],
                )])
            }
            Ok(output) => {
                let stdout_path = run_directory.join("coding-tooling.json");
                let stderr_path = run_directory.join("coding-tooling.stderr.log");
                fs::write(&stdout_path, &output.stdout)?;
                fs::write(&stderr_path, &output.stderr)?;
                let evidence = vec![
                    contracts::evidence_for_file(
                        "coding-tooling-result",
                        &stdout_path,
                        "application/json",
                    )?,
                    contracts::evidence_for_file(
                        "coding-tooling-stderr",
                        &stderr_path,
                        "text/plain",
                    )?,
                ];
                let mut results =
                    parse_tooling_results(&output.stdout, candidate_sha, started_at, &evidence)?;
                if !output.status.success() {
                    results.push(synthetic_check(
                        CheckOutcome::Error,
                        format!("coding-tooling exited with {}", output.status),
                        started_at,
                        candidate_sha,
                        evidence,
                    ));
                }
                Ok(results)
            }
        }
    }
}

fn parse_tooling_results(
    stdout: &[u8],
    candidate_sha: &str,
    started_at: DateTime<Utc>,
    evidence: &[contracts::Evidence],
) -> Result<Vec<CheckResult>> {
    let value: serde_json::Value = serde_json::from_slice(stdout)?;
    let mut results = if let Ok(result) = serde_json::from_value::<CheckResult>(value.clone()) {
        vec![result]
    } else if let Ok(results) = serde_json::from_value::<Vec<CheckResult>>(value.clone()) {
        results
    } else {
        parse_legacy_tooling(value, candidate_sha, started_at, evidence)?
    };
    for result in &mut results {
        validate_check_result(result)?;
        if result.candidate.kind != CandidateKind::GitCommit
            || result.candidate.identity != candidate_sha
        {
            result.outcome = CheckOutcome::Error;
            result.reason = Some(format!(
                "check candidate mismatch: expected {candidate_sha}, got {}",
                result.candidate.identity
            ));
            result.candidate = CandidateIdentity {
                kind: CandidateKind::GitCommit,
                identity: candidate_sha.into(),
            };
        }
        result.evidence.extend(evidence.iter().cloned());
    }
    Ok(results)
}

fn validate_check_result(result: &CheckResult) -> Result<()> {
    if result.schema_version != 1 {
        bail!(
            "unsupported check-result schema version {}",
            result.schema_version
        );
    }
    for (label, value) in [
        ("checkId", result.check_id.as_str()),
        ("capability", result.capability.as_str()),
        ("candidate.identity", result.candidate.identity.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("check-result {label} cannot be empty");
        }
    }
    if result
        .component
        .as_deref()
        .is_some_and(|component| component.trim().is_empty())
    {
        bail!("check-result component cannot be empty");
    }
    if result.finished_at < result.started_at {
        bail!("check-result finishedAt precedes startedAt");
    }
    for evidence in &result.evidence {
        if evidence.schema_version != 1
            || evidence.kind.is_empty()
            || evidence.uri.is_empty()
            || evidence
                .media_type
                .as_deref()
                .is_some_and(|media_type| media_type.trim().is_empty())
            || !valid_digest(&evidence.digest)
        {
            bail!("check-result contains invalid evidence");
        }
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    let Some((algorithm, encoded)) = value.split_once(':') else {
        return false;
    };
    !algorithm.is_empty()
        && algorithm.chars().enumerate().all(|(index, character)| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || (index > 0 && matches!(character, '+' | '.' | '_' | '-'))
        })
        && !encoded.is_empty()
        && encoded.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | '_' | '~' | '+' | '/' | '=' | '-')
        })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyEnvelope {
    schema_version: u8,
    operation: String,
    status: LegacyStatus,
    duration_ms: u64,
    data: LegacyData,
    #[serde(default)]
    diagnostics: Vec<LegacyDiagnostic>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyData {
    #[serde(default)]
    results: Vec<LegacyResult>,
    #[serde(default)]
    missing: Vec<LegacyMissing>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyResult {
    capability: String,
    component: String,
    status: LegacyStatus,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyMissing {
    capability: String,
    component: String,
    optional: bool,
}

#[derive(Deserialize)]
struct LegacyDiagnostic {
    message: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LegacyStatus {
    Passed,
    Failed,
    Unavailable,
    Error,
}

fn parse_legacy_tooling(
    value: serde_json::Value,
    candidate_sha: &str,
    started_at: DateTime<Utc>,
    evidence: &[contracts::Evidence],
) -> Result<Vec<CheckResult>> {
    let envelope: LegacyEnvelope = serde_json::from_value(value)?;
    if envelope.schema_version != 1 || envelope.operation != "run" {
        bail!("unsupported coding-tooling envelope");
    }
    let finished_at = Utc::now();
    let diagnostic_reason = (!envelope.diagnostics.is_empty()).then(|| {
        envelope
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    });
    let mut checks = envelope
        .data
        .results
        .into_iter()
        .enumerate()
        .map(|(index, item)| CheckResult {
            schema_version: 1,
            check_id: format!("coding-tooling-{}-{index}", item.capability),
            capability: item.capability,
            component: Some(item.component),
            candidate: CandidateIdentity {
                kind: CandidateKind::GitCommit,
                identity: candidate_sha.into(),
            },
            outcome: legacy_outcome(item.status),
            required: true,
            started_at,
            finished_at,
            duration_ms: item.duration_ms,
            exit_code: item.exit_code,
            reason: item.error.or_else(|| diagnostic_reason.clone()),
            evidence: evidence.to_vec(),
        })
        .collect::<Vec<_>>();
    checks.extend(envelope.data.missing.into_iter().map(|item| CheckResult {
        schema_version: 1,
        check_id: format!("coding-tooling-{}-unavailable", item.capability),
        capability: item.capability,
        component: Some(item.component),
        candidate: CandidateIdentity {
            kind: CandidateKind::GitCommit,
            identity: candidate_sha.into(),
        },
        outcome: CheckOutcome::Unavailable,
        required: !item.optional,
        started_at,
        finished_at,
        duration_ms: Some(0),
        exit_code: None,
        reason: diagnostic_reason.clone(),
        evidence: evidence.to_vec(),
    }));
    if !matches!(envelope.status, LegacyStatus::Passed) {
        checks.push(synthetic_check(
            legacy_outcome(envelope.status),
            diagnostic_reason.unwrap_or_else(|| "coding-tooling run did not pass".into()),
            started_at,
            candidate_sha,
            evidence.to_vec(),
        ));
    }
    let _ = envelope.duration_ms;
    Ok(checks)
}

fn legacy_outcome(status: LegacyStatus) -> CheckOutcome {
    match status {
        LegacyStatus::Passed => CheckOutcome::Passed,
        LegacyStatus::Failed => CheckOutcome::Failed,
        LegacyStatus::Unavailable => CheckOutcome::Unavailable,
        LegacyStatus::Error => CheckOutcome::Error,
    }
}

fn synthetic_check(
    outcome: CheckOutcome,
    reason: String,
    started_at: DateTime<Utc>,
    candidate_sha: &str,
    evidence: Vec<contracts::Evidence>,
) -> CheckResult {
    CheckResult {
        schema_version: 1,
        check_id: "coding-tooling-adapter".into(),
        capability: "validation-tier".into(),
        component: None,
        candidate: CandidateIdentity {
            kind: CandidateKind::GitCommit,
            identity: candidate_sha.into(),
        },
        outcome,
        required: true,
        started_at,
        finished_at: Utc::now(),
        duration_ms: None,
        exit_code: None,
        reason: Some(reason),
        evidence,
    }
}

fn verify_integration_preconditions(
    work_item: &WorkItem,
    run_id: Uuid,
    candidate_sha: &str,
) -> Result<()> {
    let candidate_ref = git(
        &work_item.repository_root,
        &["rev-parse", &format!("refs/agent-loop/candidates/{run_id}")],
    )?;
    if candidate_ref != candidate_sha {
        bail!("candidate identity mismatch before integration");
    }
    let current_target = git(
        &work_item.repository_root,
        &[
            "rev-parse",
            &format!("refs/heads/{}", work_item.target_branch),
        ],
    )?;
    if current_target != work_item.baseline.git_sha {
        bail!(
            "integration baseline mismatch: target {} is {}, expected {}",
            work_item.target_branch,
            current_target,
            work_item.baseline.git_sha
        );
    }
    git_ok(
        &work_item.repository_root,
        &[
            "merge-base",
            "--is-ancestor",
            &work_item.baseline.git_sha,
            candidate_sha,
        ],
    )
}

fn integrate_candidate(work_item: &WorkItem, candidate_sha: &str) -> Result<()> {
    if let Some(target_worktree) =
        branch_worktree(&work_item.repository_root, &work_item.target_branch)?
    {
        if !git(&target_worktree, &["status", "--porcelain"])?.is_empty() {
            bail!("target worktree {} is not clean", target_worktree.display());
        }
        git_ok(
            &target_worktree,
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "merge",
                "--ff-only",
                candidate_sha,
            ],
        )?;
    } else {
        git_ok(
            &work_item.repository_root,
            &[
                "update-ref",
                &format!("refs/heads/{}", work_item.target_branch),
                candidate_sha,
                &work_item.baseline.git_sha,
            ],
        )?;
    }
    let integrated = git(
        &work_item.repository_root,
        &[
            "rev-parse",
            &format!("refs/heads/{}", work_item.target_branch),
        ],
    )?;
    if integrated != candidate_sha {
        bail!("local integration did not produce the approved candidate");
    }
    Ok(())
}

fn branch_worktree(repository: &Path, branch: &str) -> Result<Option<PathBuf>> {
    let output = git(repository, &["worktree", "list", "--porcelain"])?;
    let wanted = format!("refs/heads/{branch}");
    for record in output.split("\n\n") {
        let mut path = None;
        let mut branch_name = None;
        for line in record.lines() {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(value));
            } else if let Some(value) = line.strip_prefix("branch ") {
                branch_name = Some(value);
            }
        }
        if branch_name == Some(wanted.as_str()) {
            return Ok(path);
        }
    }
    Ok(None)
}

fn validate_scope(scope: &[String]) -> Result<()> {
    if scope.is_empty() {
        bail!("declared scope cannot be empty");
    }
    let mut seen = std::collections::BTreeSet::new();
    for value in scope {
        validate_nonempty("declared scope entry", value)?;
        let path = Path::new(value);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
            || value == ".git"
            || value.starts_with(".git/")
        {
            bail!("invalid declared scope `{value}`");
        }
        if !seen.insert(value) {
            bail!("duplicate declared scope `{value}`");
        }
    }
    Ok(())
}

fn in_declared_scope(path: &str, scope: &[String]) -> bool {
    scope.iter().any(|entry| {
        entry == "."
            || path == entry
            || path
                .strip_prefix(entry)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn validate_nonempty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} cannot be empty");
    }
    Ok(())
}

fn git(repository: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .output()
        .with_context(|| format!("run git {}", arguments.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn git_with_objects(
    repository: &Path,
    candidate_object_directory: &Path,
    arguments: &[&str],
) -> Result<String> {
    let common_directory = PathBuf::from(git(repository, &["rev-parse", "--git-common-dir"])?);
    let common_directory = if common_directory.is_absolute() {
        common_directory
    } else {
        repository.join(common_directory).canonicalize()?
    };
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .env("GIT_OBJECT_DIRECTORY", candidate_object_directory)
        .env(
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            common_directory.join("objects"),
        )
        .output()
        .with_context(|| format!("run git {}", arguments.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn git_ok(repository: &Path, arguments: &[&str]) -> Result<()> {
    git(repository, arguments).map(|_| ())
}
