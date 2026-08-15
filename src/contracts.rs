//! Serde models for the pinned `agent-contracts` interchange contracts.
//!
//! These types deliberately describe exchanged records only. Dashboard state,
//! provider event envelopes, and command-line options remain orchestrator-local.

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{adapters::Provider, config::ProjectConfig};

pub const AGENT_CONTRACTS_REPOSITORY: &str = "https://github.com/moritzbrantner/agent-contracts";
pub const AGENT_CONTRACTS_REVISION: &str = "cf0d0c15a743cbf5358f4f3bdd83f38b6371cd98";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Authority {
    pub schema_version: u8,
    pub read_roots: Vec<String>,
    pub write_roots: Vec<String>,
    pub network: NetworkAuthority,
    pub tools: Vec<String>,
    pub secret_refs: Vec<String>,
    pub max_duration_seconds: u64,
    pub max_attempts: u8,
    pub may_integrate: bool,
    pub may_publish: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkAuthority {
    pub mode: NetworkMode,
    pub allowed_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NetworkMode {
    None,
    Allowlist,
    Unrestricted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Candidate {
    pub schema_version: u8,
    pub candidate_id: String,
    pub kind: CandidateKind,
    pub baseline_git_sha: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch_digest: Option<String>,
    pub produced_by_attempt_id: String,
    pub changed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateKind {
    GitCommit,
    Patch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentLock {
    pub schema_version: u8,
    pub generated_at: DateTime<Utc>,
    pub channel: ComponentChannel,
    pub resolver: ComponentResolver,
    pub components: BTreeMap<String, LockedComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ComponentChannel {
    Stable,
    Canary,
    Dev,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentResolver {
    pub component: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum LockedComponent {
    Release(ReleaseComponent),
    Local(LocalComponent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseComponent {
    pub source: ReleaseSource,
    pub version: String,
    pub repository: String,
    pub git_sha: String,
    pub tag: String,
    pub digest: String,
    pub protocols: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReleaseSource {
    #[serde(rename = "release")]
    Release,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalComponent {
    pub source: LocalSource,
    pub path: String,
    pub git_sha: String,
    pub dirty: bool,
    pub content_digest: String,
    pub protocols: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LocalSource {
    #[serde(rename = "local")]
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskPacket {
    pub schema_version: u8,
    pub slice_id: String,
    pub work_item_id: String,
    pub baseline: Baseline,
    pub primary_convention: String,
    pub convention_refs: Vec<String>,
    pub stage: String,
    pub target_surfaces: Vec<String>,
    pub behavioral_scope: Vec<String>,
    pub write_scope: Vec<String>,
    pub protected_behavior: Vec<String>,
    pub excluded_capabilities: Vec<String>,
    pub dependencies: Vec<String>,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub expected_capability_state: ExpectedCapabilityState,
    pub handoff: HandoffRequirements,
    pub authority: Authority,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub capability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExpectedCapabilityState {
    Absent,
    Partial,
    Satisfied,
    OptedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandoffRequirements {
    pub candidate_required: bool,
    pub changed_paths_required: bool,
    pub evidence_required: bool,
    pub unresolved_dependencies_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Run {
    pub schema_version: u8,
    pub run_id: String,
    pub state: RunState,
    pub project: RunProject,
    pub work_item: RunWorkItem,
    pub baseline: Baseline,
    pub convention_selection: ConventionSelection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tooling_manifest: Option<ToolingManifest>,
    pub component_set: ComponentSet,
    pub agent: RunAgent,
    pub authority: Authority,
    pub attempts: Vec<Attempt>,
    pub candidates: Vec<Candidate>,
    pub checks: Vec<CheckResult>,
    pub evaluations: Vec<EvaluationResult>,
    pub decisions: Vec<Decision>,
    pub publications: Vec<Publication>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued,
    Preparing,
    Running,
    CandidateReady,
    Evaluating,
    AwaitingDecision,
    Integrating,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunProject {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunWorkItem {
    pub id: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub dependencies: Vec<String>,
    pub declared_scope: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Baseline {
    pub git_sha: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConventionSelection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_version: Option<String>,
    pub convention_ids: Vec<String>,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolingManifest {
    pub manifest_id: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentSet {
    pub lock_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunAgent {
    pub adapter: String,
    pub identity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_digest: Option<String>,
    pub configuration_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attempt {
    pub attempt_id: String,
    pub number: u32,
    pub workspace: String,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    pub outcome: AttemptOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AttemptOutcome {
    Running,
    Candidate,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Evidence {
    pub schema_version: u8,
    pub kind: String,
    pub uri: String,
    pub digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateIdentity {
    pub kind: CandidateKind,
    pub identity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckResult {
    pub schema_version: u8,
    pub check_id: String,
    pub capability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    pub candidate: CandidateIdentity,
    pub outcome: CheckOutcome,
    pub required: bool,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CheckOutcome {
    Passed,
    Failed,
    Unavailable,
    Skipped,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvaluationResult {
    pub schema_version: u8,
    pub evaluation_id: String,
    pub evaluator: Evaluator,
    pub baseline: EvaluationIdentity,
    pub candidate: EvaluationIdentity,
    pub outcome: EvaluationOutcome,
    pub summary: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Evaluator {
    pub component: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration_digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvaluationIdentity {
    pub kind: EvaluationIdentityKind,
    pub identity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EvaluationIdentityKind {
    GitCommit,
    Patch,
    EvidenceBundle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EvaluationOutcome {
    Passed,
    Failed,
    Inconclusive,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Decision {
    pub decision_id: String,
    pub candidate_identity: String,
    pub decision: DecisionOutcome,
    pub actor: String,
    pub occurred_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionOutcome {
    Approved,
    Rejected,
    ChangesRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Publication {
    pub publication_id: String,
    pub kind: PublicationKind,
    pub candidate_identity: String,
    pub status: PublicationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PublicationKind {
    LocalIntegration,
    PullRequest,
    Release,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PublicationStatus {
    Pending,
    Succeeded,
    Failed,
}

impl Run {
    pub fn begin(
        run_id: impl Into<String>,
        config: &ProjectConfig,
        provider: Provider,
        repository_root: &Path,
        prompt: &str,
    ) -> Result<(Self, ComponentLock)> {
        let run_id = run_id.into();
        let baseline = baseline(repository_root)?;
        let lock = component_lock(repository_root, &baseline.git_sha);
        let lock_digest = digest_json(&lock)?;
        let config_digest = digest_bytes(config.to_toml()?.as_bytes());
        let now = Utc::now();
        let attempt_id = format!("{run_id}-attempt-1");
        let root = repository_root.display().to_string();
        let adapter = provider.to_string();
        Ok((
            Self {
                schema_version: 1,
                run_id: run_id.clone(),
                state: RunState::Running,
                project: RunProject {
                    id: config.project.id.clone(),
                    repository: repository_identity(repository_root),
                    local_root: Some(root.clone()),
                },
                work_item: RunWorkItem {
                    id: format!("interactive-{run_id}"),
                    version: "1".into(),
                    title: Some(prompt.into()),
                    dependencies: Vec::new(),
                    declared_scope: vec![".".into()],
                },
                baseline: baseline.clone(),
                convention_selection: ConventionSelection {
                    component_version: None,
                    convention_ids: Vec::new(),
                    digest: digest_bytes(b"agent-loop-orchestrator: no convention selection"),
                },
                tooling_manifest: None,
                component_set: ComponentSet {
                    lock_digest,
                    lock_uri: Some("component-lock.json".into()),
                },
                agent: RunAgent {
                    adapter: adapter.clone(),
                    identity: format!("{adapter}-provider"),
                    model: selected_model(config, provider),
                    prompt_digest: Some(digest_bytes(prompt.as_bytes())),
                    configuration_digest: config_digest,
                },
                authority: Authority {
                    schema_version: 1,
                    read_roots: vec![root.clone()],
                    write_roots: vec![root],
                    network: NetworkAuthority {
                        mode: NetworkMode::None,
                        allowed_domains: Vec::new(),
                    },
                    tools: vec![format!("provider:{adapter}")],
                    secret_refs: Vec::new(),
                    max_duration_seconds: config.agent.max_duration_seconds,
                    max_attempts: 1,
                    may_integrate: false,
                    may_publish: false,
                },
                attempts: vec![Attempt {
                    attempt_id,
                    number: 1,
                    workspace: repository_root.display().to_string(),
                    started_at: now,
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
            },
            lock,
        ))
    }

    pub fn complete(
        &mut self,
        repository_root: &Path,
        provider_session_id: Option<String>,
        succeeded: bool,
        cancelled: bool,
        evidence_directory: &Path,
    ) -> Result<()> {
        let attempt = self.attempts.first_mut().context("run has no attempt")?;
        attempt.finished_at = Some(Utc::now());
        attempt.provider_session_id = provider_session_id;
        attempt.evidence = evidence_from_directory(evidence_directory)?;
        if cancelled {
            attempt.outcome = AttemptOutcome::Cancelled;
            self.state = RunState::Cancelled;
        } else if succeeded {
            let patch = git(
                repository_root,
                &["diff", "--binary", &self.baseline.git_sha],
            )?;
            let changed_paths = git(
                repository_root,
                &["diff", "--name-only", &self.baseline.git_sha],
            )?
            .lines()
            .map(str::to_owned)
            .filter(|path| !path.is_empty())
            .collect();
            attempt.outcome = AttemptOutcome::Candidate;
            self.candidates.push(Candidate {
                schema_version: 1,
                candidate_id: format!("{}-candidate-1", self.run_id),
                kind: CandidateKind::Patch,
                baseline_git_sha: self.baseline.git_sha.clone(),
                git_sha: None,
                patch_digest: Some(digest_bytes(patch.as_bytes())),
                produced_by_attempt_id: attempt.attempt_id.clone(),
                changed_paths,
            });
            self.state = RunState::Completed;
        } else {
            attempt.outcome = AttemptOutcome::Failed;
            self.state = RunState::Failed;
        }
        Ok(())
    }

    pub fn write_to(&self, path: &Path) -> Result<()> {
        fs::write(path, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("write {}", path.display()))
    }
}

pub fn begin_persisted_run(
    run_id: impl Into<String>,
    config: &ProjectConfig,
    provider: Provider,
    repository_root: &Path,
    prompt: &str,
    run_directory: &Path,
) -> Result<Run> {
    fs::create_dir_all(run_directory)
        .with_context(|| format!("create {}", run_directory.display()))?;
    let (run, lock) = Run::begin(run_id, config, provider, repository_root, prompt)?;
    write_component_lock(&lock, &run_directory.join("component-lock.json"))?;
    run.write_to(&run_directory.join("run.json"))?;
    Ok(run)
}

pub fn complete_persisted_run(
    run: &mut Run,
    repository_root: &Path,
    provider_session_id: Option<String>,
    succeeded: bool,
    cancelled: bool,
    run_directory: &Path,
) -> Result<()> {
    run.complete(
        repository_root,
        provider_session_id,
        succeeded,
        cancelled,
        run_directory,
    )?;
    run.write_to(&run_directory.join("run.json"))
}

pub fn write_component_lock(lock: &ComponentLock, path: &Path) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(lock)?)
        .with_context(|| format!("write {}", path.display()))
}

fn baseline(repository_root: &Path) -> Result<Baseline> {
    Ok(Baseline {
        git_sha: git(repository_root, &["rev-parse", "HEAD"])?
            .trim()
            .to_owned(),
        r#ref: git(
            repository_root,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
        )
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty()),
    })
}

fn component_lock(repository_root: &Path, git_sha: &str) -> ComponentLock {
    let mut protocols = BTreeMap::new();
    for protocol in [
        "agent.run",
        "agent.authority",
        "agent.task-packet",
        "agent.candidate",
        "agent.component-lock",
    ] {
        protocols.insert(protocol.into(), "v1".into());
    }
    let mut components = BTreeMap::new();
    components.insert(
        "agent-loop-orchestrator".into(),
        LockedComponent::Local(LocalComponent {
            source: LocalSource::Local,
            path: repository_root.display().to_string(),
            git_sha: git_sha.into(),
            dirty: false,
            content_digest: digest_bytes(git_sha.as_bytes()),
            protocols,
        }),
    );
    ComponentLock {
        schema_version: 1,
        generated_at: Utc::now(),
        channel: ComponentChannel::Dev,
        resolver: ComponentResolver {
            component: "agent-loop-orchestrator".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        components,
    }
}

fn selected_model(config: &ProjectConfig, provider: Provider) -> Option<String> {
    match provider {
        Provider::Codex => config.providers.codex.model.clone(),
        Provider::Claude => config.providers.claude.model.clone(),
    }
}

fn repository_identity(repository_root: &Path) -> Option<String> {
    git(repository_root, &["remote", "get-url", "origin"])
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn evidence_from_directory(directory: &Path) -> Result<Vec<Evidence>> {
    [
        ("provider-events", "events.jsonl", "application/x-ndjson"),
        ("provider-stdout", "raw.jsonl", "application/x-ndjson"),
        ("provider-stderr", "stderr.log", "text/plain"),
    ]
    .into_iter()
    .filter_map(|(kind, name, media_type)| {
        let path = directory.join(name);
        path.exists().then_some((kind, path, media_type))
    })
    .map(|(kind, path, media_type)| {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        Ok(Evidence {
            schema_version: 1,
            kind: kind.into(),
            uri: name_for_uri(&path),
            digest: digest_bytes(&bytes),
            media_type: Some(media_type.into()),
            created_at: Utc::now(),
            size_bytes: Some(bytes.len() as u64),
        })
    })
    .collect()
}

fn name_for_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

fn git(repository_root: &Path, arguments: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(arguments)
        .current_dir(repository_root)
        .output()
        .with_context(|| format!("run git {}", arguments.join(" ")))?;
    if !output.status.success() {
        anyhow::bail!("git {} failed", arguments.join(" "));
    }
    String::from_utf8(output.stdout).context("git returned non-UTF-8 output")
}

pub fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn digest_json(value: &impl Serialize) -> Result<String> {
    Ok(digest_bytes(&serde_json::to_vec(value)?))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use jsonschema::{Draft, Resource};
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize)]
    struct Provenance {
        source: String,
        revision: String,
        files: Vec<SnapshotFile>,
    }

    #[derive(Debug, Deserialize)]
    struct SnapshotFile {
        path: String,
        sha256: String,
    }

    #[test]
    fn snapshot_is_the_exact_pinned_agent_contracts_revision() {
        let root = snapshot_root();
        let provenance: Provenance = serde_json::from_slice(
            &fs::read(root.join("PROVENANCE.json")).expect("read snapshot provenance"),
        )
        .expect("parse snapshot provenance");
        assert_eq!(provenance.source, AGENT_CONTRACTS_REPOSITORY);
        assert_eq!(provenance.revision, AGENT_CONTRACTS_REVISION);
        for file in provenance.files {
            let bytes = fs::read(root.join(&file.path)).expect("read generated snapshot file");
            assert_eq!(digest_bytes(&bytes), format!("sha256:{}", file.sha256));
        }
    }

    #[test]
    fn emitted_contract_types_conform_to_pinned_schemas() {
        let (run, lock) = fixture_run(RunState::Completed, AttemptOutcome::Candidate);
        let run_json = serde_json::to_value(&run).expect("serialize run");
        let authority_json = serde_json::to_value(&run.authority).expect("serialize authority");
        let lock_json = serde_json::to_value(&lock).expect("serialize component lock");
        let candidate_json = serde_json::to_value(&run.candidates[0]).expect("serialize candidate");
        let task_packet_json = serde_json::to_value(TaskPacket {
            schema_version: 1,
            slice_id: "slice-1".into(),
            work_item_id: "work-1".into(),
            baseline: Baseline {
                git_sha: "a".repeat(40),
                r#ref: Some("main".into()),
            },
            primary_convention: "TEST-001".into(),
            convention_refs: vec!["TEST-001".into()],
            stage: "implementation".into(),
            target_surfaces: vec!["src/contracts.rs".into()],
            behavioral_scope: vec!["canonical serialization".into()],
            write_scope: vec!["src/contracts.rs".into()],
            protected_behavior: Vec::new(),
            excluded_capabilities: Vec::new(),
            dependencies: Vec::new(),
            acceptance: vec![AcceptanceCriterion {
                id: "acceptance-1".into(),
                capability: "test:unit".into(),
                component: None,
                required: true,
            }],
            expected_capability_state: ExpectedCapabilityState::Satisfied,
            handoff: HandoffRequirements {
                candidate_required: true,
                changed_paths_required: true,
                evidence_required: true,
                unresolved_dependencies_required: true,
            },
            authority: run.authority.clone(),
        })
        .expect("serialize task packet");

        validate("schemas/run/v1.schema.json", &run_json);
        validate("schemas/authority/v1.schema.json", &authority_json);
        validate("schemas/candidate/v1.schema.json", &candidate_json);
        validate("schemas/component/lock-v1.schema.json", &lock_json);
        validate("schemas/task-packet/v1.schema.json", &task_packet_json);
        for (state, outcome) in [
            (RunState::Completed, AttemptOutcome::Candidate),
            (RunState::Failed, AttemptOutcome::Failed),
            (RunState::Cancelled, AttemptOutcome::Cancelled),
        ] {
            let (run, _) = fixture_run(state, outcome);
            validate(
                "schemas/run/v1.schema.json",
                &serde_json::to_value(run).expect("serialize terminal run"),
            );
        }
    }

    fn fixture_run(state: RunState, outcome: AttemptOutcome) -> (Run, ComponentLock) {
        let occurred_at = "2026-08-15T02:10:00Z".parse().expect("fixed timestamp");
        let baseline = Baseline {
            git_sha: "a".repeat(40),
            r#ref: Some("main".into()),
        };
        let authority = fixture_authority();
        let candidate = Candidate {
            schema_version: 1,
            candidate_id: "candidate-1".into(),
            kind: CandidateKind::Patch,
            baseline_git_sha: baseline.git_sha.clone(),
            git_sha: None,
            patch_digest: Some(format!("sha256:{}", "d".repeat(64))),
            produced_by_attempt_id: "attempt-1".into(),
            changed_paths: vec!["src/contracts.rs".into()],
        };
        let candidate_identity = CandidateIdentity {
            kind: CandidateKind::Patch,
            identity: candidate.patch_digest.clone().expect("patch digest"),
        };
        let evidence = Evidence {
            schema_version: 1,
            kind: "provider-events".into(),
            uri: "file:///evidence/events.jsonl".into(),
            digest: format!("sha256:{}", "e".repeat(64)),
            media_type: Some("application/x-ndjson".into()),
            created_at: occurred_at,
            size_bytes: Some(0),
        };
        let lock = ComponentLock {
            schema_version: 1,
            generated_at: occurred_at,
            channel: ComponentChannel::Dev,
            resolver: ComponentResolver {
                component: "agent-loop-orchestrator".into(),
                version: "0.1.0".into(),
            },
            components: BTreeMap::from([(
                "agent-loop-orchestrator".into(),
                LockedComponent::Local(LocalComponent {
                    source: LocalSource::Local,
                    path: "/project".into(),
                    git_sha: baseline.git_sha.clone(),
                    dirty: false,
                    content_digest: format!("sha256:{}", "f".repeat(64)),
                    protocols: BTreeMap::from([("agent.run".into(), "v1".into())]),
                }),
            )]),
        };
        let run = Run {
            schema_version: 1,
            run_id: "run-conformance".into(),
            state,
            project: RunProject {
                id: "project".into(),
                repository: Some("owner/project".into()),
                local_root: Some("/project".into()),
            },
            work_item: RunWorkItem {
                id: "work-1".into(),
                version: "1".into(),
                title: Some("Conform exactly".into()),
                dependencies: Vec::new(),
                declared_scope: vec!["src/contracts.rs".into()],
            },
            baseline: baseline.clone(),
            convention_selection: ConventionSelection {
                component_version: Some("0.1.0".into()),
                convention_ids: vec!["TEST-001".into()],
                digest: format!("sha256:{}", "b".repeat(64)),
            },
            tooling_manifest: Some(ToolingManifest {
                manifest_id: "tooling-1".into(),
                digest: format!("sha256:{}", "c".repeat(64)),
            }),
            component_set: ComponentSet {
                lock_digest: digest_json(&lock).expect("digest lock"),
                lock_uri: Some("component-lock.json".into()),
            },
            agent: RunAgent {
                adapter: "codex".into(),
                identity: "codex-provider".into(),
                model: Some("test-model".into()),
                prompt_digest: Some(format!("sha256:{}", "1".repeat(64))),
                configuration_digest: format!("sha256:{}", "2".repeat(64)),
            },
            authority,
            attempts: vec![Attempt {
                attempt_id: "attempt-1".into(),
                number: 1,
                workspace: "/project".into(),
                started_at: occurred_at,
                finished_at: Some(occurred_at),
                outcome,
                provider_session_id: Some("session-1".into()),
                evidence: vec![evidence.clone()],
            }],
            candidates: vec![candidate],
            checks: vec![CheckResult {
                schema_version: 1,
                check_id: "check-1".into(),
                capability: "test:unit".into(),
                component: Some(".".into()),
                candidate: candidate_identity.clone(),
                outcome: CheckOutcome::Passed,
                required: true,
                started_at: occurred_at,
                finished_at: occurred_at,
                duration_ms: Some(0),
                exit_code: Some(0),
                reason: None,
                evidence: vec![evidence.clone()],
            }],
            evaluations: vec![EvaluationResult {
                schema_version: 1,
                evaluation_id: "evaluation-1".into(),
                evaluator: Evaluator {
                    component: "moonlight".into(),
                    version: Some("1".into()),
                    protocol: "agent.evaluation/v1".into(),
                    configuration_digest: None,
                },
                baseline: EvaluationIdentity {
                    kind: EvaluationIdentityKind::GitCommit,
                    identity: baseline.git_sha.clone(),
                },
                candidate: EvaluationIdentity {
                    kind: EvaluationIdentityKind::Patch,
                    identity: candidate_identity.identity.clone(),
                },
                outcome: EvaluationOutcome::Passed,
                summary: "Passed".into(),
                started_at: occurred_at,
                finished_at: occurred_at,
                evidence: vec![evidence.clone()],
            }],
            decisions: vec![Decision {
                decision_id: "decision-1".into(),
                candidate_identity: candidate_identity.identity.clone(),
                decision: DecisionOutcome::Approved,
                actor: "user".into(),
                occurred_at,
                reason: None,
            }],
            publications: vec![Publication {
                publication_id: "publication-1".into(),
                kind: PublicationKind::LocalIntegration,
                candidate_identity: candidate_identity.identity,
                status: PublicationStatus::Succeeded,
                external_id: None,
                occurred_at,
                evidence: vec![evidence],
            }],
        };
        (run, lock)
    }

    fn fixture_authority() -> Authority {
        Authority {
            schema_version: 1,
            read_roots: vec!["/project".into()],
            write_roots: vec!["/project".into()],
            network: NetworkAuthority {
                mode: NetworkMode::None,
                allowed_domains: Vec::new(),
            },
            tools: vec!["provider:codex".into()],
            secret_refs: Vec::new(),
            max_duration_seconds: 1,
            max_attempts: 1,
            may_integrate: false,
            may_publish: false,
        }
    }

    fn validate(schema_path: &str, instance: &serde_json::Value) {
        let root = snapshot_root();
        let schema: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(schema_path)).expect("read pinned schema"))
                .expect("parse pinned schema");
        let validator = jsonschema::options()
            .with_draft(Draft::Draft202012)
            .with_resources(resources(&root).into_iter())
            .build(&schema)
            .expect("compile pinned schema");
        let errors: Vec<_> = validator.iter_errors(instance).collect();
        assert!(errors.is_empty(), "schema errors: {errors:#?}");
    }

    fn resources(root: &std::path::Path) -> Vec<(String, Resource)> {
        [
            (
                "urn:agent-contracts:authority:v1",
                "schemas/authority/v1.schema.json",
            ),
            (
                "urn:agent-contracts:candidate:v1",
                "schemas/candidate/v1.schema.json",
            ),
            (
                "urn:agent-contracts:evidence:v1",
                "schemas/evidence-v1.schema.json",
            ),
            (
                "urn:agent-contracts:check-result:v1",
                "schemas/tooling/check-result-v1.schema.json",
            ),
            (
                "urn:agent-contracts:evaluation-result:v1",
                "schemas/evaluation/result-v1.schema.json",
            ),
        ]
        .into_iter()
        .map(|(uri, path)| {
            let value = serde_json::from_slice(
                &fs::read(root.join(path)).expect("read referenced pinned schema"),
            )
            .expect("parse referenced pinned schema");
            (uri.into(), Resource::from_contents(value))
        })
        .collect()
    }

    fn snapshot_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("contracts/agent-contracts")
    }
}
