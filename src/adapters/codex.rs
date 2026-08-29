use anyhow::Result;

use super::{AgentAdapter, Provider, RunRequest};
use crate::{
    config::{ProjectConfig, validate_codex_effort},
    process::CommandSpec,
};

pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn provider(&self) -> Provider {
        Provider::Codex
    }

    fn command(&self, config: &ProjectConfig, request: &RunRequest<'_>) -> Result<CommandSpec> {
        let provider = &config.providers.codex;
        let effort = request
            .effort_override
            .or(provider.reasoning_effort.as_deref());
        validate_codex_effort(effort)?;

        let automatically_approve = matches!(
            provider.approval_policy,
            crate::config::CodexApprovalPolicy::Never
        );
        let mut args = vec![
            "exec".into(),
            "--json".into(),
            "--cd".into(),
            request.repository_root.as_os_str().to_owned(),
        ];

        if matches!(
            provider.sandbox,
            crate::config::CodexSandbox::DangerFullAccess
        ) {
            args.push("--dangerously-bypass-approvals-and-sandbox".into());
        } else if automatically_approve {
            args.push("--approve-for-me".into());
        } else {
            args.extend(["--sandbox".into(), provider.sandbox.as_cli_value().into()]);
        }

        if let Some(model) = request.model_override.or(provider.model.as_deref()) {
            args.extend(["--model".into(), model.into()]);
        }
        if let Some(effort) = effort {
            args.extend([
                "--config".into(),
                format!("model_reasoning_effort=\"{effort}\"").into(),
            ]);
        }
        if let Some(session) = request.resume_session {
            args.extend(["resume".into(), session.into()]);
        }
        args.push(request.prompt.into());

        Ok(CommandSpec {
            program: provider.executable.clone().into(),
            args,
            current_dir: request.repository_root.to_owned(),
        })
    }

    fn session_id(&self, event: &serde_json::Value) -> Option<String> {
        (event.get("type")?.as_str()? == "thread.started")
            .then(|| event.get("thread_id")?.as_str().map(str::to_owned))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::Provider;

    #[test]
    fn builds_official_non_interactive_jsonl_command() {
        let config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        let request = RunRequest {
            repository_root: std::path::Path::new("/tmp/demo"),
            prompt: "implement it",
            resume_session: None,
            model_override: Some("gpt-5.6-sol"),
            effort_override: Some("xhigh"),
        };
        let command = CodexAdapter.command(&config, &request).unwrap();
        let args: Vec<_> = command
            .args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect();
        assert_eq!(args[0], "exec");
        assert!(args.iter().any(|arg| arg == "--json"));
        assert!(args.iter().any(|arg| arg == "--approve-for-me"));
        assert!(!args.iter().any(|arg| arg == "--ask-for-approval"));
        assert!(!args.iter().any(|arg| arg == "--sandbox"));
        assert!(
            args.iter()
                .any(|arg| arg == "model_reasoning_effort=\"xhigh\"")
        );
    }

    #[test]
    fn resumes_the_requested_exec_session() {
        let config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        let request = RunRequest {
            repository_root: std::path::Path::new("/tmp/demo"),
            prompt: "continue",
            resume_session: Some("0199a213-81c0-7800-8aa1-bbab2a035a53"),
            model_override: None,
            effort_override: None,
        };
        let command = CodexAdapter.command(&config, &request).unwrap();
        let args: Vec<_> = command
            .args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect();
        assert!(
            args.windows(2)
                .any(|pair| { pair == ["resume", "0199a213-81c0-7800-8aa1-bbab2a035a53"] })
        );
    }

    #[test]
    fn uses_explicit_bypass_only_for_full_access_configuration() {
        let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        config.providers.codex.sandbox = crate::config::CodexSandbox::DangerFullAccess;
        let command = CodexAdapter
            .command(
                &config,
                &RunRequest {
                    repository_root: std::path::Path::new("/tmp/demo"),
                    prompt: "implement it",
                    resume_session: None,
                    model_override: None,
                    effort_override: None,
                },
            )
            .unwrap();
        assert!(
            command
                .args
                .iter()
                .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox")
        );
    }
}
