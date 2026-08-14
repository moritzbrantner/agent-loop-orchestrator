mod claude;
mod codex;

use std::{fmt, path::Path, str::FromStr};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::{config::ProjectConfig, process::CommandSpec};

pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

impl fmt::Display for Provider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        })
    }
}

impl FromStr for Provider {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            _ => bail!("unknown provider `{value}`; expected claude or codex"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunRequest<'a> {
    pub repository_root: &'a Path,
    pub prompt: &'a str,
    pub resume_session: Option<&'a str>,
    pub model_override: Option<&'a str>,
    pub effort_override: Option<&'a str>,
}

pub trait AgentAdapter {
    fn provider(&self) -> Provider;
    fn command(&self, config: &ProjectConfig, request: &RunRequest<'_>) -> Result<CommandSpec>;
    fn session_id(&self, event: &serde_json::Value) -> Option<String>;
}

pub fn adapter(provider: Provider) -> Box<dyn AgentAdapter> {
    match provider {
        Provider::Claude => Box::new(ClaudeAdapter),
        Provider::Codex => Box::new(CodexAdapter),
    }
}
