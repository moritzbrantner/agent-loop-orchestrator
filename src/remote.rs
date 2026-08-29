use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    config::{MergeMethod, ProjectConfig, RemoteConfig},
    contracts::{Publication, PublicationKind, PublicationStatus},
    control,
    execution::{CreateWorkItem, DecisionRequest, ExecutionService, LocalRunStatus},
    repository::RegisteredProject,
};

const STATE_FILE: &str = "remote-state.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Pending,
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mergeability {
    Mergeable,
    Conflicting,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestSnapshot {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub state: String,
    pub draft: bool,
    pub head_ref: String,
    pub head_sha: String,
    pub head_repository: String,
    pub base_ref: String,
    pub cross_repository: bool,
    pub checks: CheckState,
    pub failed_checks: Vec<FailedCheck>,
    pub mergeability: Mergeability,
    pub merge_state: String,
    pub review_decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedCheck {
    pub name: String,
    pub conclusion: String,
    pub details_url: Option<String>,
}

pub trait RemoteHost {
    fn open_pull_requests(&self, repository: &str) -> Result<Vec<u64>>;
    fn pull_request(&self, repository: &str, number: u64) -> Result<PullRequestSnapshot>;
    fn failure_details(
        &self,
        repository: &str,
        pull_request: &PullRequestSnapshot,
    ) -> Result<String>;
    fn merge(
        &self,
        repository: &str,
        number: u64,
        expected_head_sha: &str,
        method: MergeMethod,
    ) -> Result<()>;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepairReason {
    FailedChecks,
    MergeConflict,
}

pub struct RepairRequest<'a> {
    pub project: &'a RegisteredProject,
    pub config: &'a ProjectConfig,
    pub repository: &'a str,
    pub pull_request: &'a PullRequestSnapshot,
    pub reason: RepairReason,
    pub failure_details: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum RepairResult {
    Published {
        run_id: Uuid,
        head_sha: String,
    },
    Failed {
        run_id: Option<Uuid>,
        reason: String,
    },
    Deferred {
        reason: String,
    },
}

pub trait RepairExecutor {
    fn repair(&mut self, request: RepairRequest<'_>) -> Result<RepairResult>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum ReconcileOutcome {
    Error {
        number: Option<u64>,
        reason: String,
    },
    Waiting {
        number: u64,
        head_sha: String,
        reason: String,
    },
    WouldMerge {
        number: u64,
        head_sha: String,
    },
    Merged {
        number: u64,
        head_sha: String,
    },
    WouldRepair {
        number: u64,
        head_sha: String,
        reason: RepairReason,
    },
    Repaired {
        number: u64,
        previous_head_sha: String,
        head_sha: String,
        run_id: Uuid,
    },
    RepairFailed {
        number: u64,
        head_sha: String,
        run_id: Option<Uuid>,
        reason: String,
    },
    Deferred {
        number: u64,
        head_sha: String,
        reason: String,
    },
    NeedsHuman {
        number: u64,
        head_sha: String,
        reason: String,
    },
}

pub struct RemotePullRequestLoop<'a> {
    data_root: PathBuf,
    host: &'a dyn RemoteHost,
    repairs: &'a mut dyn RepairExecutor,
}

impl<'a> RemotePullRequestLoop<'a> {
    pub fn new(
        data_root: impl Into<PathBuf>,
        host: &'a dyn RemoteHost,
        repairs: &'a mut dyn RepairExecutor,
    ) -> Self {
        Self {
            data_root: data_root.into(),
            host,
            repairs,
        }
    }

    pub fn reconcile(
        &mut self,
        project: &RegisteredProject,
        config: &ProjectConfig,
        dry_run: bool,
    ) -> Result<Vec<ReconcileOutcome>> {
        if !config.remote.enabled {
            bail!(
                "remote automation is disabled for project `{}`",
                config.project.id
            );
        }
        let repository = config.remote.repository()?;
        let store = RemoteStateStore::new(&self.data_root)?;
        let lock = store.lock_exclusive()?;
        let mut state = store.load()?;
        let mut outcomes = Vec::new();
        for number in self.host.open_pull_requests(repository)? {
            let outcome = self
                .host
                .pull_request(repository, number)
                .and_then(|pull_request| {
                    self.reconcile_one(
                        project,
                        config,
                        repository,
                        pull_request,
                        dry_run,
                        &store,
                        &mut state,
                    )
                })
                .unwrap_or_else(|error| ReconcileOutcome::Error {
                    number: Some(number),
                    reason: format!("{error:#}"),
                });
            outcomes.push(outcome);
        }
        if !dry_run {
            store.write(&state)?;
        }
        FileExt::unlock(&lock)?;
        Ok(outcomes)
    }

    #[allow(clippy::too_many_arguments)]
    fn reconcile_one(
        &mut self,
        project: &RegisteredProject,
        config: &ProjectConfig,
        repository: &str,
        pull_request: PullRequestSnapshot,
        dry_run: bool,
        store: &RemoteStateStore,
        state: &mut RemoteState,
    ) -> Result<ReconcileOutcome> {
        let key = format!("{repository}#{}", pull_request.number);
        state
            .pull_requests
            .entry(key.clone())
            .or_default()
            .observe(&pull_request);

        if pull_request.state != "OPEN" {
            return Ok(waiting(&pull_request, "pull request is not open"));
        }
        if !trusted_author(&config.remote, &pull_request.author) {
            return Ok(needs_human(
                &pull_request,
                format!(
                    "author `{}` is not listed in remote.trusted_authors",
                    pull_request.author
                ),
            ));
        }
        if pull_request.draft {
            return Ok(waiting(&pull_request, "pull request is a draft"));
        }

        let repair_attempts = state
            .pull_requests
            .get(&key)
            .context("remote pull request record disappeared")?
            .repair_attempts;
        let action = classify(&pull_request, &config.remote, repair_attempts);
        match action {
            PolicyAction::Wait(reason) => Ok(waiting(&pull_request, reason)),
            PolicyAction::NeedsHuman(reason) => Ok(needs_human(&pull_request, reason)),
            PolicyAction::Merge => {
                if dry_run {
                    return Ok(ReconcileOutcome::WouldMerge {
                        number: pull_request.number,
                        head_sha: pull_request.head_sha,
                    });
                }
                self.host.merge(
                    repository,
                    pull_request.number,
                    &pull_request.head_sha,
                    config.remote.merge_method,
                )?;
                let record = state
                    .pull_requests
                    .get_mut(&key)
                    .context("remote pull request record disappeared")?;
                record.last_action = Some("merged".into());
                record.updated_at = Some(Utc::now());
                store.write(state)?;
                Ok(ReconcileOutcome::Merged {
                    number: pull_request.number,
                    head_sha: pull_request.head_sha,
                })
            }
            PolicyAction::Repair(reason) => {
                if pull_request.cross_repository || pull_request.head_repository != repository {
                    return Ok(needs_human(
                        &pull_request,
                        "automatic repair cannot publish to a cross-repository pull request",
                    ));
                }
                if dry_run {
                    return Ok(ReconcileOutcome::WouldRepair {
                        number: pull_request.number,
                        head_sha: pull_request.head_sha,
                        reason,
                    });
                }
                let failure_details = if reason == RepairReason::FailedChecks {
                    self.host.failure_details(repository, &pull_request)?
                } else {
                    String::new()
                };
                {
                    let record = state
                        .pull_requests
                        .get_mut(&key)
                        .context("remote pull request record disappeared")?;
                    record.repair_attempts += 1;
                    record.last_action = Some("repairing".into());
                    record.updated_at = Some(Utc::now());
                }
                store.write(state)?;
                let repair = self.repairs.repair(RepairRequest {
                    project,
                    config,
                    repository,
                    pull_request: &pull_request,
                    reason,
                    failure_details: &failure_details,
                });
                let repair = match repair {
                    Ok(repair) => repair,
                    Err(error) => {
                        let record = state
                            .pull_requests
                            .get_mut(&key)
                            .context("remote pull request record disappeared")?;
                        record.repair_attempts -= 1;
                        record.last_action = Some("repair_error".into());
                        record.updated_at = Some(Utc::now());
                        store.write(state)?;
                        return Err(error);
                    }
                };
                match repair {
                    RepairResult::Published { run_id, head_sha } => {
                        let record = state
                            .pull_requests
                            .get_mut(&key)
                            .context("remote pull request record disappeared")?;
                        record.last_published_sha = Some(head_sha.clone());
                        record.last_action = Some("repaired".into());
                        record.updated_at = Some(Utc::now());
                        store.write(state)?;
                        Ok(ReconcileOutcome::Repaired {
                            number: pull_request.number,
                            previous_head_sha: pull_request.head_sha,
                            head_sha,
                            run_id,
                        })
                    }
                    RepairResult::Failed { run_id, reason } => {
                        let record = state
                            .pull_requests
                            .get_mut(&key)
                            .context("remote pull request record disappeared")?;
                        record.last_action = Some("repair_failed".into());
                        record.updated_at = Some(Utc::now());
                        store.write(state)?;
                        Ok(ReconcileOutcome::RepairFailed {
                            number: pull_request.number,
                            head_sha: pull_request.head_sha,
                            run_id,
                            reason,
                        })
                    }
                    RepairResult::Deferred { reason } => {
                        let record = state
                            .pull_requests
                            .get_mut(&key)
                            .context("remote pull request record disappeared")?;
                        record.repair_attempts -= 1;
                        record.last_action = Some("deferred".into());
                        record.updated_at = Some(Utc::now());
                        store.write(state)?;
                        Ok(ReconcileOutcome::Deferred {
                            number: pull_request.number,
                            head_sha: pull_request.head_sha,
                            reason,
                        })
                    }
                }
            }
        }
    }
}

enum PolicyAction {
    Wait(String),
    Merge,
    Repair(RepairReason),
    NeedsHuman(String),
}

fn classify(
    pull_request: &PullRequestSnapshot,
    config: &RemoteConfig,
    repair_attempts: u32,
) -> PolicyAction {
    match pull_request.checks {
        CheckState::Pending => PolicyAction::Wait("required checks are pending".into()),
        CheckState::Failed => repair_action(
            config.repair_failures,
            RepairReason::FailedChecks,
            config.max_repair_attempts,
            repair_attempts,
            "required checks failed",
        ),
        CheckState::Passed => match pull_request.mergeability {
            Mergeability::Unknown => {
                PolicyAction::Wait("remote mergeability is still being computed".into())
            }
            Mergeability::Conflicting => repair_action(
                config.repair_conflicts,
                RepairReason::MergeConflict,
                config.max_repair_attempts,
                repair_attempts,
                "pull request conflicts with its base branch",
            ),
            Mergeability::Mergeable => {
                if pull_request.merge_state != "CLEAN" {
                    PolicyAction::NeedsHuman(format!(
                        "remote merge policy is `{}` rather than `CLEAN`",
                        pull_request.merge_state
                    ))
                } else if config.auto_merge {
                    PolicyAction::Merge
                } else {
                    PolicyAction::Wait("remote.auto_merge is disabled".into())
                }
            }
        },
    }
}

fn repair_action(
    enabled: bool,
    reason: RepairReason,
    maximum: u32,
    attempts: u32,
    disabled_reason: &str,
) -> PolicyAction {
    if !enabled {
        PolicyAction::NeedsHuman(format!("{disabled_reason}; automatic repair is disabled"))
    } else if attempts >= maximum {
        PolicyAction::NeedsHuman(format!(
            "{disabled_reason}; automatic repair reached the configured limit of {maximum}"
        ))
    } else {
        PolicyAction::Repair(reason)
    }
}

fn trusted_author(config: &RemoteConfig, author: &str) -> bool {
    config
        .trusted_authors
        .iter()
        .any(|trusted| trusted.eq_ignore_ascii_case(author))
}

fn waiting(pull_request: &PullRequestSnapshot, reason: impl Into<String>) -> ReconcileOutcome {
    ReconcileOutcome::Waiting {
        number: pull_request.number,
        head_sha: pull_request.head_sha.clone(),
        reason: reason.into(),
    }
}

fn needs_human(pull_request: &PullRequestSnapshot, reason: impl Into<String>) -> ReconcileOutcome {
    ReconcileOutcome::NeedsHuman {
        number: pull_request.number,
        head_sha: pull_request.head_sha.clone(),
        reason: reason.into(),
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteState {
    #[serde(default)]
    pull_requests: BTreeMap<String, PullRequestRecord>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestRecord {
    last_observed_sha: Option<String>,
    last_published_sha: Option<String>,
    #[serde(default)]
    repair_attempts: u32,
    last_action: Option<String>,
    updated_at: Option<DateTime<Utc>>,
}

impl PullRequestRecord {
    fn observe(&mut self, pull_request: &PullRequestSnapshot) {
        if self.last_observed_sha.as_deref() != Some(&pull_request.head_sha) {
            if self.last_published_sha.as_deref() != Some(&pull_request.head_sha) {
                self.repair_attempts = 0;
            }
            self.last_observed_sha = Some(pull_request.head_sha.clone());
            self.updated_at = Some(Utc::now());
        }
    }
}

struct RemoteStateStore {
    state_path: PathBuf,
    lock_path: PathBuf,
}

impl RemoteStateStore {
    fn new(data_root: &Path) -> Result<Self> {
        fs::create_dir_all(data_root)
            .with_context(|| format!("create remote state directory {}", data_root.display()))?;
        Ok(Self {
            state_path: data_root.join(STATE_FILE),
            lock_path: data_root.join("remote-state.lock"),
        })
    }

    fn lock_exclusive(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.lock_path)
            .with_context(|| format!("open {}", self.lock_path.display()))?;
        FileExt::lock_exclusive(&file)?;
        Ok(file)
    }

    fn load(&self) -> Result<RemoteState> {
        if !self.state_path.exists() {
            return Ok(RemoteState::default());
        }
        serde_json::from_slice(&fs::read(&self.state_path)?)
            .with_context(|| format!("parse {}", self.state_path.display()))
    }

    fn write(&self, state: &RemoteState) -> Result<()> {
        let temporary = self.state_path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(state)?)
            .with_context(|| format!("write {}", temporary.display()))?;
        fs::rename(&temporary, &self.state_path)
            .with_context(|| format!("replace {}", self.state_path.display()))
    }
}

pub struct GitHubRemoteHost {
    executable: String,
}

impl GitHubRemoteHost {
    pub fn new(executable: impl Into<String>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    fn output(&self, arguments: &[&str]) -> Result<Output> {
        Command::new(&self.executable)
            .args(arguments)
            .output()
            .with_context(|| format!("run {} {}", self.executable, arguments.join(" ")))
    }

    fn successful_output(&self, arguments: &[&str]) -> Result<Vec<u8>> {
        let output = self.output(arguments)?;
        if !output.status.success() {
            bail!(
                "{} {} failed: {}",
                self.executable,
                arguments.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(output.stdout)
    }
}

impl RemoteHost for GitHubRemoteHost {
    fn open_pull_requests(&self, repository: &str) -> Result<Vec<u64>> {
        let output = self.successful_output(&[
            "pr", "list", "--repo", repository, "--state", "open", "--limit", "100", "--json",
            "number",
        ])?;
        let rows: Vec<NumberRow> =
            serde_json::from_slice(&output).context("parse GitHub pull request list")?;
        Ok(rows.into_iter().map(|row| row.number).collect())
    }

    fn pull_request(&self, repository: &str, number: u64) -> Result<PullRequestSnapshot> {
        let number = number.to_string();
        let output = self.successful_output(&[
            "pr",
            "view",
            &number,
            "--repo",
            repository,
            "--json",
            "number,title,state,isDraft,headRefName,headRefOid,headRepository,headRepositoryOwner,isCrossRepository,baseRefName,mergeable,mergeStateStatus,statusCheckRollup,reviewDecision,author,url",
        ])?;
        let row: GitHubPullRequest =
            serde_json::from_slice(&output).context("parse GitHub pull request")?;
        row.try_into()
    }

    fn failure_details(
        &self,
        repository: &str,
        pull_request: &PullRequestSnapshot,
    ) -> Result<String> {
        let mut details = String::new();
        for check in &pull_request.failed_checks {
            details.push_str(&format!(
                "Check `{}` concluded `{}`.\n",
                check.name, check.conclusion
            ));
            let Some(run_id) = check.details_url.as_deref().and_then(github_run_id) else {
                continue;
            };
            let output =
                self.output(&["run", "view", run_id, "--repo", repository, "--log-failed"])?;
            details.push_str(&String::from_utf8_lossy(&output.stdout));
            details.push_str(&String::from_utf8_lossy(&output.stderr));
            if details.len() > 60_000 {
                details.truncate(60_000);
                details.push_str("\n[failed-check output truncated by agent-loop]\n");
                break;
            }
        }
        if details.is_empty() {
            details
                .push_str("GitHub reported failed checks but exposed no failed-check log output.");
        }
        Ok(details)
    }

    fn merge(
        &self,
        repository: &str,
        number: u64,
        expected_head_sha: &str,
        method: MergeMethod,
    ) -> Result<()> {
        let number_text = number.to_string();
        self.successful_output(&[
            "pr",
            "merge",
            &number_text,
            "--repo",
            repository,
            "--match-head-commit",
            expected_head_sha,
            method.as_gh_flag(),
        ])?;
        let observed = self.pull_request(repository, number)?;
        if observed.state != "MERGED" {
            bail!(
                "GitHub merge command completed but pull request #{} is `{}`",
                number_text,
                observed.state
            );
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct NumberRow {
    number: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubPullRequest {
    number: u64,
    title: String,
    state: String,
    is_draft: bool,
    head_ref_name: String,
    head_ref_oid: String,
    head_repository: GitHubRepository,
    head_repository_owner: GitHubAuthor,
    is_cross_repository: bool,
    base_ref_name: String,
    mergeable: String,
    merge_state_status: String,
    status_check_rollup: Vec<Value>,
    review_decision: String,
    author: GitHubAuthor,
    url: String,
}

#[derive(Deserialize)]
struct GitHubRepository {
    name: String,
}

#[derive(Deserialize)]
struct GitHubAuthor {
    login: String,
}

impl TryFrom<GitHubPullRequest> for PullRequestSnapshot {
    type Error = anyhow::Error;

    fn try_from(row: GitHubPullRequest) -> Result<Self> {
        let mut pending = false;
        let mut failed_checks = Vec::new();
        for check in &row.status_check_rollup {
            let name = check
                .get("name")
                .or_else(|| check.get("context"))
                .and_then(Value::as_str)
                .unwrap_or("unnamed check")
                .to_owned();
            let status = check
                .get("status")
                .or_else(|| check.get("state"))
                .and_then(Value::as_str)
                .unwrap_or("UNKNOWN");
            let conclusion = check
                .get("conclusion")
                .or_else(|| check.get("state"))
                .and_then(Value::as_str)
                .unwrap_or("UNKNOWN");
            if !matches!(status, "COMPLETED" | "SUCCESS" | "FAILURE" | "ERROR") {
                pending = true;
            }
            if !matches!(conclusion, "SUCCESS" | "NEUTRAL" | "SKIPPED") {
                if matches!(conclusion, "PENDING" | "EXPECTED" | "UNKNOWN" | "") {
                    pending = true;
                } else {
                    failed_checks.push(FailedCheck {
                        name,
                        conclusion: conclusion.into(),
                        details_url: check
                            .get("detailsUrl")
                            .or_else(|| check.get("targetUrl"))
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    });
                }
            }
        }
        let checks = if row.status_check_rollup.is_empty() || pending {
            CheckState::Pending
        } else if failed_checks.is_empty() {
            CheckState::Passed
        } else {
            CheckState::Failed
        };
        let mergeability = match row.mergeable.as_str() {
            "MERGEABLE" => Mergeability::Mergeable,
            "CONFLICTING" => Mergeability::Conflicting,
            "UNKNOWN" => Mergeability::Unknown,
            other => bail!("unknown GitHub mergeability `{other}`"),
        };
        Ok(Self {
            number: row.number,
            title: row.title,
            url: row.url,
            author: row.author.login,
            state: row.state,
            draft: row.is_draft,
            head_ref: row.head_ref_name,
            head_sha: row.head_ref_oid,
            head_repository: format!(
                "{}/{}",
                row.head_repository_owner.login, row.head_repository.name
            ),
            base_ref: row.base_ref_name,
            cross_repository: row.is_cross_repository,
            checks,
            failed_checks,
            mergeability,
            merge_state: row.merge_state_status,
            review_decision: (!row.review_decision.is_empty()).then_some(row.review_decision),
        })
    }
}

fn github_run_id(url: &str) -> Option<&str> {
    let tail = url.split("/actions/runs/").nth(1)?;
    tail.split('/').next().filter(|value| !value.is_empty())
}

fn safe_checkout_segment(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

pub struct LocalRepairExecutor {
    data_root: PathBuf,
    github_executable: String,
}

impl LocalRepairExecutor {
    pub fn new(data_root: impl Into<PathBuf>, github_executable: impl Into<String>) -> Self {
        Self {
            data_root: data_root.into(),
            github_executable: github_executable.into(),
        }
    }

    fn checkout(&self, request: &RepairRequest<'_>) -> Result<PathBuf> {
        if request.pull_request.cross_repository
            || request.pull_request.head_repository != request.repository
        {
            bail!("automatic repair cannot publish to a cross-repository pull request");
        }
        let (owner, repository) = request
            .repository
            .split_once('/')
            .context("remote repository must use OWNER/REPOSITORY format")?;
        if !safe_checkout_segment(owner) || !safe_checkout_segment(repository) {
            bail!("remote repository contains an unsafe path segment");
        }
        let directory = self
            .data_root
            .join("remote-checkouts")
            .join(owner)
            .join(repository);
        if directory.exists() {
            git_ok(&directory, &["fetch", "--prune", "origin"])?;
        } else {
            let parent = directory
                .parent()
                .context("remote checkout has no parent")?;
            fs::create_dir_all(parent)?;
            command_ok(
                &self.github_executable,
                &["repo", "clone", request.repository],
                Some(&directory),
            )?;
        }
        let remote_ref = format!("refs/heads/{}", request.pull_request.head_ref);
        let tracking_ref = format!("refs/remotes/agent-loop/pr-{}", request.pull_request.number);
        git_ok(
            &directory,
            &[
                "fetch",
                "--force",
                "origin",
                &format!("{remote_ref}:{tracking_ref}"),
            ],
        )?;
        let observed = git(&directory, &["rev-parse", &tracking_ref])?;
        if observed != request.pull_request.head_sha {
            bail!(
                "pull request head changed before repair: expected {}, got {observed}",
                request.pull_request.head_sha
            );
        }
        let internal_ref = format!("refs/heads/agent-loop/pr-{}", request.pull_request.number);
        git_ok(&directory, &["update-ref", &internal_ref, &observed])?;
        let source_config = request
            .project
            .repository_root
            .join(".agent-loop/config.toml");
        let target_config = directory.join(".agent-loop/config.toml");
        fs::create_dir_all(
            target_config
                .parent()
                .context("config path has no parent")?,
        )?;
        fs::copy(&source_config, &target_config).with_context(|| {
            format!(
                "copy remote automation configuration from {} to {}",
                source_config.display(),
                target_config.display()
            )
        })?;
        Ok(directory)
    }

    fn publish(
        &self,
        checkout: &Path,
        pull_request: &PullRequestSnapshot,
        candidate_sha: &str,
    ) -> Result<()> {
        let remote_ref = format!("refs/heads/{}", pull_request.head_ref);
        git_ok(
            checkout,
            &[
                "push",
                "origin",
                &format!("{candidate_sha}:{remote_ref}"),
                &format!("--force-with-lease={remote_ref}:{}", pull_request.head_sha),
            ],
        )?;
        let observed = git(checkout, &["ls-remote", "origin", &remote_ref])?;
        let published = observed.split_whitespace().next().unwrap_or_default();
        if published != candidate_sha {
            bail!("published pull request head does not match the checked candidate");
        }
        Ok(())
    }
}

impl RepairExecutor for LocalRepairExecutor {
    fn repair(&mut self, request: RepairRequest<'_>) -> Result<RepairResult> {
        if request.pull_request.cross_repository
            || request.pull_request.head_repository != request.repository
        {
            return Ok(RepairResult::Deferred {
                reason: "cross-repository pull requests require a human-owned publication path"
                    .into(),
            });
        }
        let mut service = ExecutionService::load(&self.data_root)?;
        if service.snapshot().runs.iter().any(|run| {
            matches!(
                run.status,
                LocalRunStatus::Preparing
                    | LocalRunStatus::Running
                    | LocalRunStatus::Evaluating
                    | LocalRunStatus::AwaitingDecision
                    | LocalRunStatus::Integrating
            )
        }) {
            return Ok(RepairResult::Deferred {
                reason: "another local or remote agent run is active".into(),
            });
        }
        let checkout = self.checkout(&request)?;
        let internal_branch = format!("agent-loop/pr-{}", request.pull_request.number);
        let reason = match request.reason {
            RepairReason::FailedChecks => "Fix the failed remote pipeline",
            RepairReason::MergeConflict => "Resolve the pull request's merge conflict",
        };
        let prompt = format!(
            "{reason} for pull request #{} (`{}`) at exact head {} against base `{}`. Preserve the pull request's intended behavior. Run the repository-owned checks, leave a clean committed candidate, and do not push or merge.\n\nRemote failure evidence:\n{}",
            request.pull_request.number,
            request.pull_request.title,
            request.pull_request.head_sha,
            request.pull_request.base_ref,
            if request.failure_details.is_empty() {
                "(no failed-check logs; inspect and resolve the merge conflict)"
            } else {
                request.failure_details
            }
        );
        let work_item = service.create_work_item(CreateWorkItem {
            project: RegisteredProject {
                id: format!(
                    "{}-remote-pr-{}",
                    request.config.project.id, request.pull_request.number
                ),
                repository_root: checkout.clone(),
            },
            title: format!(
                "Repair PR #{}: {}",
                request.pull_request.number, request.pull_request.title
            ),
            prompt: prompt.clone(),
            declared_scope: vec![".".into()],
            baseline_ref: request.pull_request.head_sha.clone(),
            target_branch: internal_branch,
        })?;
        control::record_work_item_intent(
            &self.data_root,
            work_item.id,
            prompt,
            Vec::new(),
            Vec::new(),
        )?;
        let provider = request
            .config
            .remote
            .provider
            .unwrap_or(request.config.agent.provider);
        let run = service.run_work_item(&work_item.id, provider, None, |_| {})?;
        if run.status != LocalRunStatus::AwaitingDecision {
            return Ok(RepairResult::Failed {
                run_id: Some(run.id),
                reason: run
                    .error
                    .unwrap_or_else(|| format!("repair run ended as {:?}", run.status)),
            });
        }
        let candidate_sha = run
            .contract
            .candidates
            .last()
            .and_then(|candidate| candidate.git_sha.clone())
            .context("repair run produced no Git candidate")?;
        let decided = service.decide(
            run.id,
            DecisionRequest::Approve {
                actor: "remote-pull-request-loop".into(),
                reason: Some("validated automated repair candidate".into()),
            },
        )?;
        if decided.status != LocalRunStatus::Completed {
            return Ok(RepairResult::Failed {
                run_id: Some(run.id),
                reason: decided
                    .error
                    .unwrap_or_else(|| "repair candidate integration failed".into()),
            });
        }
        let publication = match self.publish(&checkout, request.pull_request, &candidate_sha) {
            Ok(()) => Publication {
                publication_id: Uuid::new_v4().to_string(),
                kind: PublicationKind::PullRequest,
                candidate_identity: candidate_sha.clone(),
                status: PublicationStatus::Succeeded,
                external_id: Some(request.pull_request.url.clone()),
                occurred_at: Utc::now(),
                evidence: Vec::new(),
            },
            Err(error) => {
                service.record_publication(
                    run.id,
                    Publication {
                        publication_id: Uuid::new_v4().to_string(),
                        kind: PublicationKind::PullRequest,
                        candidate_identity: candidate_sha,
                        status: PublicationStatus::Failed,
                        external_id: Some(request.pull_request.url.clone()),
                        occurred_at: Utc::now(),
                        evidence: Vec::new(),
                    },
                )?;
                return Ok(RepairResult::Failed {
                    run_id: Some(run.id),
                    reason: format!("publish repair candidate: {error}"),
                });
            }
        };
        service.record_publication(run.id, publication)?;
        Ok(RepairResult::Published {
            run_id: run.id,
            head_sha: candidate_sha,
        })
    }
}

fn command_ok(executable: &str, arguments: &[&str], destination: Option<&Path>) -> Result<()> {
    let mut command = Command::new(executable);
    command.args(arguments);
    if let Some(destination) = destination {
        command.arg(destination);
    }
    let output = command
        .output()
        .with_context(|| format!("run {executable} {}", arguments.join(" ")))?;
    if !output.status.success() {
        bail!(
            "{executable} {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
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
    Ok(String::from_utf8(output.stdout)?.trim().into())
}

fn git_ok(repository: &Path, arguments: &[&str]) -> Result<()> {
    let _ = git(repository, arguments)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use tempfile::TempDir;

    use super::*;
    use crate::adapters::Provider;

    struct FakeHost {
        pull_request: RefCell<PullRequestSnapshot>,
        merges: RefCell<Vec<(u64, String)>>,
    }

    impl RemoteHost for FakeHost {
        fn open_pull_requests(&self, _repository: &str) -> Result<Vec<u64>> {
            Ok(vec![self.pull_request.borrow().number])
        }

        fn pull_request(&self, _repository: &str, _number: u64) -> Result<PullRequestSnapshot> {
            Ok(self.pull_request.borrow().clone())
        }

        fn failure_details(
            &self,
            _repository: &str,
            _pull_request: &PullRequestSnapshot,
        ) -> Result<String> {
            Ok("tests failed".into())
        }

        fn merge(
            &self,
            _repository: &str,
            number: u64,
            expected_head_sha: &str,
            _method: MergeMethod,
        ) -> Result<()> {
            self.merges
                .borrow_mut()
                .push((number, expected_head_sha.into()));
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeRepairs {
        requests: Vec<(u64, RepairReason)>,
        result: Option<RepairResult>,
    }

    impl RepairExecutor for FakeRepairs {
        fn repair(&mut self, request: RepairRequest<'_>) -> Result<RepairResult> {
            self.requests
                .push((request.pull_request.number, request.reason));
            Ok(self.result.clone().unwrap_or(RepairResult::Deferred {
                reason: "busy".into(),
            }))
        }
    }

    #[test]
    fn green_clean_pull_request_merges_only_the_observed_head() {
        let fixture = Fixture::new(CheckState::Passed, Mergeability::Mergeable);
        let host = FakeHost {
            pull_request: RefCell::new(fixture.pull_request.clone()),
            merges: RefCell::new(Vec::new()),
        };
        let mut repairs = FakeRepairs::default();
        let mut loop_ = RemotePullRequestLoop::new(fixture.data.path(), &host, &mut repairs);

        let outcomes = loop_
            .reconcile(&fixture.project, &fixture.config, false)
            .unwrap();

        assert!(matches!(outcomes[0], ReconcileOutcome::Merged { .. }));
        assert_eq!(host.merges.borrow().as_slice(), &[(42, "head-sha".into())]);
        assert!(repairs.requests.is_empty());
    }

    #[test]
    fn failed_checks_create_one_bounded_repair_and_wait_for_the_new_pipeline() {
        let mut fixture = Fixture::new(CheckState::Failed, Mergeability::Mergeable);
        fixture.config.remote.repair_failures = true;
        let host = FakeHost {
            pull_request: RefCell::new(fixture.pull_request.clone()),
            merges: RefCell::new(Vec::new()),
        };
        let mut repairs = FakeRepairs {
            result: Some(RepairResult::Published {
                run_id: Uuid::nil(),
                head_sha: "repaired-sha".into(),
            }),
            ..FakeRepairs::default()
        };

        {
            let mut loop_ = RemotePullRequestLoop::new(fixture.data.path(), &host, &mut repairs);
            let outcomes = loop_
                .reconcile(&fixture.project, &fixture.config, false)
                .unwrap();
            assert!(matches!(outcomes[0], ReconcileOutcome::Repaired { .. }));
        }
        let mut updated = fixture.pull_request.clone();
        updated.head_sha = "repaired-sha".into();
        updated.checks = CheckState::Pending;
        *host.pull_request.borrow_mut() = updated;
        let mut loop_ = RemotePullRequestLoop::new(fixture.data.path(), &host, &mut repairs);
        let outcomes = loop_
            .reconcile(&fixture.project, &fixture.config, false)
            .unwrap();

        assert!(matches!(outcomes[0], ReconcileOutcome::Waiting { .. }));
        assert_eq!(repairs.requests, vec![(42, RepairReason::FailedChecks)]);
        assert!(host.merges.borrow().is_empty());
    }

    #[test]
    fn untrusted_authors_never_merge_or_receive_an_agent() {
        let mut fixture = Fixture::new(CheckState::Passed, Mergeability::Mergeable);
        fixture.pull_request.author = "outsider".into();
        let host = FakeHost {
            pull_request: RefCell::new(fixture.pull_request.clone()),
            merges: RefCell::new(Vec::new()),
        };
        let mut repairs = FakeRepairs::default();
        let mut loop_ = RemotePullRequestLoop::new(fixture.data.path(), &host, &mut repairs);

        let outcomes = loop_
            .reconcile(&fixture.project, &fixture.config, false)
            .unwrap();

        assert!(matches!(outcomes[0], ReconcileOutcome::NeedsHuman { .. }));
        assert!(host.merges.borrow().is_empty());
        assert!(repairs.requests.is_empty());
    }

    #[test]
    fn failed_repair_is_not_repeated_past_the_configured_limit() {
        let mut fixture = Fixture::new(CheckState::Failed, Mergeability::Mergeable);
        fixture.config.remote.repair_failures = true;
        fixture.config.remote.max_repair_attempts = 1;
        let host = FakeHost {
            pull_request: RefCell::new(fixture.pull_request.clone()),
            merges: RefCell::new(Vec::new()),
        };
        let mut repairs = FakeRepairs {
            result: Some(RepairResult::Failed {
                run_id: None,
                reason: "agent could not fix the checks".into(),
            }),
            ..FakeRepairs::default()
        };

        {
            let mut loop_ = RemotePullRequestLoop::new(fixture.data.path(), &host, &mut repairs);
            let first = loop_
                .reconcile(&fixture.project, &fixture.config, false)
                .unwrap();
            assert!(matches!(first[0], ReconcileOutcome::RepairFailed { .. }));
        }
        let mut loop_ = RemotePullRequestLoop::new(fixture.data.path(), &host, &mut repairs);
        let second = loop_
            .reconcile(&fixture.project, &fixture.config, false)
            .unwrap();

        assert!(matches!(second[0], ReconcileOutcome::NeedsHuman { .. }));
        assert_eq!(repairs.requests.len(), 1);
    }

    #[test]
    fn github_check_rollup_is_normalized_into_failed_check_evidence() {
        let row: GitHubPullRequest = serde_json::from_value(serde_json::json!({
            "number": 42,
            "title": "Change",
            "state": "OPEN",
            "isDraft": false,
            "headRefName": "feature",
            "headRefOid": "head-sha",
            "headRepository": { "name": "demo" },
            "headRepositoryOwner": { "login": "owner" },
            "isCrossRepository": false,
            "baseRefName": "main",
            "mergeable": "MERGEABLE",
            "mergeStateStatus": "UNSTABLE",
            "statusCheckRollup": [{
                "__typename": "CheckRun",
                "name": "CI",
                "status": "COMPLETED",
                "conclusion": "FAILURE",
                "detailsUrl": "https://github.com/owner/demo/actions/runs/123/job/456"
            }],
            "reviewDecision": "",
            "author": { "login": "trusted" },
            "url": "https://github.com/owner/demo/pull/42"
        }))
        .unwrap();

        let snapshot = PullRequestSnapshot::try_from(row).unwrap();

        assert_eq!(snapshot.checks, CheckState::Failed);
        assert_eq!(snapshot.failed_checks[0].name, "CI");
        assert_eq!(
            github_run_id(snapshot.failed_checks[0].details_url.as_deref().unwrap()),
            Some("123")
        );
    }

    struct Fixture {
        data: TempDir,
        project: RegisteredProject,
        config: ProjectConfig,
        pull_request: PullRequestSnapshot,
    }

    impl Fixture {
        fn new(checks: CheckState, mergeability: Mergeability) -> Self {
            let data = tempfile::tempdir().unwrap();
            let project = RegisteredProject {
                id: "demo".into(),
                repository_root: data.path().join("developer-checkout"),
            };
            let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
            config.remote.enabled = true;
            config.remote.repository = Some("owner/demo".into());
            config.remote.auto_merge = true;
            config.remote.trusted_authors = vec!["trusted".into()];
            let pull_request = PullRequestSnapshot {
                number: 42,
                title: "Change".into(),
                url: "https://github.example/owner/demo/pull/42".into(),
                author: "trusted".into(),
                state: "OPEN".into(),
                draft: false,
                head_ref: "feature".into(),
                head_sha: "head-sha".into(),
                head_repository: "owner/demo".into(),
                base_ref: "main".into(),
                cross_repository: false,
                checks,
                failed_checks: vec![FailedCheck {
                    name: "CI".into(),
                    conclusion: "FAILURE".into(),
                    details_url: None,
                }],
                mergeability,
                merge_state: "CLEAN".into(),
                review_decision: None,
            };
            Self {
                data,
                project,
                config,
                pull_request,
            }
        }
    }
}
