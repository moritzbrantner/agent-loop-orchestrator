use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    config::{MergeMethod, ProjectConfig, PublicationMode},
    contracts::{Publication, PublicationKind, PublicationStatus},
    control,
    execution::{
        CreateWorkItem, DecisionRequest, ExecutionOverrides, ExecutionService, LocalRunStatus,
    },
    publication::{
        NewPullRequest, PullRequestPublication, PullRequestPublisher, PullRequestRepair,
    },
    repository::RegisteredProject,
};

const READY_LABEL: &str = "ready-for-agent";
const ACTIVE_LABEL: &str = "agent-loop:active";
const BLOCKED_LABEL: &str = "agent-loop:blocked";
const READY_TO_MERGE_LABEL: &str = "agent-loop:ready-to-merge";
const MAX_REPAIR_EVIDENCE_CHARS: usize = 8_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueuePullRequest {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub draft: bool,
    pub head_ref: String,
    pub head_sha: String,
    pub base_ref: String,
    pub same_repository: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueIssue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub url: String,
    pub labels: BTreeSet<String>,
    pub scope: Vec<String>,
    pub blocked_by: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum IntegrationResult {
    Merged,
    Repairable { reason: String },
    Blocked { reason: String },
    Refresh { reason: String },
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum QueueWorkResult {
    Published {
        run_id: Uuid,
        publication: PullRequestPublication,
    },
    Repaired {
        run_id: Uuid,
        head_sha: String,
    },
    Stale {
        run_id: Option<Uuid>,
        reason: String,
    },
    Failed {
        run_id: Option<Uuid>,
        reason: String,
    },
}

pub trait QueuePlatform {
    fn refresh(&mut self) -> Result<()>;
    fn open_pull_requests(&mut self) -> Result<Vec<QueuePullRequest>>;
    fn integrate(&mut self, pull_request: &QueuePullRequest) -> Result<IntegrationResult>;
    fn issues(&mut self) -> Result<Vec<QueueIssue>>;
    fn issue_states(&mut self, numbers: &[u64]) -> Result<BTreeMap<u64, String>>;
    fn mark_issue_active(&mut self, issue: &QueueIssue) -> Result<()>;
    fn mark_issue_ready(&mut self, issue: &QueueIssue, pull_request: u64) -> Result<()>;
    fn mark_issue_blocked(&mut self, issue: &QueueIssue, reason: &str) -> Result<()>;
}

pub trait QueueWorker {
    fn implement(&mut self, issue: &QueueIssue) -> Result<QueueWorkResult>;
    fn repair(&mut self, pull_request: &QueuePullRequest, reason: &str) -> Result<QueueWorkResult>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "event")]
pub enum QueueEvent {
    PullRequestMerged {
        number: u64,
        head_sha: String,
    },
    PullRequestRepaired {
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
    RepairSkippedNoInformationGain {
        number: u64,
        head_sha: String,
        evidence_signature: String,
    },
    IssuePublished {
        issue: u64,
        pull_request: u64,
        url: String,
        head_sha: String,
        run_id: Uuid,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueStopReason {
    NoWork,
    Blocked,
    HardBlocker,
    ItemLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueBlocker {
    pub kind: String,
    pub number: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueUsage {
    pub integration_evaluations: u32,
    pub provider_attempts: u32,
    pub issue_attempts: u32,
    pub repair_attempts: u32,
    pub repair_limit_stops: u32,
    pub no_information_gain_stops: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueReport {
    pub stop_reason: QueueStopReason,
    pub items_processed: u32,
    pub events: Vec<QueueEvent>,
    pub blockers: Vec<QueueBlocker>,
    #[serde(default)]
    pub usage: QueueUsage,
}

pub struct QueueRunner<'a> {
    data_root: PathBuf,
    repository: String,
    config: &'a ProjectConfig,
    platform: &'a mut dyn QueuePlatform,
    worker: &'a mut dyn QueueWorker,
}

impl<'a> QueueRunner<'a> {
    pub fn new(
        data_root: impl Into<PathBuf>,
        repository: impl Into<String>,
        config: &'a ProjectConfig,
        platform: &'a mut dyn QueuePlatform,
        worker: &'a mut dyn QueueWorker,
    ) -> Self {
        Self {
            data_root: data_root.into(),
            repository: repository.into(),
            config,
            platform,
            worker,
        }
    }

    pub fn run(&mut self) -> Result<QueueReport> {
        if self.config.publication.mode != PublicationMode::PullRequest {
            bail!("queue run requires publication.mode = \"pull-request\"");
        }
        let store = QueueStateStore::new(&self.data_root)?;
        let lock = store.lock_exclusive()?;
        let mut state = store.load()?;
        let mut events = Vec::new();
        let mut blockers = Vec::new();
        let mut items_processed = 0;
        let mut usage = QueueUsage::default();

        loop {
            if items_processed >= self.config.queue.max_items_per_run {
                return finish_report(
                    &store,
                    lock,
                    &state,
                    QueueReport {
                        stop_reason: QueueStopReason::ItemLimit,
                        items_processed,
                        events,
                        blockers,
                        usage,
                    },
                );
            }
            blockers.clear();
            self.platform.refresh()?;
            let mut progressed = false;
            let mut pull_requests = self.platform.open_pull_requests()?;
            pull_requests.sort_by_key(|pull_request| pull_request.number);
            for pull_request in pull_requests {
                if pull_request.draft {
                    blockers.push(pr_blocker(&pull_request, "pull request is a draft"));
                    continue;
                }
                if !pull_request.same_repository {
                    blockers.push(pr_blocker(
                        &pull_request,
                        "cross-repository pull request cannot be repaired by this queue",
                    ));
                    continue;
                }
                let key = format!("{}#{}", self.repository, pull_request.number);
                state
                    .pull_requests
                    .entry(key.clone())
                    .or_default()
                    .observe(&pull_request);
                let queue_owned = state
                    .pull_requests
                    .get(&key)
                    .is_some_and(|record| record.queue_owned);
                let trusted_author = self
                    .config
                    .queue
                    .trusted_authors
                    .iter()
                    .any(|author| author.eq_ignore_ascii_case(&pull_request.author));
                if !queue_owned && !trusted_author {
                    blockers.push(pr_blocker(
                        &pull_request,
                        format!(
                            "author {} is not listed in queue.trusted_authors",
                            pull_request.author
                        ),
                    ));
                    continue;
                }
                usage.integration_evaluations += 1;
                match self.platform.integrate(&pull_request)? {
                    IntegrationResult::Merged => {
                        state.pull_requests.remove(&key);
                        store.write(&state)?;
                        events.push(QueueEvent::PullRequestMerged {
                            number: pull_request.number,
                            head_sha: pull_request.head_sha,
                        });
                        items_processed += 1;
                        progressed = true;
                        break;
                    }
                    IntegrationResult::Refresh { .. } => {
                        progressed = true;
                        break;
                    }
                    IntegrationResult::Blocked { reason } => {
                        blockers.push(pr_blocker(&pull_request, reason));
                        continue;
                    }
                    IntegrationResult::Failed { reason } => {
                        blockers.push(pr_blocker(&pull_request, reason));
                        return finish_report(
                            &store,
                            lock,
                            &state,
                            QueueReport {
                                stop_reason: QueueStopReason::HardBlocker,
                                items_processed,
                                events,
                                blockers,
                                usage,
                            },
                        );
                    }
                    IntegrationResult::Repairable {
                        reason: integration_reason,
                    } => {
                        let attempts = state
                            .pull_requests
                            .get(&key)
                            .context("queue pull request record disappeared")?
                            .repair_attempts;
                        if attempts >= self.config.queue.max_repair_attempts {
                            usage.repair_limit_stops += 1;
                            blockers.push(pr_blocker(
                                &pull_request,
                                format!(
                                    "repair limit of {} reached",
                                    self.config.queue.max_repair_attempts
                                ),
                            ));
                            continue;
                        }

                        let integration_signature = evidence_signature(&integration_reason);
                        let previous_failure = state
                            .pull_requests
                            .get(&key)
                            .context("queue pull request record disappeared")?
                            .last_failed_repair
                            .clone();
                        if previous_failure.as_ref().is_some_and(|previous| {
                            previous.head_sha == pull_request.head_sha
                                && previous.integration_signature == integration_signature
                        }) {
                            usage.no_information_gain_stops += 1;
                            events.push(QueueEvent::RepairSkippedNoInformationGain {
                                number: pull_request.number,
                                head_sha: pull_request.head_sha.clone(),
                                evidence_signature: integration_signature,
                            });
                            blockers.push(pr_blocker(
                                &pull_request,
                                "repair stopped because the same PR head produced materially unchanged integration evidence after the previous failed provider attempt",
                            ));
                            continue;
                        }

                        state
                            .pull_requests
                            .get_mut(&key)
                            .context("queue pull request record disappeared")?
                            .repair_attempts += 1;
                        store.write(&state)?;
                        let repair_context =
                            build_repair_context(&integration_reason, previous_failure.as_ref());
                        usage.provider_attempts += 1;
                        usage.repair_attempts += 1;
                        let repair = self.worker.repair(&pull_request, &repair_context)?;
                        items_processed += 1;
                        match repair {
                            QueueWorkResult::Repaired { run_id, head_sha } => {
                                let record = state
                                    .pull_requests
                                    .get_mut(&key)
                                    .context("queue pull request record disappeared")?;
                                record.last_published_sha = Some(head_sha.clone());
                                record.last_failed_repair = None;
                                store.write(&state)?;
                                events.push(QueueEvent::PullRequestRepaired {
                                    number: pull_request.number,
                                    previous_head_sha: pull_request.head_sha,
                                    head_sha,
                                    run_id,
                                });
                                progressed = true;
                                break;
                            }
                            QueueWorkResult::Failed { run_id, reason } => {
                                events.push(QueueEvent::RepairFailed {
                                    number: pull_request.number,
                                    head_sha: pull_request.head_sha.clone(),
                                    run_id,
                                    reason: reason.clone(),
                                });
                                let attempts = {
                                    let record = state
                                        .pull_requests
                                        .get_mut(&key)
                                        .context("queue pull request record disappeared")?;
                                    record.last_failed_repair = Some(FailedRepairEvidence {
                                        head_sha: pull_request.head_sha.clone(),
                                        integration_signature,
                                        integration_reason: bounded_evidence(&integration_reason),
                                        run_id,
                                        failure_reason: bounded_evidence(&reason),
                                    });
                                    record.repair_attempts
                                };
                                store.write(&state)?;
                                if attempts >= self.config.queue.max_repair_attempts {
                                    usage.repair_limit_stops += 1;
                                    blockers.push(pr_blocker(&pull_request, reason));
                                } else {
                                    progressed = true;
                                }
                                break;
                            }
                            QueueWorkResult::Stale { .. } => {
                                progressed = true;
                                break;
                            }
                            QueueWorkResult::Published { .. } => {
                                bail!("repair worker returned a new pull request publication")
                            }
                        }
                    }
                }
            }
            if progressed {
                continue;
            }

            let issues = self.platform.issues()?;
            let selection = select_issue(issues, self.platform)?;
            if let Some(issue) = selection.ready {
                self.platform.mark_issue_active(&issue)?;
                usage.provider_attempts += 1;
                usage.issue_attempts += 1;
                let result = self.worker.implement(&issue)?;
                items_processed += 1;
                match result {
                    QueueWorkResult::Published {
                        run_id,
                        publication,
                    } => {
                        let key = format!("{}#{}", self.repository, publication.number);
                        let record = state.pull_requests.entry(key).or_default();
                        record.queue_owned = true;
                        record.last_observed_sha = Some(publication.head_sha.clone());
                        record.last_published_sha = Some(publication.head_sha.clone());
                        record.updated_at = Some(Utc::now());
                        store.write(&state)?;
                        self.platform.mark_issue_ready(&issue, publication.number)?;
                        events.push(QueueEvent::IssuePublished {
                            issue: issue.number,
                            pull_request: publication.number,
                            url: publication.url,
                            head_sha: publication.head_sha,
                            run_id,
                        });
                        continue;
                    }
                    QueueWorkResult::Failed { reason, .. } => {
                        self.platform.mark_issue_blocked(&issue, &reason)?;
                        blockers.push(QueueBlocker {
                            kind: "issue".into(),
                            number: Some(issue.number),
                            reason,
                        });
                        return finish_report(
                            &store,
                            lock,
                            &state,
                            QueueReport {
                                stop_reason: QueueStopReason::HardBlocker,
                                items_processed,
                                events,
                                blockers,
                                usage,
                            },
                        );
                    }
                    QueueWorkResult::Repaired { .. } => {
                        bail!("issue worker returned a pull request repair")
                    }
                    QueueWorkResult::Stale { .. } => {
                        bail!("issue worker returned stale pull request state")
                    }
                }
            }
            blockers.extend(selection.blockers);
            let stop_reason = if blockers.is_empty() {
                QueueStopReason::NoWork
            } else {
                QueueStopReason::Blocked
            };
            return finish_report(
                &store,
                lock,
                &state,
                QueueReport {
                    stop_reason,
                    items_processed,
                    events,
                    blockers,
                    usage,
                },
            );
        }
    }
}

fn finish_report(
    store: &QueueStateStore,
    lock: File,
    state: &QueueState,
    report: QueueReport,
) -> Result<QueueReport> {
    store.write(state)?;
    FileExt::unlock(&lock)?;
    Ok(report)
}

fn pr_blocker(pull_request: &QueuePullRequest, reason: impl Into<String>) -> QueueBlocker {
    QueueBlocker {
        kind: "pull_request".into(),
        number: Some(pull_request.number),
        reason: reason.into(),
    }
}

fn build_repair_context(
    integration_reason: &str,
    previous_failure: Option<&FailedRepairEvidence>,
) -> String {
    let Some(previous) = previous_failure else {
        return integration_reason.to_owned();
    };
    let previous_run = previous
        .run_id
        .map(|run_id| run_id.to_string())
        .unwrap_or_else(|| "unavailable".into());
    format!(
        "{integration_reason}\n\nPrevious failed repair attempt on the same PR head:\n- head: {}\n- run: {previous_run}\n- previous integration evidence:\n{}\n- previous worker result:\n{}\n\nUse this as prior evidence; do not repeat investigation that it already settles.",
        previous.head_sha, previous.integration_reason, previous.failure_reason
    )
}

fn bounded_evidence(value: &str) -> String {
    let mut chars = value.chars();
    let mut bounded = chars
        .by_ref()
        .take(MAX_REPAIR_EVIDENCE_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        bounded.push_str("\n...[truncated]");
    }
    bounded
}

fn evidence_signature(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

struct IssueSelection {
    ready: Option<QueueIssue>,
    blockers: Vec<QueueBlocker>,
}

fn select_issue(
    mut issues: Vec<QueueIssue>,
    platform: &mut dyn QueuePlatform,
) -> Result<IssueSelection> {
    issues.sort_by_key(|issue| issue.number);
    let blocker_numbers = issues
        .iter()
        .flat_map(|issue| issue.blocked_by.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let states = platform.issue_states(&blocker_numbers)?;
    let mut blockers = Vec::new();
    for issue in issues {
        if issue.labels.contains("prd") {
            blockers.push(QueueBlocker {
                kind: "issue".into(),
                number: Some(issue.number),
                reason: "ready PRD requires decomposition before implementation".into(),
            });
            continue;
        }
        if !issue.labels.contains(READY_LABEL)
            || issue.labels.contains(ACTIVE_LABEL)
            || issue.labels.contains(BLOCKED_LABEL)
            || issue.labels.contains(READY_TO_MERGE_LABEL)
            || issue.labels.contains("ready-for-human")
        {
            if issue.labels.contains(BLOCKED_LABEL) || issue.labels.contains("ready-for-human") {
                blockers.push(QueueBlocker {
                    kind: "issue".into(),
                    number: Some(issue.number),
                    reason: "issue requires human input".into(),
                });
            }
            continue;
        }
        if issue.scope.is_empty() {
            blockers.push(QueueBlocker {
                kind: "issue".into(),
                number: Some(issue.number),
                reason: "agent-ready issue has no declared scope".into(),
            });
            continue;
        }
        let open_blockers = issue
            .blocked_by
            .iter()
            .copied()
            .filter(|number| states.get(number).is_none_or(|state| state != "CLOSED"))
            .collect::<Vec<_>>();
        if !open_blockers.is_empty() {
            blockers.push(QueueBlocker {
                kind: "issue".into(),
                number: Some(issue.number),
                reason: format!("blocked by open issues {open_blockers:?}"),
            });
            continue;
        }
        return Ok(IssueSelection {
            ready: Some(issue),
            blockers,
        });
    }
    Ok(IssueSelection {
        ready: None,
        blockers,
    })
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueState {
    #[serde(default)]
    pull_requests: BTreeMap<String, PullRequestRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FailedRepairEvidence {
    head_sha: String,
    integration_signature: String,
    integration_reason: String,
    run_id: Option<Uuid>,
    failure_reason: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestRecord {
    last_observed_sha: Option<String>,
    last_published_sha: Option<String>,
    #[serde(default)]
    repair_attempts: u32,
    #[serde(default)]
    queue_owned: bool,
    #[serde(default)]
    last_failed_repair: Option<FailedRepairEvidence>,
    updated_at: Option<DateTime<Utc>>,
}

impl PullRequestRecord {
    fn observe(&mut self, pull_request: &QueuePullRequest) {
        if self.last_observed_sha.as_deref() != Some(&pull_request.head_sha) {
            if self.last_published_sha.as_deref() != Some(&pull_request.head_sha) {
                self.repair_attempts = 0;
            }
            self.last_failed_repair = None;
            self.last_observed_sha = Some(pull_request.head_sha.clone());
            self.updated_at = Some(Utc::now());
        }
    }
}

struct QueueStateStore {
    state_path: PathBuf,
    lock_path: PathBuf,
}

impl QueueStateStore {
    fn new(data_root: &Path) -> Result<Self> {
        fs::create_dir_all(data_root)?;
        Ok(Self {
            state_path: data_root.join("queue-state.json"),
            lock_path: data_root.join("queue-state.lock"),
        })
    }

    fn lock_exclusive(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.lock_path)?;
        FileExt::lock_exclusive(&file)?;
        Ok(file)
    }

    fn load(&self) -> Result<QueueState> {
        if !self.state_path.exists() {
            return Ok(QueueState::default());
        }
        serde_json::from_slice(&fs::read(&self.state_path)?)
            .with_context(|| format!("parse {}", self.state_path.display()))
    }

    fn write(&self, state: &QueueState) -> Result<()> {
        let temporary = self.state_path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(state)?)?;
        fs::rename(&temporary, &self.state_path)?;
        Ok(())
    }
}

pub struct GitHubQueuePlatform {
    source_root: PathBuf,
    checkout: PathBuf,
    repository: String,
    remote: String,
    github_executable: String,
    coding_tooling_executable: String,
    merge_method: MergeMethod,
}

impl GitHubQueuePlatform {
    pub fn prepare(
        _data_root: &Path,
        project: &RegisteredProject,
        config: &ProjectConfig,
    ) -> Result<Self> {
        let publisher = PullRequestPublisher::new(config.publication.github_executable.clone());
        let repository = publisher.repository_slug(&project.repository_root)?;
        let checkout = project.repository_root.clone();
        Ok(Self {
            source_root: project.repository_root.clone(),
            checkout,
            repository,
            remote: config.publication.remote.clone(),
            github_executable: config.publication.github_executable.clone(),
            coding_tooling_executable: config.execution.coding_tooling_executable.clone(),
            merge_method: config.queue.merge_method,
        })
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }

    pub fn checkout(&self) -> &Path {
        &self.checkout
    }

    fn github(&self, arguments: &[&str]) -> Result<Vec<u8>> {
        command_output(&self.github_executable, arguments, &self.source_root)
    }
}

impl QueuePlatform for GitHubQueuePlatform {
    fn refresh(&mut self) -> Result<()> {
        git_ok(&self.checkout, &["fetch", "--prune", &self.remote])?;
        let status = git(&self.checkout, &["status", "--porcelain"])?;
        if !status.is_empty() {
            bail!("registered repository checkout is not clean");
        }
        Ok(())
    }

    fn open_pull_requests(&mut self) -> Result<Vec<QueuePullRequest>> {
        let output = self.github(&[
            "pr",
            "list",
            "--repo",
            &self.repository,
            "--state",
            "open",
            "--limit",
            "100",
            "--json",
            "number,title,url,author,isDraft,headRefName,headRefOid,baseRefName,isCrossRepository",
        ])?;
        let rows: Vec<GitHubPullRequest> =
            serde_json::from_slice(&output).context("parse GitHub pull request queue")?;
        Ok(rows
            .into_iter()
            .map(|row| QueuePullRequest {
                number: row.number,
                title: row.title,
                url: row.url,
                author: row.author.login,
                draft: row.is_draft,
                head_ref: row.head_ref_name,
                head_sha: row.head_ref_oid,
                base_ref: row.base_ref_name,
                same_repository: !row.is_cross_repository,
            })
            .collect())
    }

    fn integrate(&mut self, pull_request: &QueuePullRequest) -> Result<IntegrationResult> {
        let number = pull_request.number.to_string();
        let output = Command::new(&self.coding_tooling_executable)
            .args([
                "pr",
                "integrate",
                &number,
                "--tier",
                "full",
                "--merge-method",
                merge_method(self.merge_method),
                "--remote",
                &self.remote,
                "--json",
            ])
            .current_dir(&self.checkout)
            .output()
            .with_context(|| {
                format!(
                    "run {} pr integrate {}",
                    self.coding_tooling_executable, pull_request.number
                )
            })?;
        let envelope: ToolingEnvelope = serde_json::from_slice(&output.stdout)
            .context("parse coding-tooling pr integrate result")?;
        Ok(classify_integration(&envelope))
    }

    fn issues(&mut self) -> Result<Vec<QueueIssue>> {
        let output = self.github(&[
            "issue",
            "list",
            "--repo",
            &self.repository,
            "--state",
            "open",
            "--limit",
            "100",
            "--label",
            READY_LABEL,
            "--json",
            "number,title,body,url,labels",
        ])?;
        let rows: Vec<GitHubIssue> =
            serde_json::from_slice(&output).context("parse GitHub issue queue")?;
        rows.into_iter().map(QueueIssue::try_from).collect()
    }

    fn issue_states(&mut self, numbers: &[u64]) -> Result<BTreeMap<u64, String>> {
        let mut states = BTreeMap::new();
        for number in numbers {
            let text = number.to_string();
            let output = self.github(&[
                "issue",
                "view",
                &text,
                "--repo",
                &self.repository,
                "--json",
                "number,state",
            ])?;
            let state: GitHubIssueState =
                serde_json::from_slice(&output).context("parse blocker issue state")?;
            states.insert(state.number, state.state);
        }
        Ok(states)
    }

    fn mark_issue_active(&mut self, issue: &QueueIssue) -> Result<()> {
        let number = issue.number.to_string();
        self.github(&[
            "issue",
            "edit",
            &number,
            "--repo",
            &self.repository,
            "--add-label",
            ACTIVE_LABEL,
        ])?;
        Ok(())
    }

    fn mark_issue_ready(&mut self, issue: &QueueIssue, pull_request: u64) -> Result<()> {
        let number = issue.number.to_string();
        self.github(&[
            "issue",
            "edit",
            &number,
            "--repo",
            &self.repository,
            "--remove-label",
            ACTIVE_LABEL,
            "--add-label",
            READY_TO_MERGE_LABEL,
        ])?;
        let body = format!("Agent Loop published pull request #{pull_request}.");
        self.github(&[
            "issue",
            "comment",
            &number,
            "--repo",
            &self.repository,
            "--body",
            &body,
        ])?;
        Ok(())
    }

    fn mark_issue_blocked(&mut self, issue: &QueueIssue, reason: &str) -> Result<()> {
        let number = issue.number.to_string();
        self.github(&[
            "issue",
            "edit",
            &number,
            "--repo",
            &self.repository,
            "--remove-label",
            ACTIVE_LABEL,
            "--add-label",
            BLOCKED_LABEL,
        ])?;
        let body = format!("## Agent Loop Blocker\n\n{reason}");
        self.github(&[
            "issue",
            "comment",
            &number,
            "--repo",
            &self.repository,
            "--body",
            &body,
        ])?;
        Ok(())
    }
}

pub struct LocalQueueWorker {
    data_root: PathBuf,
    project: RegisteredProject,
    config: ProjectConfig,
    repository: String,
    checkout: PathBuf,
    publisher: PullRequestPublisher,
}

impl LocalQueueWorker {
    pub fn new(
        data_root: impl Into<PathBuf>,
        project: RegisteredProject,
        config: ProjectConfig,
        repository: impl Into<String>,
        checkout: impl Into<PathBuf>,
    ) -> Self {
        let publisher = PullRequestPublisher::new(config.publication.github_executable.clone());
        Self {
            data_root: data_root.into(),
            project,
            config,
            repository: repository.into(),
            checkout: checkout.into(),
            publisher,
        }
    }

    fn run_candidate(
        &self,
        title: String,
        prompt: String,
        scope: Vec<String>,
        baseline: &str,
        target_branch: &str,
    ) -> Result<QueueWorkResult> {
        let target_ref = format!("refs/heads/{target_branch}");
        match local_ref(&self.checkout, &target_ref)? {
            Some(current) if current == baseline => {}
            Some(current) => {
                return Ok(QueueWorkResult::Failed {
                    run_id: None,
                    reason: format!(
                        "local publication branch {target_branch} moved from {baseline} to {current}"
                    ),
                });
            }
            None => git_ok(&self.checkout, &["update-ref", &target_ref, baseline])?,
        }
        let mut service = ExecutionService::load(&self.data_root)?;
        let item = service.create_work_item(CreateWorkItem {
            project: RegisteredProject {
                id: format!("{}-queue", self.project.id),
                repository_root: self.checkout.clone(),
            },
            title,
            prompt: prompt.clone(),
            declared_scope: scope,
            baseline_ref: baseline.into(),
            target_branch: target_branch.into(),
        })?;
        control::record_work_item_intent(&self.data_root, item.id, prompt, Vec::new(), Vec::new())?;
        let provider = self
            .config
            .queue
            .provider
            .unwrap_or(self.config.agent.provider);
        let run = service.run_work_item_with_config(
            &item.id,
            provider,
            &self.config,
            ExecutionOverrides {
                check_tier: Some("full".into()),
                ..ExecutionOverrides::default()
            },
            None,
            |_| {},
        )?;
        if run.status != LocalRunStatus::AwaitingDecision {
            return Ok(QueueWorkResult::Failed {
                run_id: Some(run.id),
                reason: run
                    .error
                    .unwrap_or_else(|| format!("queue work item ended as {:?}", run.status)),
            });
        }
        let candidate_sha = run
            .contract
            .candidates
            .last()
            .and_then(|candidate| candidate.git_sha.clone())
            .context("queue work item produced no Git candidate")?;
        let decided = service.decide(
            run.id,
            DecisionRequest::Approve {
                actor: "queue-runner".into(),
                reason: Some("checked queue candidate".into()),
            },
        )?;
        if decided.status != LocalRunStatus::Completed {
            return Ok(QueueWorkResult::Failed {
                run_id: Some(run.id),
                reason: decided.error.unwrap_or_else(|| {
                    "queue candidate could not be integrated into its publication branch".into()
                }),
            });
        }
        Ok(QueueWorkResult::Repaired {
            run_id: run.id,
            head_sha: candidate_sha,
        })
    }

    fn record_publication(
        &self,
        run_id: Uuid,
        candidate_sha: String,
        external_id: String,
        status: PublicationStatus,
    ) -> Result<()> {
        let mut service = ExecutionService::load(&self.data_root)?;
        service.record_publication(
            run_id,
            Publication {
                publication_id: Uuid::new_v4().to_string(),
                kind: PublicationKind::PullRequest,
                candidate_identity: candidate_sha,
                status,
                external_id: Some(external_id),
                occurred_at: Utc::now(),
                evidence: Vec::new(),
            },
        )?;
        Ok(())
    }
}

fn issue_worker_prompt(issue: &QueueIssue) -> String {
    format!(
        "Implement GitHub issue #{} ({}). Preserve out-of-scope behavior and satisfy the issue acceptance criteria. Use the narrowest relevant repository-owned checks while iterating. The orchestrator will independently run the configured full completion gate on your committed candidate; run broader checks yourself only when their result is needed to diagnose the task. Leave a clean committed candidate. Do not push, publish, or merge.\n\nIssue: {}\n\n{}",
        issue.number, issue.title, issue.url, issue.body
    )
}

fn repair_worker_prompt(pull_request: &QueuePullRequest, repair_context: &str) -> String {
    format!(
        "Repair pull request #{} ({}) at exact head {} against base {}. Preserve its intended behavior and fix the integration failure below. Use the narrowest relevant repository-owned checks while iterating. The orchestrator will independently run the configured full completion gate on your committed candidate; run broader checks yourself only when their result is needed to diagnose the failure. Leave a clean committed candidate. Do not push, publish, or merge.\n\nIntegration evidence:\n{}",
        pull_request.number,
        pull_request.title,
        pull_request.head_sha,
        pull_request.base_ref,
        repair_context
    )
}

impl QueueWorker for LocalQueueWorker {
    fn implement(&mut self, issue: &QueueIssue) -> Result<QueueWorkResult> {
        let baseline_ref = format!(
            "refs/remotes/{}/{}",
            self.config.publication.remote, self.config.execution.target_branch
        );
        let baseline = git(&self.checkout, &["rev-parse", &baseline_ref])?;
        let branch = format!("agent-loop/issue-{}", issue.number);
        if let Some(existing) = local_ref(&self.checkout, &format!("refs/heads/{branch}"))?
            && existing != baseline
        {
            return Ok(QueueWorkResult::Failed {
                run_id: None,
                reason: format!(
                    "publication branch {branch} already exists at {existing}; refusing to overwrite it"
                ),
            });
        }
        let prompt = issue_worker_prompt(issue);
        let result = self.run_candidate(
            format!("Issue #{}: {}", issue.number, issue.title),
            prompt,
            issue.scope.clone(),
            &baseline,
            &branch,
        )?;
        let QueueWorkResult::Repaired { run_id, head_sha } = result else {
            return Ok(result);
        };
        let body = format!(
            "Closes #{}\n\nCreated by Agent Loop from exact checked candidate {head_sha}.",
            issue.number
        );
        let publication = match self.publisher.publish_new(NewPullRequest {
            checkout: &self.checkout,
            repository: &self.repository,
            remote: &self.config.publication.remote,
            candidate_sha: &head_sha,
            branch: &branch,
            base: &self.config.execution.target_branch,
            title: &issue.title,
            body: &body,
        }) {
            Ok(publication) => publication,
            Err(error) => {
                self.record_publication(
                    run_id,
                    head_sha,
                    format!("issue:{}", issue.number),
                    PublicationStatus::Failed,
                )?;
                return Ok(QueueWorkResult::Failed {
                    run_id: Some(run_id),
                    reason: format!("publish checked candidate: {error:#}"),
                });
            }
        };
        self.record_publication(
            run_id,
            publication.head_sha.clone(),
            publication.url.clone(),
            PublicationStatus::Succeeded,
        )?;
        Ok(QueueWorkResult::Published {
            run_id,
            publication,
        })
    }

    fn repair(&mut self, pull_request: &QueuePullRequest, reason: &str) -> Result<QueueWorkResult> {
        if !pull_request.same_repository {
            return Ok(QueueWorkResult::Failed {
                run_id: None,
                reason: "cross-repository pull request cannot be repaired automatically".into(),
            });
        }
        let remote_ref = format!("refs/heads/{}", pull_request.head_ref);
        let tracking_ref = format!("refs/remotes/agent-loop/pr-{}", pull_request.number);
        git_ok(
            &self.checkout,
            &[
                "fetch",
                "--force",
                &self.config.publication.remote,
                &format!("{remote_ref}:{tracking_ref}"),
            ],
        )?;
        let observed = git(&self.checkout, &["rev-parse", &tracking_ref])?;
        if observed != pull_request.head_sha {
            return Ok(QueueWorkResult::Failed {
                run_id: None,
                reason: format!(
                    "pull request head moved before repair: expected {}, observed {observed}",
                    pull_request.head_sha
                ),
            });
        }
        let internal_branch = format!("agent-loop/pr-{}", pull_request.number);
        git_ok(
            &self.checkout,
            &[
                "update-ref",
                &format!("refs/heads/{internal_branch}"),
                &observed,
            ],
        )?;
        let prompt = repair_worker_prompt(pull_request, reason);
        let result = self.run_candidate(
            format!("Repair PR #{}: {}", pull_request.number, pull_request.title),
            prompt,
            vec![".".into()],
            &observed,
            &internal_branch,
        )?;
        let QueueWorkResult::Repaired { run_id, head_sha } = result else {
            return Ok(result);
        };
        if let Err(error) = self.publisher.publish_repair(PullRequestRepair {
            checkout: &self.checkout,
            remote: &self.config.publication.remote,
            branch: &pull_request.head_ref,
            expected_head_sha: &pull_request.head_sha,
            candidate_sha: &head_sha,
        }) {
            let reason = format!("publish repair candidate: {error:#}");
            self.record_publication(
                run_id,
                head_sha.clone(),
                pull_request.url.clone(),
                PublicationStatus::Failed,
            )?;
            if reason.contains("head moved before publication") || reason.contains("stale info") {
                return Ok(QueueWorkResult::Stale {
                    run_id: Some(run_id),
                    reason,
                });
            }
            return Ok(QueueWorkResult::Failed {
                run_id: Some(run_id),
                reason,
            });
        }
        self.record_publication(
            run_id,
            head_sha.clone(),
            pull_request.url.clone(),
            PublicationStatus::Succeeded,
        )?;
        Ok(QueueWorkResult::Repaired { run_id, head_sha })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubPullRequest {
    number: u64,
    title: String,
    url: String,
    author: GitHubAuthor,
    is_draft: bool,
    head_ref_name: String,
    head_ref_oid: String,
    base_ref_name: String,
    is_cross_repository: bool,
}

#[derive(Deserialize)]
struct GitHubAuthor {
    login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubIssue {
    number: u64,
    title: String,
    body: String,
    url: String,
    labels: Vec<GitHubLabel>,
}

#[derive(Deserialize)]
struct GitHubLabel {
    name: String,
}

impl TryFrom<GitHubIssue> for QueueIssue {
    type Error = anyhow::Error;

    fn try_from(issue: GitHubIssue) -> Result<Self> {
        let metadata = parse_issue_metadata(&issue.body)?;
        Ok(Self {
            number: issue.number,
            title: issue.title,
            body: issue.body,
            url: issue.url,
            labels: issue.labels.into_iter().map(|label| label.name).collect(),
            scope: metadata.scope,
            blocked_by: metadata.blocked_by,
        })
    }
}

#[derive(Deserialize)]
struct GitHubIssueState {
    number: u64,
    state: String,
}

#[derive(Default)]
struct IssueMetadata {
    scope: Vec<String>,
    blocked_by: Vec<u64>,
}

fn parse_issue_metadata(body: &str) -> Result<IssueMetadata> {
    let mut lines = body.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Ok(IssueMetadata::default());
    }
    let mut metadata = IssueMetadata::default();
    let mut section = "";
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some(key) = trimmed.strip_suffix(':') {
            section = key;
            continue;
        }
        let Some(value) = trimmed.strip_prefix('-').map(str::trim) else {
            continue;
        };
        let value = value.trim_matches(['\'', '"']);
        match section {
            "scope" => metadata.scope.push(value.into()),
            "blocked_by" => metadata.blocked_by.push(
                value
                    .parse()
                    .with_context(|| format!("invalid blocked_by issue number {value}"))?,
            ),
            _ => {}
        }
    }
    Ok(metadata)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolingEnvelope {
    status: String,
    data: BTreeMap<String, Value>,
    diagnostics: Vec<ToolingDiagnostic>,
}

#[derive(Deserialize)]
struct ToolingDiagnostic {
    code: Option<String>,
    message: String,
}

fn classify_integration(envelope: &ToolingEnvelope) -> IntegrationResult {
    if envelope.status == "passed"
        && envelope
            .data
            .get("merged")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return IntegrationResult::Merged;
    }
    let codes = envelope
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.as_deref())
        .collect::<BTreeSet<_>>();
    let reason = envelope
        .diagnostics
        .iter()
        .map(|diagnostic| match diagnostic.code.as_deref() {
            Some(code) => format!("[{code}] {}", diagnostic.message),
            None => diagnostic.message.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if codes.iter().any(|code| {
        matches!(
            *code,
            "local-merge-conflict"
                | "local-pipeline-not-green"
                | "pipeline-mutated-tracked-files"
                | "remote-checks-failed"
        )
    }) {
        IntegrationResult::Repairable { reason }
    } else if codes.iter().any(|code| {
        matches!(
            *code,
            "pr-head-moved" | "pr-head-moved-during-fetch" | "base-moved"
        )
    }) {
        IntegrationResult::Refresh { reason }
    } else if envelope.status == "unavailable"
        || codes.iter().any(|code| {
            matches!(
                *code,
                "remote-checks-pending" | "review-blocks-merge" | "pr-is-draft" | "pr-not-open"
            )
        })
    {
        IntegrationResult::Blocked { reason }
    } else {
        IntegrationResult::Failed { reason }
    }
}

fn merge_method(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Merge => "merge",
        MergeMethod::Rebase => "rebase",
        MergeMethod::Squash => "squash",
    }
}

fn local_ref(checkout: &Path, reference: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["show-ref", "--verify", "--hash", reference])
        .current_dir(checkout)
        .output()?;
    if output.status.success() {
        Ok(Some(String::from_utf8(output.stdout)?.trim().into()))
    } else {
        Ok(None)
    }
}

fn command_output(executable: &str, arguments: &[&str], checkout: &Path) -> Result<Vec<u8>> {
    let output = Command::new(executable)
        .args(arguments)
        .current_dir(checkout)
        .output()
        .with_context(|| format!("run {executable} {}", arguments.join(" ")))?;
    if !output.status.success() {
        bail!(
            "{executable} {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn git(checkout: &Path, arguments: &[&str]) -> Result<String> {
    Ok(
        String::from_utf8(command_output("git", arguments, checkout)?)?
            .trim()
            .into(),
    )
}

fn git_ok(checkout: &Path, arguments: &[&str]) -> Result<()> {
    let _ = git(checkout, arguments)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use tempfile::TempDir;

    use super::*;
    use crate::adapters::Provider;

    fn sample_pull_request() -> QueuePullRequest {
        QueuePullRequest {
            number: 42,
            title: "Change".into(),
            url: "https://github.example/owner/demo/pull/42".into(),
            author: "trusted".into(),
            draft: false,
            head_ref: "feature".into(),
            head_sha: "a".repeat(40),
            base_ref: "main".into(),
            same_repository: true,
        }
    }

    #[test]
    fn issue_frontmatter_provides_scope_and_blockers() {
        let metadata = parse_issue_metadata(
            "---\nparent: 1\nblocked_by:\n  - 2\n  - 3\nscope:\n  - src/**\n  - tests/**\n---\n\nImplement it.",
        )
        .unwrap();
        assert_eq!(metadata.blocked_by, vec![2, 3]);
        assert_eq!(metadata.scope, vec!["src/**", "tests/**"]);
    }

    #[test]
    fn integration_failures_are_split_between_repair_and_external_blockers() {
        let repair = ToolingEnvelope {
            status: "failed".into(),
            data: BTreeMap::new(),
            diagnostics: vec![ToolingDiagnostic {
                code: Some("local-pipeline-not-green".into()),
                message: "tests failed".into(),
            }],
        };
        let IntegrationResult::Repairable { reason } = classify_integration(&repair) else {
            panic!("expected repairable integration failure");
        };
        assert_eq!(reason, "[local-pipeline-not-green] tests failed");
        let blocked = ToolingEnvelope {
            status: "unavailable".into(),
            data: BTreeMap::new(),
            diagnostics: vec![ToolingDiagnostic {
                code: Some("review-blocks-merge".into()),
                message: "review required".into(),
            }],
        };
        assert!(matches!(
            classify_integration(&blocked),
            IntegrationResult::Blocked { .. }
        ));
    }

    #[test]
    fn worker_prompts_leave_the_full_completion_gate_to_the_orchestrator() {
        let issue = QueueIssue {
            number: 7,
            title: "Fix it".into(),
            body: "Acceptance criteria".into(),
            url: "https://github.example/owner/demo/issues/7".into(),
            labels: BTreeSet::new(),
            scope: vec!["src/**".into()],
            blocked_by: Vec::new(),
        };
        let issue_prompt = issue_worker_prompt(&issue);
        assert!(issue_prompt.contains("narrowest relevant repository-owned checks"));
        assert!(issue_prompt.contains("orchestrator will independently run"));
        assert!(!issue_prompt.contains("run the full repository-owned checks"));

        let repair_prompt = repair_worker_prompt(&sample_pull_request(), "tests failed");
        assert!(repair_prompt.contains("narrowest relevant repository-owned checks"));
        assert!(repair_prompt.contains("orchestrator will independently run"));
        assert!(!repair_prompt.contains("run the full repository-owned checks"));
    }

    #[test]
    fn prior_failed_repair_is_added_to_changed_evidence_context() {
        let previous = FailedRepairEvidence {
            head_sha: "a".repeat(40),
            integration_signature: evidence_signature("tests failed"),
            integration_reason: "tests failed".into(),
            run_id: Some(Uuid::nil()),
            failure_reason: "agent could not repair it".into(),
        };
        let context = build_repair_context("lint now fails", Some(&previous));
        assert!(context.contains("lint now fails"));
        assert!(context.contains("Previous failed repair attempt"));
        assert!(context.contains("tests failed"));
        assert!(context.contains("agent could not repair it"));
    }

    struct FakePlatform {
        pull_request: QueuePullRequest,
        integrations: VecDeque<IntegrationResult>,
        integration_calls: usize,
    }

    impl QueuePlatform for FakePlatform {
        fn refresh(&mut self) -> Result<()> {
            Ok(())
        }

        fn open_pull_requests(&mut self) -> Result<Vec<QueuePullRequest>> {
            Ok(vec![self.pull_request.clone()])
        }

        fn integrate(&mut self, _pull_request: &QueuePullRequest) -> Result<IntegrationResult> {
            self.integration_calls += 1;
            Ok(self
                .integrations
                .pop_front()
                .unwrap_or(IntegrationResult::Repairable {
                    reason: "still failing".into(),
                }))
        }

        fn issues(&mut self) -> Result<Vec<QueueIssue>> {
            Ok(Vec::new())
        }

        fn issue_states(&mut self, _numbers: &[u64]) -> Result<BTreeMap<u64, String>> {
            Ok(BTreeMap::new())
        }

        fn mark_issue_active(&mut self, _issue: &QueueIssue) -> Result<()> {
            Ok(())
        }

        fn mark_issue_ready(&mut self, _issue: &QueueIssue, _pull_request: u64) -> Result<()> {
            Ok(())
        }

        fn mark_issue_blocked(&mut self, _issue: &QueueIssue, _reason: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailingWorker {
        repairs: usize,
        reasons: Vec<String>,
    }

    impl QueueWorker for FailingWorker {
        fn implement(&mut self, _issue: &QueueIssue) -> Result<QueueWorkResult> {
            unreachable!("fixture has no issue")
        }

        fn repair(
            &mut self,
            _pull_request: &QueuePullRequest,
            reason: &str,
        ) -> Result<QueueWorkResult> {
            self.repairs += 1;
            self.reasons.push(reason.to_owned());
            Ok(QueueWorkResult::Failed {
                run_id: None,
                reason: "agent could not repair it".into(),
            })
        }
    }

    #[test]
    fn queue_stops_on_unchanged_evidence_without_a_second_provider_attempt() {
        let data = TempDir::new().unwrap();
        let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        config.publication.mode = PublicationMode::PullRequest;
        config.queue.max_repair_attempts = 2;
        config.queue.trusted_authors = vec!["trusted".into()];
        let mut platform = FakePlatform {
            pull_request: sample_pull_request(),
            integrations: VecDeque::from([
                IntegrationResult::Repairable {
                    reason: "tests failed".into(),
                },
                IntegrationResult::Repairable {
                    reason: "tests failed".into(),
                },
            ]),
            integration_calls: 0,
        };
        let mut worker = FailingWorker::default();
        let report = QueueRunner::new(
            data.path(),
            "owner/demo",
            &config,
            &mut platform,
            &mut worker,
        )
        .run()
        .unwrap();

        assert_eq!(report.stop_reason, QueueStopReason::Blocked);
        assert_eq!(report.items_processed, 1);
        assert_eq!(worker.repairs, 1);
        assert_eq!(platform.integration_calls, 2);
        assert_eq!(report.usage.integration_evaluations, 2);
        assert_eq!(report.usage.provider_attempts, 1);
        assert_eq!(report.usage.repair_attempts, 1);
        assert_eq!(report.usage.no_information_gain_stops, 1);
        assert_eq!(report.usage.repair_limit_stops, 0);
        assert_eq!(
            report
                .events
                .iter()
                .filter(|event| matches!(event, QueueEvent::RepairFailed { .. }))
                .count(),
            1
        );
        assert!(
            report
                .events
                .iter()
                .any(|event| matches!(event, QueueEvent::RepairSkippedNoInformationGain { .. }))
        );
    }

    #[test]
    fn changed_integration_evidence_justifies_the_second_bounded_attempt() {
        let data = TempDir::new().unwrap();
        let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        config.publication.mode = PublicationMode::PullRequest;
        config.queue.max_repair_attempts = 2;
        config.queue.trusted_authors = vec!["trusted".into()];
        let mut platform = FakePlatform {
            pull_request: sample_pull_request(),
            integrations: VecDeque::from([
                IntegrationResult::Repairable {
                    reason: "tests failed".into(),
                },
                IntegrationResult::Repairable {
                    reason: "lint now fails".into(),
                },
            ]),
            integration_calls: 0,
        };
        let mut worker = FailingWorker::default();
        let report = QueueRunner::new(
            data.path(),
            "owner/demo",
            &config,
            &mut platform,
            &mut worker,
        )
        .run()
        .unwrap();

        assert_eq!(report.stop_reason, QueueStopReason::Blocked);
        assert_eq!(worker.repairs, 2);
        assert_eq!(platform.integration_calls, 2);
        assert_eq!(report.usage.integration_evaluations, 2);
        assert_eq!(report.usage.provider_attempts, 2);
        assert_eq!(report.usage.repair_attempts, 2);
        assert_eq!(report.usage.no_information_gain_stops, 0);
        assert_eq!(report.usage.repair_limit_stops, 1);
        assert!(worker.reasons[1].contains("lint now fails"));
        assert!(worker.reasons[1].contains("Previous failed repair attempt"));
        assert!(worker.reasons[1].contains("tests failed"));
        assert!(worker.reasons[1].contains("agent could not repair it"));
    }

    #[test]
    fn observing_a_new_head_invalidates_failed_repair_evidence() {
        let mut record = PullRequestRecord {
            last_observed_sha: Some("a".repeat(40)),
            repair_attempts: 1,
            last_failed_repair: Some(FailedRepairEvidence {
                head_sha: "a".repeat(40),
                integration_signature: evidence_signature("tests failed"),
                integration_reason: "tests failed".into(),
                run_id: None,
                failure_reason: "agent failed".into(),
            }),
            ..PullRequestRecord::default()
        };
        let mut pull_request = sample_pull_request();
        pull_request.head_sha = "b".repeat(40);
        record.observe(&pull_request);
        assert_eq!(record.repair_attempts, 0);
        assert!(record.last_failed_repair.is_none());
        assert_eq!(record.last_observed_sha, Some("b".repeat(40)));
    }

    #[derive(Default)]
    struct RepairingWorker;

    impl QueueWorker for RepairingWorker {
        fn implement(&mut self, _issue: &QueueIssue) -> Result<QueueWorkResult> {
            unreachable!("fixture has no issue")
        }

        fn repair(
            &mut self,
            _pull_request: &QueuePullRequest,
            _reason: &str,
        ) -> Result<QueueWorkResult> {
            Ok(QueueWorkResult::Repaired {
                run_id: Uuid::nil(),
                head_sha: "b".repeat(40),
            })
        }
    }

    #[test]
    fn queue_stops_at_the_item_limit_even_when_more_progress_is_possible() {
        let data = TempDir::new().unwrap();
        let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        config.publication.mode = PublicationMode::PullRequest;
        config.queue.max_items_per_run = 1;
        config.queue.trusted_authors = vec!["trusted".into()];
        let mut platform = FakePlatform {
            pull_request: sample_pull_request(),
            integrations: VecDeque::from([
                IntegrationResult::Repairable {
                    reason: "tests failed".into(),
                },
                IntegrationResult::Merged,
            ]),
            integration_calls: 0,
        };
        let mut worker = RepairingWorker;
        let report = QueueRunner::new(
            data.path(),
            "owner/demo",
            &config,
            &mut platform,
            &mut worker,
        )
        .run()
        .unwrap();

        assert_eq!(report.stop_reason, QueueStopReason::ItemLimit);
        assert_eq!(report.items_processed, 1);
        assert_eq!(platform.integration_calls, 1);
        assert_eq!(report.usage.integration_evaluations, 1);
        assert_eq!(report.usage.provider_attempts, 1);
        assert_eq!(report.usage.repair_attempts, 1);
        assert!(matches!(
            report.events.as_slice(),
            [QueueEvent::PullRequestRepaired { .. }]
        ));
    }
}
