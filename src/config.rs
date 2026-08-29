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
    #[serde(default, skip_serializing_if = "RemoteConfig::is_default")]
    pub remote: RemoteConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfig {
    pub coding_tooling_executable: String,
    pub check_tier: String,
    pub target_branch: String,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            coding_tooling_executable: "coding-tooling".into(),
            check_tier: "fast".into(),
            target_branch: "main".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RemoteConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default = "default_github_executable")]
    pub github_executable: String,
    #[serde(default = "default_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default)]
    pub auto_merge: bool,
    #[serde(default)]
    pub repair_failures: bool,
    #[serde(default)]
    pub repair_conflicts: bool,
    #[serde(default = "default_max_repair_attempts")]
    pub max_repair_attempts: u32,
    #[serde(default)]
    pub merge_method: MergeMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Provider>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trusted_authors: Vec<String>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            repository: None,
            github_executable: default_github_executable(),
            poll_interval_seconds: default_poll_interval_seconds(),
            auto_merge: false,
            repair_failures: false,
            repair_conflicts: false,
            max_repair_attempts: default_max_repair_attempts(),
            merge_method: MergeMethod::default(),
            provider: None,
            trusted_authors: Vec::new(),
        }
    }
}

impl RemoteConfig {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn repository(&self) -> Result<&str> {
        self.repository
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .context("remote.repository is required when remote automation is enabled")
    }

    fn validate(&self) -> Result<()> {
        if self.github_executable.trim().is_empty() {
            bail!("remote.github_executable cannot be empty");
        }
        if self.poll_interval_seconds == 0 {
            bail!("remote.poll_interval_seconds must be greater than zero");
        }
        if self.max_repair_attempts == 0 {
            bail!("remote.max_repair_attempts must be greater than zero");
        }
        if self.enabled {
            validate_repository_slug(self.repository()?)?;
        } else if let Some(repository) = self.repository.as_deref() {
            validate_repository_slug(repository)?;
        }
        if self.enabled
            && (self.auto_merge || self.repair_failures || self.repair_conflicts)
            && self.trusted_authors.is_empty()
        {
            bail!(
                "remote.trusted_authors must name at least one trusted GitHub login before remote mutation is enabled"
            );
        }
        let mut seen = HashSet::new();
        for author in &self.trusted_authors {
            if author.trim().is_empty() || author.chars().any(char::is_whitespace) {
                bail!("remote.trusted_authors entries cannot be empty or contain whitespace");
            }
            if !seen.insert(author.to_ascii_lowercase()) {
                bail!("remote.trusted_authors contains duplicate `{author}`");
            }
        }
        Ok(())
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

const fn default_poll_interval_seconds() -> u64 {
    30
}

const fn default_max_repair_attempts() -> u32 {
    2
}

fn validate_repository_slug(value: &str) -> Result<()> {
    let Some((owner, repository)) = value.split_once('/') else {
        bail!("remote.repository must use OWNER/REPOSITORY format");
    };
    if !valid_repository_segment(owner)
        || !valid_repository_segment(repository)
        || repository.contains('/')
    {
        bail!("remote.repository must use OWNER/REPOSITORY format");
    }
    Ok(())
}

fn valid_repository_segment(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
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
            remote: RemoteConfig::default(),
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
        self.remote.validate()?;
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
        assert!(!encoded.contains("[paths]"));
        assert!(!encoded.contains("[reviews]"));
        assert!(!encoded.contains("[remote]"));
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
        assert_eq!(config.remote, RemoteConfig::default());
    }

    #[test]
    fn enabled_remote_automation_requires_a_repository_slug() {
        let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        config.remote.enabled = true;
        assert!(config.validate().is_err());

        config.remote.repository = Some("owner/demo".into());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn remote_mutation_requires_an_explicit_trusted_author() {
        let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        config.remote.enabled = true;
        config.remote.repository = Some("owner/demo".into());
        config.remote.auto_merge = true;
        assert!(config.validate().is_err());

        config.remote.trusted_authors = vec!["trusted-login".into()];
        assert!(config.validate().is_ok());
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
