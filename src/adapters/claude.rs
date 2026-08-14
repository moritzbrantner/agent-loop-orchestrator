use anyhow::Result;

use super::{AgentAdapter, Provider, RunRequest};
use crate::{
    config::{ProjectConfig, validate_claude_effort},
    process::CommandSpec,
};

pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn provider(&self) -> Provider {
        Provider::Claude
    }

    fn command(&self, config: &ProjectConfig, request: &RunRequest<'_>) -> Result<CommandSpec> {
        let provider = &config.providers.claude;
        let effort = request.effort_override.or(provider.effort.as_deref());
        validate_claude_effort(effort)?;

        let mut args = vec![
            "--print".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
            "--permission-mode".into(),
            provider.permission_mode.as_cli_value().into(),
        ];

        if !provider.allowed_tools.is_empty() {
            args.extend(["--tools".into(), provider.allowed_tools.join(",").into()]);
        }
        if let Some(model) = request.model_override.or(provider.model.as_deref()) {
            args.extend(["--model".into(), model.into()]);
        }
        if let Some(effort) = effort {
            args.extend(["--effort".into(), effort.into()]);
        }
        if let Some(session) = request.resume_session {
            args.extend(["--resume".into(), session.into()]);
        }
        args.push(request.prompt.into());

        Ok(CommandSpec {
            program: provider.executable.clone().into(),
            args,
            current_dir: request.repository_root.to_owned(),
        })
    }

    fn session_id(&self, event: &serde_json::Value) -> Option<String> {
        event
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_official_headless_streaming_command() {
        let config = ProjectConfig::default_for("demo".into(), Provider::Claude);
        let request = RunRequest {
            repository_root: std::path::Path::new("/tmp/demo"),
            prompt: "implement it",
            resume_session: None,
            model_override: Some("sonnet"),
            effort_override: Some("max"),
        };
        let command = ClaudeAdapter.command(&config, &request).unwrap();
        let args: Vec<_> = command
            .args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect();
        assert!(args.iter().any(|arg| arg == "--print"));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--output-format", "stream-json"])
        );
        assert!(args.iter().any(|arg| arg == "--verbose"));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--permission-mode", "dontAsk"])
        );
        assert!(args.windows(2).any(|pair| pair == ["--effort", "max"]));
    }

    #[test]
    fn resumes_the_requested_claude_session() {
        let config = ProjectConfig::default_for("demo".into(), Provider::Claude);
        let request = RunRequest {
            repository_root: std::path::Path::new("/tmp/demo"),
            prompt: "continue",
            resume_session: Some("550e8400-e29b-41d4-a716-446655440000"),
            model_override: None,
            effort_override: None,
        };
        let command = ClaudeAdapter.command(&config, &request).unwrap();
        let args: Vec<_> = command
            .args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect();
        assert!(
            args.windows(2)
                .any(|pair| { pair == ["--resume", "550e8400-e29b-41d4-a716-446655440000"] })
        );
    }
}
