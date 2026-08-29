# Claude and Codex adapters

The orchestrator treats the agent CLI as an adapter boundary. It never builds a shell command, never interpolates prompts into shell source, and never treats a successful process spawn as a successful run.

## Codex

The Codex adapter uses `codex exec --json`, sets the repository with `--cd`, and passes an explicit sandbox and approval policy. Model selection uses `--model`; reasoning effort is a one-run TOML override through `--config model_reasoning_effort=...`.

`agent-loop init` configures Codex as `danger-full-access` plus `never` for trusted local automation. This deliberately gives a Codex worker the same filesystem authority as the local agent that launched Agent Loop and avoids nesting current Codex inside Agent Loop's read-only Bubblewrap/sandbox-exec wrapper. Existing repositories keep their explicit configuration; set `providers.codex.sandbox = "danger-full-access"` to opt an older repository into the same behavior.

A repository may still explicitly choose `workspace-write` or `read-only` when it wants stronger provider isolation. Those modes remain supported, but they are no longer the generated default for Codex-backed local automation.

The adapter extracts `thread_id` from the `thread.started` JSONL event. `agent-loop run --resume <thread-id>` may continue that provider session, but still creates one fresh isolated worktree and Attempt at the newly bound baseline.

## Claude Code

The Claude adapter uses `claude --print --output-format stream-json --verbose`. It sets the permission mode, model, effort, and tool restriction explicitly. The default permission mode is `dontAsk`, so a missing permission fails instead of blocking a headless process forever. The default tools are `Bash,Edit,Read,Write`.

The adapter extracts `session_id` from Claude's stream events. `agent-loop run --resume <session-id>` may continue it inside a fresh isolated worktree and Attempt.

## Evidence

Every Run writes durable files under the per-user Agent Loop data directory at `runs/<run-id>/`:

- `run.json`: canonical `agent.run/v1` aggregate
- `component-lock.json`: canonical `agent.component-lock/v1` used by that Run
- `task-packet.json`: exact canonical `agent.task-packet/v1` supplied to the worker
- `raw.jsonl`: the provider's original stdout stream
- `events.jsonl`: provider-neutral envelopes containing the original event
- `stderr.log`: provider diagnostics
- `coding-tooling.json`: machine-readable deterministic check output

The run fails when the executable is missing, configuration is invalid, the process times out, or the provider exits unsuccessfully. A non-JSON stdout line is preserved and represented as a parse-error event instead of being discarded.

## Authority boundary

Every provider still runs from a clean detached worktree at the recorded baseline, but filesystem isolation is a configurable execution policy rather than the source of correctness. Codex repositories initialized for trusted local automation use `danger-full-access`: their authority record names `/` as writable and Agent Loop does not add Bubblewrap or `sandbox-exec` around the provider. Explicitly sandboxed modes may still use the outer OS filesystem boundary.

The durable correctness boundary is after provider execution: Agent Loop rejects missing, dirty, non-descendant, or out-of-scope candidates before retaining an immutable candidate ref, then requires repository-owned deterministic checks before a candidate can be approved or published. Providers are instructed not to integrate, push, publish, or otherwise mutate remote systems; the Queue Runner owns guarded publication and delegates merges to `coding-tooling pr integrate`. Provider model APIs require network access, so the authority record treats network as unrestricted rather than claiming a domain-level boundary it cannot enforce.

Full-access workers are therefore trusted local automation, not hostile-code sandboxes. Use an explicitly restricted provider mode when a repository requires a stronger security boundary.
