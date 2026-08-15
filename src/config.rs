use std::{fs, path::Path};

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
}

impl CodexSandbox {
    pub fn as_cli_value(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
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
                    // Headless processes cannot answer approval prompts. The workspace sandbox is
                    // therefore the enforcement boundary for the default local run.
                    approval_policy: CodexApprovalPolicy::Never,
                },
                claude: ClaudeConfig {
                    executable: "claude".into(),
                    model: None,
                    effort: Some("high".into()),
                    // dontAsk fails closed instead of hanging on an interactive prompt.
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
        Ok(())
    }
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
    }

    #[test]
    fn provider_efforts_are_validated_independently() {
        assert!(validate_codex_effort(Some("xhigh")).is_ok());
        assert!(validate_codex_effort(Some("max")).is_err());
        assert!(validate_claude_effort(Some("max")).is_ok());
        assert!(validate_claude_effort(Some("minimal")).is_err());
    }

    #[test]
    fn pre_execution_slice_config_uses_safe_execution_defaults() {
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
    }
}
