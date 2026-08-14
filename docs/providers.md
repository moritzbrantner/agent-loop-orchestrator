# Claude and Codex adapters

The orchestrator treats the agent CLI as an adapter boundary. It never builds a shell command, never interpolates prompts into shell source, and never treats a successful process spawn as a successful run.

## Codex

The Codex adapter uses `codex exec --json`, sets the repository with `--cd`, and passes an explicit sandbox and approval policy. Model selection uses `--model`; reasoning effort is a one-run TOML override through `--config model_reasoning_effort=...`.

The default is `workspace-write` plus `never`. This combination is intentional for a headless local run: the process cannot answer approval prompts, while Codex still enforces its workspace sandbox. Broader access is not exposed in the v1 repository configuration.

The adapter extracts `thread_id` from the `thread.started` JSONL event. A follow-up run uses `codex exec resume <SESSION_ID>` through `agent-loop run --resume <SESSION_ID>`.

## Claude Code

The Claude adapter uses `claude --print --output-format stream-json --verbose`. It sets the permission mode, model, effort, and tool restriction explicitly. The default permission mode is `dontAsk`, so a missing permission fails instead of blocking a headless process forever. The default tools are `Bash,Edit,Read,Write`.

The adapter extracts `session_id` from Claude's stream events. A follow-up run passes `--resume <SESSION_ID>` through `agent-loop run --resume <SESSION_ID>`.

## Evidence

Every run writes these files under `.agent-loop/runs/<run-id>/`:

- `raw.jsonl`: the provider's original stdout stream
- `events.jsonl`: provider-neutral envelopes containing the original event
- `stderr.log`: provider diagnostics
- `outcome.json`: provider, session ID, exit code, timeout state, and evidence path

The run fails when the executable is missing, configuration is invalid, the process times out, or the provider exits unsuccessfully. A non-JSON stdout line is preserved and represented as a parse-error event instead of being discarded.

## Authority boundary

Provider flags are only one enforcement layer. Filesystem roots outside the repository, domain allowlists, secrets, and publication rights from the shared run contract must be materialized by the orchestrator's workspace/container layer before starting an adapter. An adapter must not claim that a provider flag enforces an authority it cannot enforce.
