mod claude;
mod codex;

use std::{fmt, path::Path, str::FromStr};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{config::ProjectConfig, contracts::TaskPacket, process::CommandSpec};

pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;

const TASK_PACKET_PROMPT_PREFIX: &str = "Implement the work described by this canonical agent.task-packet/v1. Leave the worktree clean and commit the completed candidate. You have no authority to integrate, push, publish, or otherwise mutate a remote system.\n\n";

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

/// Provider-neutral seam between orchestrator interchange records and a coding worker.
///
/// The orchestrator keeps `agent.task-packet/v1` as its canonical cross-component
/// record. The provider does not need to treat that schema as its ontology: this
/// adapter turns the packet into the same bounded caller context a standalone
/// implementation worker can receive from a human, skill, or lightweight loop.
struct OrchestratedWorkerAdapter {
    provider: Provider,
}

impl AgentAdapter for OrchestratedWorkerAdapter {
    fn provider(&self) -> Provider {
        self.provider
    }

    fn command(&self, config: &ProjectConfig, request: &RunRequest<'_>) -> Result<CommandSpec> {
        let prompt = adapt_worker_prompt(request.prompt)?;
        let adapted = RunRequest {
            repository_root: request.repository_root,
            prompt: &prompt,
            resume_session: request.resume_session,
            model_override: request.model_override,
            effort_override: request.effort_override,
        };
        match self.provider {
            Provider::Claude => ClaudeAdapter.command(config, &adapted),
            Provider::Codex => CodexAdapter.command(config, &adapted),
        }
    }

    fn session_id(&self, event: &serde_json::Value) -> Option<String> {
        match self.provider {
            Provider::Claude => ClaudeAdapter.session_id(event),
            Provider::Codex => CodexAdapter.session_id(event),
        }
    }
}

pub fn adapter(provider: Provider) -> Box<dyn AgentAdapter> {
    Box::new(OrchestratedWorkerAdapter { provider })
}

fn adapt_worker_prompt(prompt: &str) -> Result<String> {
    let Some(payload) = prompt.strip_prefix(TASK_PACKET_PROMPT_PREFIX) else {
        return Ok(prompt.to_owned());
    };
    let packet: TaskPacket = serde_json::from_str(payload)
        .context("parse canonical task packet before provider invocation")?;
    Ok(render_worker_context(&packet))
}

fn render_worker_context(packet: &TaskPacket) -> String {
    let mut output = String::from(
        "Implement one bounded piece of coding work supplied by an orchestration adapter.\n\n\
When an installed `slice-implementer` procedure is available, follow it. Otherwise use the same caller-agnostic implementation behavior: inspect only as broadly as needed, obey supplied boundaries, run focused checks, and return the result to the caller.\n\n\
The canonical task packet is retained by the orchestrator as interchange provenance. Do not create or discover additional work items, queues, or orchestration state.\n\n",
    );

    output.push_str("Objective:\n");
    append_list(
        &mut output,
        &packet.behavioral_scope,
        "the bounded requested change",
    );

    output.push_str("\nCaller references:\n");
    output.push_str(&format!("- work item: {}\n", packet.work_item_id));
    output.push_str(&format!("- delegated slice: {}\n", packet.slice_id));
    output.push_str(&format!("- stage: {}\n", packet.stage));

    output.push_str("\nHard boundaries supplied by the caller:\n");
    output.push_str(&format!(
        "- exact baseline Git SHA: {}\n",
        packet.baseline.git_sha
    ));
    output.push_str("- mutable/write scope:\n");
    append_indented_list(&mut output, &packet.write_scope, "(none declared)");
    if !packet.protected_behavior.is_empty() {
        output.push_str("- protected behavior:\n");
        append_indented_list(&mut output, &packet.protected_behavior, "(none)");
    }
    if !packet.excluded_capabilities.is_empty() {
        output.push_str("- excluded capabilities:\n");
        append_indented_list(&mut output, &packet.excluded_capabilities, "(none)");
    }
    output.push_str(&format!(
        "- integration authority: {}\n",
        if packet.authority.may_integrate {
            "granted"
        } else {
            "not granted"
        }
    ));
    output.push_str(&format!(
        "- publication authority: {}\n",
        if packet.authority.may_publish {
            "granted"
        } else {
            "not granted"
        }
    ));

    output.push_str("\nAcceptance evidence requested by the caller:\n");
    if packet.acceptance.is_empty() {
        output.push_str("- no additional structured acceptance criteria\n");
    } else {
        for criterion in &packet.acceptance {
            let requirement = if criterion.required {
                "required"
            } else {
                "optional"
            };
            let component = criterion
                .component
                .as_deref()
                .map(|value| format!(" for {value}"))
                .unwrap_or_default();
            output.push_str(&format!(
                "- [{requirement}] {}: {}{component}\n",
                criterion.id, criterion.capability
            ));
        }
    }

    output.push_str(
        "\nCompletion contract for this orchestrated invocation:\n\
- stay within the supplied write/authority boundaries;\n\
- leave the worktree clean and commit the completed candidate;\n\
- report meaningful focused checks, risks, and blockers when the provider interface supports it;\n\
- do not integrate, push, publish, schedule other work, or invent durable task state.\n",
    );
    output
}

fn append_list(output: &mut String, values: &[String], fallback: &str) {
    if values.is_empty() {
        output.push_str(&format!("- {fallback}\n"));
    } else {
        for value in values {
            output.push_str(&format!("- {value}\n"));
        }
    }
}

fn append_indented_list(output: &mut String, values: &[String], fallback: &str) {
    if values.is_empty() {
        output.push_str(&format!("  - {fallback}\n"));
    } else {
        for value in values {
            output.push_str(&format!("  - {value}\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        AcceptanceCriterion, Authority, Baseline, ExpectedCapabilityState, HandoffRequirements,
        NetworkAuthority, NetworkMode,
    };

    fn task_packet() -> TaskPacket {
        TaskPacket {
            schema_version: 1,
            slice_id: "slice-1".into(),
            work_item_id: "work-1".into(),
            baseline: Baseline {
                git_sha: "0123456789abcdef".into(),
                r#ref: Some("main".into()),
            },
            primary_convention: "repository-default".into(),
            convention_refs: vec!["repository-default".into()],
            stage: "implementation".into(),
            target_surfaces: vec!["src".into(), "tests".into()],
            behavioral_scope: vec![
                "Add the health endpoint without changing existing API behavior".into(),
            ],
            write_scope: vec!["src".into(), "tests".into()],
            protected_behavior: vec!["existing API behavior".into()],
            excluded_capabilities: vec!["remote-publication".into()],
            dependencies: Vec::new(),
            acceptance: vec![AcceptanceCriterion {
                id: "unit-tests".into(),
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
            authority: Authority {
                schema_version: 1,
                read_roots: vec!["/".into()],
                write_roots: vec!["/tmp/worktree".into()],
                network: NetworkAuthority {
                    mode: NetworkMode::Unrestricted,
                    allowed_domains: Vec::new(),
                },
                tools: vec!["provider:codex".into(), "git".into()],
                secret_refs: Vec::new(),
                max_duration_seconds: 900,
                max_attempts: 1,
                may_integrate: false,
                may_publish: false,
            },
        }
    }

    #[test]
    fn direct_provider_prompt_passes_through_unchanged() {
        let prompt = "Fix the typo and run the focused test.";
        assert_eq!(adapt_worker_prompt(prompt).unwrap(), prompt);
    }

    #[test]
    fn task_packet_becomes_caller_agnostic_worker_context() {
        let packet = task_packet();
        let prompt = format!(
            "{TASK_PACKET_PROMPT_PREFIX}{}",
            serde_json::to_string_pretty(&packet).unwrap()
        );
        let adapted = adapt_worker_prompt(&prompt).unwrap();

        assert!(adapted.contains("one bounded piece of coding work"));
        assert!(adapted.contains("caller-agnostic implementation behavior"));
        assert!(adapted.contains("Add the health endpoint"));
        assert!(adapted.contains("exact baseline Git SHA: 0123456789abcdef"));
        assert!(adapted.contains("  - src"));
        assert!(adapted.contains("[required] unit-tests: test:unit"));
        assert!(adapted.contains("integration authority: not granted"));
        assert!(adapted.contains("do not integrate, push, publish, schedule other work"));
        assert!(!adapted.contains("\"schemaVersion\""));
        assert!(
            !adapted
                .contains("Implement the work described by this canonical agent.task-packet/v1")
        );
    }
}
