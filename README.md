# Agent Loop Orchestrator

A single-user localhost control plane for creating, executing, checking, deciding, and locally integrating coding-agent work.

The orchestrator is the local system of record that connects the rest of the coding-agent landscape without absorbing their responsibilities:

| Component | Responsibility |
| --- | --- |
| `agent-contracts` | Neutral, versioned interchange contracts shared across repositories |
| `coding-agent-conventions` | Policy, principles, convention profiles, and stable convention IDs |
| `coding-tooling` | Deterministic repository discovery, affected-scope analysis, and checks |
| `runtime-profiler` | Reproducible runtime evidence capture and immutable evidence bundles |
| `agent-loop-setup` | Reusable worker procedures and environment-specific installation composition |
| `agent-loop-orchestrator` | Repository bootstrap, work items, isolated execution, run state, authority, evidence references, decisions, and local integration |
| `moonlight` | Baseline/candidate comparison and evaluation |
| `local-refactor` | A specialized refactoring worker |

The orchestrator should know **that** evidence and evaluations exist, but should not need to understand profiler metrics, Moonlight comparison internals, or repository-specific check formats. Those boundaries are represented by `agent-contracts`.

## Install once

On macOS, Linux, or WSL:

```bash
git clone git@github.com:moritzbrantner/agent-loop-orchestrator.git
cd agent-loop-orchestrator
./setup.sh
```

The script verifies the platform sandbox (`bubblewrap`/`bwrap` on Linux and WSL, `sandbox-exec` on macOS), installs a minimal Rust toolchain when necessary, installs missing Claude Code and Codex CLIs from their official installers, builds `agent-loop`, installs it under `~/.local/bin`, and installs completion definitions for Bash, Zsh, or Fish. Use `--skip-providers` or `--skip-rust` when those dependencies are managed elsewhere. Provider execution fails closed if the sandbox is unavailable.

Provider authentication remains an explicit one-time user action:

```bash
codex login
claude
```

## Add a repository

From any Git repository:

```bash
agent-loop init
agent-loop doctor
```

`init` creates the versioned `.agent-loop/config.toml`, ignores local run evidence, and registers the repository in the per-user orchestrator registry. It does not overwrite an existing configuration unless you pass `--force`.

Create a bounded work item, start it, inspect the candidate, then approve or reject it:

```bash
agent-loop work-item create --title "Add health endpoint" --prompt-file task.md --scope src --scope tests
agent-loop start <work-item-id> --provider codex
agent-loop show <run-id>
agent-loop approve <run-id>
# or: agent-loop reject <run-id> --reason "Not the intended behavior"
```

`agent-loop run --prompt "..."` is the one-command shorthand for creating and starting a whole-repository work item. It still stops at `awaiting_decision`; integration always requires an explicit approval.

Execution settings are repository-local and backwards-compatible with existing v1 config files:

```toml
[execution]
coding_tooling_executable = "coding-tooling"
check_tier = "fast"
target_branch = "main"
```

See [Claude and Codex adapters](docs/providers.md) for command mappings, permission defaults, event normalization, and the authority boundary.

## Run the LAN dashboard

The dashboard is a React application served by the Rust service. It creates durable work items for projects registered with `agent-loop init`, starts one active run at a time, streams output, displays candidates and deterministic checks, and requires an explicit approve/reject decision.

Build the frontend once after changing its source:

```bash
cd web
bun install
bun run build
cd ..
```

Start the service on the desired LAN interface:

```bash
cargo run -- serve --bind 0.0.0.0:3000
```

The command generates and displays a fresh access token in the terminal. Open `http://<your-machine-lan-address>:3000` and enter that token; the browser retains it only for that session. This MVP uses plain HTTP, so run it only on a trusted network. HTTPS can be restored when remote/LAN hardening becomes a priority.

For frontend development, run `bun run dev` from `web/` while the Rust service is running; Vite proxies `/api` requests to it.

## Interchange contracts

[`moritzbrantner/agent-contracts`](https://github.com/moritzbrantner/agent-contracts) owns all interchange semantics. This orchestrator pins revision `cf0d0c15a743cbf5358f4f3bdd83f38b6371cd98` and emits `agent.run/v1`, with its `agent.authority/v1`, `agent.task-packet/v1`, `agent.candidate/v1`, `agent.check-result/v1`, and `agent.component-lock/v1` records. The local [checksum-verified snapshot](contracts/agent-contracts/PROVENANCE.json) exists only so conformance tests run offline; it is not a fork or a normative schema source.

Neutral `agent.evidence/v1` references represent runtime, check, trace, or other artifacts, while `agent.evaluation-result/v1` represents evaluator outcomes such as future Moonlight results. The orchestrator owns lifecycle and state; it does not own the internal formats or logic of those producers. Provider-specific commands and UI payloads are local implementation details, not replacement contract models.

## Implemented local execution slice

1. A local work item binds a registered project, declared write scope, target branch, and exact baseline SHA.
2. One clean detached Git worktree is created for its single attempt.
3. The selected Claude or Codex adapter receives the canonical `agent.task-packet/v1` and runs inside an OS filesystem sandbox with a read-only host view and write access only to the attempt worktree, detached worktree metadata, a run-local Git object store, and temporary files. Provider API access requires network, so the authority snapshot records network as unrestricted rather than claiming a domain boundary this slice cannot enforce.
4. A successful provider must leave a clean descendant commit whose changed paths are within scope. The commit is retained under an immutable local candidate ref before the worktree is removed.
5. The external `coding-tooling run --tier <tier> --strict --json` process discovers and executes repository checks. Canonical check results are ingested directly; the currently installed legacy envelope is translated only inside the typed adapter. Missing or malformed tooling stops the run explicitly.
6. Passed required checks move the run to `awaiting_decision`. Rejection records a candidate-bound decision and leaves the target unchanged. Approval verifies the target still equals the bound baseline and integrates the exact candidate locally with a fast-forward.
7. Work items, runs, attempts, provider output, task packets, candidates, checks, evidence, decisions, and integration results are persisted below the per-user Agent Loop data directory.

This slice never pushes, opens a pull request, publishes remotely, invokes Moonlight or `runtime-profiler`, retries, or schedules parallel workers.

## Boundary

The orchestrator owns coordination and durable run state. It does not own coding conventions, discover or invent repository checks, capture profiler-specific measurements, decide semantic equivalence itself, or embed Moonlight or specialist implementation logic. It decides when future collectors and evaluators run, not how they normalize measurements or classify differences.

Contract evolution happens in `agent-contracts`; this repository updates its pin deliberately and validates emitted records against that exact revision.
