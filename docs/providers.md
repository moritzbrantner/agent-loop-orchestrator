# Claude and Codex adapters

The orchestrator treats the agent CLI as an adapter boundary. It never builds a shell command, never interpolates prompts into shell source, and never treats a successful process spawn as a successful run.

## Codex

The Codex adapter uses `codex exec --json`, sets the repository with `--cd`, and passes an explicit sandbox and approval policy. Model selection uses `--model`; reasoning effort is a one-run TOML override through `--config model_reasoning_effort=...`.

The default is `workspace-write` plus `never`. This combination is intentional for a headless local run: the process cannot answer approval prompts, while Codex still enforces its workspace sandbox. Broader access is not exposed in the v1 repository configuration.

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

Every provider runs from a clean detached worktree at the recorded baseline. On Linux/WSL, Bubblewrap gives it a read-only host view and narrowly writable worktree/detached-Git-object paths; macOS uses an equivalent deny-by-default `sandbox-exec` filesystem profile. The canonical authority records that host-wide read access, provider authentication, attempt-scoped writes (the worktree, run directory, and detached-worktree metadata root), and denied integration/publication. Actual OS write bindings are narrower than those declared roots. Provider model APIs require network access, so this slice records network as unrestricted and does not claim a domain-level network boundary. If the filesystem sandbox is unavailable, execution fails closed. Candidate objects are written to a run-local object directory and imported through a bundle, so the worker cannot mutate the repository's shared object store or refs. Git hooks, credential helpers, prompts, and SSH transport are disabled in the worker environment; the task packet and prompt deny push/publication. After execution, the orchestrator rejects dirty, non-descendant, missing, or out-of-scope candidates before retaining an immutable candidate ref. The orchestrator itself never invokes a remote publication or push operation, and local integration disables Git hooks. This is a practical boundary for the local worker slice, not a claim that unrestricted provider-API networking is a hostile-code security boundary. Provider flags remain an additional provider-specific layer and must not be presented as stronger isolation than they provide.
