use std::{collections::HashSet, fs, path::Path, str::FromStr};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::adapters::Provider;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub version: u32,
    pub project: Project,
    pub agent: AgentDefaults,
    pub providers: Providers,
    #[serde(default)]
    pub execution: ExecutionConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default, skip_serializing_if = "KnowledgePaths::is_default")]
    pub paths: KnowledgePaths,
    #[serde(default, skip_serializing_if = "ReviewConfig::is_default")]
    pub reviews: ReviewConfig,
    #[serde(default, skip_serializing_if = "PublicationConfig::is_default")]
    pub publication: PublicationConfig,
    #[serde(default, skip_serializing_if = "QueueConfig::is_default")]
    pub queue: QueueConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublicationConfig {
    #[serde(default)]
    pub mode: PublicationMode,
    #[serde(default = "default_remote")]
    pub remote: String,
    #[serde(default = "default_github_executable")]
    pub github_executable: String,
}

impl Default for PublicationConfig {
    fn default() -> Self {
        Self {
            mode: PublicationMode::default(),
            remote: default_remote(),
            github_executable: default_github_executable(),
        }
    }
}

impl PublicationConfig {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }

    fn validate(&self) -> Result<()> {
        validate_command_name("publication.github_executable", &self.github_executable)?;
        validate_git_remote("publication.remote", &self.remote)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PublicationMode {
    #[default]
    LocalIntegration,
    PullRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QueueConfig {
    #[serde(default = "default_max_repair_attempts")]
    pub max_repair_attempts: u32,
    #[serde(default = "default_max_items_per_run")]
    pub max_items_per_run: u32,
    #[serde(default)]
    pub merge_method: MergeMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Provider>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trusted_authors: Vec<String>,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_repair_attempts: default_max_repair_attempts(),
            max_items_per_run: default_max_items_per_run(),
            merge_method: MergeMethod::default(),
            provider: None,
            trusted_authors: Vec::new(),
        }
    }
}

impl QueueConfig {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }

    fn validate(&self) -> Result<()> {
        if self.max_repair_attempts == 0 {
            bail!("queue.max_repair_attempts must be greater than zero");
        }
        if self.max_items_per_run == 0 {
            bail!("queue.max_items_per_run must be greater than zero");
        }
        let mut seen = HashSet::new();
        for author in &self.trusted_authors {
            if author.trim().is_empty() || author.chars().any(char::is_whitespace) {
                bail!("queue.trusted_authors entries cannot be empty or contain whitespace");
            }
            if !seen.insert(author.to_ascii_lowercase()) {
                bail!("queue.trusted_authors contains duplicate `{author}`");
            }
        }
        Ok(())
    }
}

fn default_remote() -> String {
    "origin".into()
}

const fn default_max_items_per_run() -> u32 {
    20
}

fn validate_command_name(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} cannot be empty");
    }
    Ok(())
}

fn validate_git_remote(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.starts_with('-') || value.chars().any(char::is_whitespace) {
        bail!("{label} must be a non-empty Git remote name without whitespace");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfig {
    pub coding_tooling_executable: String,
    pub check_tier: String,
    pub target_branch: String,
    #[serde(default)]
    pub environment_profile: EnvironmentProfile,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            coding_tooling_executable: "coding-tooling".into(),
            check_tier: "fast".into(),
            target_branch: "main".into(),
            environment_profile: EnvironmentProfile::Default,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EnvironmentProfile {
    #[default]
    Default,
    SourceDevelopment,
}

impl EnvironmentProfile {
    pub fn as_cli_value(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::SourceDevelopment => "source-development",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MergeMethod {
    Merge,
    Rebase,
    #[default]
    Squash,
}

impl MergeMethod {
    pub fn as_gh_flag(self) -> &'static str {
        match self {
            Self::Merge => "--merge",
            Self::Rebase => "--rebase",
            Self::Squash => "--squash",
        }
    }
}

fn default_github_executable() -> String {
    "gh".into()
}

const fn default_max_repair_attempts() -> u32 {
    2
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SkillProfile {
    Minimal,
    #[default]
    Standard,
    Custom,
}

impl FromStr for SkillProfile {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "minimal" => Ok(Self::Minimal),
            "standard" => Ok(Self::Standard),
            "custom" => Ok(Self::Custom),
            other => {
                bail!("unsupported skill profile `{other}`; expected minimal, standard, or custom")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsConfig {
    #[serde(default)]
    pub profile: SkillProfile,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            profile: SkillProfile::Standard,
            capabilities: Vec::new(),
        }
    }
}

impl SkillsConfig {
    pub fn for_profile(profile: SkillProfile, capabilities: Vec<String>) -> Result<Self> {
        let config = Self {
            profile,
            capabilities,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.profile != SkillProfile::Custom && !self.capabilities.is_empty() {
            bail!("skills.capabilities is only valid when skills.profile = \"custom\"");
        }
        let mut seen = HashSet::new();
        for capability in &self.capabilities {
            let Some((namespace, name)) = capability.split_once('/') else {
                bail!("custom capability `{capability}` must use a namespaced stable ID");
            };
            if namespace.trim().is_empty()
                || name.trim().is_empty()
                || namespace.chars().any(char::is_whitespace)
                || name.chars().any(char::is_whitespace)
            {
                bail!("custom capability `{capability}` must use a namespaced stable ID");
            }
            if !seen.insert(capability) {
                bail!("custom capability `{capability}` is duplicated");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct KnowledgePaths {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adrs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviews: Option<String>,
}

impl KnowledgePaths {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ReviewConfig {
    #[serde(default)]
    pub persist: bool,
}

impl ReviewConfig {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentDefaults {
    pub provider: Provider,
    pub max_duration_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Providers {
    pub codex: CodexConfig,
    pub claude: ClaudeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CodexConfig {
    pub executable: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub sandbox: CodexSandbox,
    pub approval_policy: CodexApprovalPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CodexSandbox {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

impl CodexSandbox {
    pub fn as_cli_value(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CodexApprovalPolicy {
    Never,
    OnRequest,
    Untrusted,
}

impl CodexApprovalPolicy {
    pub fn as_cli_value(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::OnRequest => "on-request",
            Self::Untrusted => "untrusted",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClaudeConfig {
    pub executable: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: ClaudePermissionMode,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ClaudePermissionMode {
    Plan,
    AcceptEdits,
    DontAsk,
    Auto,
}

impl ClaudePermissionMode {
    pub fn as_cli_value(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::AcceptEdits => "acceptEdits",
            Self::DontAsk => "dontAsk",
            Self::Auto => "auto",
        }
    }
}

impl ProjectConfig {
    pub fn default_for(project_id: String, provider: Provider) -> Self {
        Self::default_for_with_skills(project_id, provider, SkillsConfig::default())
    }

    pub fn default_for_with_skills(
        project_id: String,
        provider: Provider,
        skills: SkillsConfig,
    ) -> Self {
        Self {
            version: 1,
            project: Project { id: project_id },
            agent: AgentDefaults {
                provider,
                max_duration_seconds: 7_200,
            },
            providers: Providers {
                codex: CodexConfig {
                    executable: "codex".into(),
                    model: None,
                    reasoning_effort: Some("high".into()),
                    sandbox: CodexSandbox::WorkspaceWrite,
                    approval_policy: CodexApprovalPolicy::Never,
                },
                claude: ClaudeConfig {
                    executable: "claude".into(),
                    model: None,
                    effort: Some("high".into()),
                    permission_mode: ClaudePermissionMode::DontAsk,
                    allowed_tools: vec![
                        "Bash".into(),
                        "Edit".into(),
                        "Read".into(),
                        "Write".into(),
                    ],
                },
            },
            execution: ExecutionConfig::default(),
            skills,
            paths: KnowledgePaths::default(),
            reviews: ReviewConfig::default(),
            publication: PublicationConfig::default(),
            queue: QueueConfig::default(),
        }
    }

    pub fn load(repository_root: &Path) -> Result<Self> {
        let path = repository_root.join(".agent-loop/config.toml");
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("read {} (run `agent-loop init` first)", path.display()))?;
        let config: Self =
            toml::from_str(&contents).with_context(|| format!("parse {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).context("serialize project configuration")
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("unsupported .agent-loop config version {}", self.version);
        }
        if self.project.id.trim().is_empty() {
            bail!("project.id cannot be empty");
        }
        if self.agent.max_duration_seconds == 0 {
            bail!("agent.max_duration_seconds must be greater than zero");
        }
        validate_codex_effort(self.providers.codex.reasoning_effort.as_deref())?;
        validate_claude_effort(self.providers.claude.effort.as_deref())?;
        if self.execution.coding_tooling_executable.trim().is_empty() {
            bail!("execution.coding_tooling_executable cannot be empty");
        }
        if self.execution.check_tier.trim().is_empty() {
            bail!("execution.check_tier cannot be empty");
        }
        if self.execution.target_branch.trim().is_empty() {
            bail!("execution.target_branch cannot be empty");
        }
        self.skills.validate()?;
        validate_optional_path("paths.specs", self.paths.specs.as_deref())?;
        validate_optional_path("paths.domain", self.paths.domain.as_deref())?;
        validate_optional_path("paths.adrs", self.paths.adrs.as_deref())?;
        validate_optional_path("paths.reviews", self.paths.reviews.as_deref())?;
        self.publication.validate()?;
        self.queue.validate()?;
        Ok(())
    }
}

fn validate_optional_path(label: &str, value: Option<&str>) -> Result<()> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        bail!("{label} cannot be empty when configured");
    }
    Ok(())
}

pub fn validate_codex_effort(value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        const VALID: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];
        if !VALID.contains(&value) {
            bail!("unsupported Codex reasoning effort `{value}`; expected one of {VALID:?}");
        }
    }
    Ok(())
}

pub fn validate_claude_effort(value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        const VALID: &[&str] = &["low", "medium", "high", "xhigh", "max", "ultracode"];
        if !VALID.contains(&value) {
            bail!("unsupported Claude effort `{value}`; expected one of {VALID:?}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        let encoded = config.to_toml().unwrap();
        let decoded: ProjectConfig = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, config);
        assert_eq!(decoded.skills.profile, SkillProfile::Standard);
        assert_eq!(
            decoded.execution.environment_profile,
            EnvironmentProfile::Default
        );
        assert!(!encoded.contains("[paths]"));
        assert!(!encoded.contains("[reviews]"));
        assert!(!encoded.contains("[publication]"));
        assert!(!encoded.contains("[queue]"));
    }

    #[test]
    fn provider_efforts_are_validated_independently() {
        assert!(validate_codex_effort(Some("xhigh")).is_ok());
        assert!(validate_codex_effort(Some("max")).is_err());
        assert!(validate_claude_effort(Some("max")).is_ok());
        assert!(validate_claude_effort(Some("minimal")).is_err());
    }

    #[test]
    fn pre_execution_slice_config_uses_safe_execution_defaults_and_standard_skills() {
        let encoded = r#"
version = 1

[project]
id = "demo"

[agent]
provider = "codex"
max_duration_seconds = 60

[providers.codex]
executable = "codex"
sandbox = "workspace-write"
approval_policy = "never"

[providers.claude]
executable = "claude"
permission_mode = "dontAsk"
allowed_tools = []
"#;
        let config: ProjectConfig = toml::from_str(encoded).unwrap();
        assert_eq!(config.execution, ExecutionConfig::default());
        assert_eq!(config.skills, SkillsConfig::default());
        assert_eq!(config.publication, PublicationConfig::default());
        assert_eq!(config.queue, QueueConfig::default());
    }

    #[test]
    fn source_development_environment_profile_round_trips() {
        let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        config.execution.environment_profile = EnvironmentProfile::SourceDevelopment;
        let encoded = config.to_toml().unwrap();
        let decoded: ProjectConfig = toml::from_str(&encoded).unwrap();
        assert_eq!(
            decoded.execution.environment_profile,
            EnvironmentProfile::SourceDevelopment
        );
        assert!(encoded.contains("environment_profile = \"source-development\""));
    }

    #[test]
    fn pull_request_publication_and_queue_limits_round_trip() {
        let encoded = r#"
version = 1

[project]
id = "demo"

[agent]
provider = "codex"
max_duration_seconds = 60

[providers.codex]
executable = "codex"
sandbox = "workspace-write"
approval_policy = "never"

[providers.claude]
executable = "claude"
permission_mode = "dontAsk"
allowed_tools = []

[publication]
mode = "pull-request"
remote = "upstream"

[queue]
max_repair_attempts = 2
max_items_per_run = 20
"#;
        let config: ProjectConfig = toml::from_str(encoded).unwrap();
        config.validate().unwrap();
        assert_eq!(config.publication.mode, PublicationMode::PullRequest);
        assert_eq!(config.publication.remote, "upstream");
        assert_eq!(config.queue.max_repair_attempts, 2);
        assert_eq!(config.queue.max_items_per_run, 20);
    }

    #[test]
    fn custom_profile_owns_an_independent_explicit_allowlist() {
        let skills = SkillsConfig::for_profile(
            SkillProfile::Custom,
            vec!["general/grilling".into(), "general/refactor".into()],
        )
        .unwrap();
        let config = ProjectConfig::default_for_with_skills("demo".into(), Provider::Codex, skills);
        let encoded = config.to_toml().unwrap();
        assert!(encoded.contains("profile = \"custom\""));
        assert!(encoded.contains("general/grilling"));
    }

    #[test]
    fn named_profiles_reject_explicit_capability_lists() {
        assert!(
            SkillsConfig::for_profile(SkillProfile::Standard, vec!["general/tdd".into()]).is_err()
        );
    }
}
