# Agent Loop Orchestrator

A single-user localhost control plane for creating, scheduling, executing, evaluating, and integrating coding-agent work.

The orchestrator is the local system of record that connects the rest of the coding-agent landscape without absorbing their responsibilities:

| Component | Responsibility |
| --- | --- |
| `agent-contracts` | Neutral, versioned interchange contracts shared across repositories |
| `coding-agent-conventions` | Policy, principles, convention profiles, and stable convention IDs |
| `coding-tooling` | Deterministic repository discovery, affected-scope analysis, and checks |
| `runtime-profiler` | Reproducible runtime evidence capture and immutable evidence bundles |
| `agent-loop-setup` | Reusable worker procedures and environment-specific installation composition |
| `agent-loop-orchestrator` | Repository bootstrap, execution adapters, tasks, scheduling, run state, authority, evidence references, decisions, and integration |
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

The script installs a minimal Rust toolchain when necessary, installs missing Claude Code and Codex CLIs from their official installers, builds `agent-loop`, installs it under `~/.local/bin`, and installs completion definitions for Bash, Zsh, or Fish. Use `--skip-providers` or `--skip-rust` when those dependencies are managed elsewhere.

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

Run a task with the configured provider or choose one for a single run:

```bash
agent-loop run --prompt "Implement the next ready task"
agent-loop run --provider claude --effort max --prompt-file task.md
agent-loop run --provider codex --effort xhigh --prompt "Review the candidate"
```

Continue the same provider session:

```bash
agent-loop run --provider codex --resume <thread-id> --prompt "Apply the review findings"
agent-loop run --provider claude --resume <session-id> --prompt "Run the final checks"
```

See [Claude and Codex adapters](docs/providers.md) for command mappings, permission defaults, event normalization, and the authority boundary.

## Run the LAN dashboard

The dashboard is a React application served by the Rust service. It lists projects registered with `agent-loop init`, starts one active run at a time, keeps a single editable pending run, streams output, and preserves run history locally.

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

## Shared contracts

Cross-repository interchange is owned by `agent-contracts`. In particular, the orchestrator should converge on:

- `agent.run/v1` for the durable aggregate;
- `agent.authority/v1` for enforceable worker authority;
- `agent.task-packet/v1` for bounded delegation;
- `agent.candidate/v1` for immutable candidate identity;
- `agent.evidence/v1` for references to runtime, check, trace, or other evidence artifacts;
- `agent.check-result/v1` for deterministic validation outcomes;
- `agent.evaluation-result/v1` for evaluator outcomes such as Moonlight results;
- `agent.component-lock/v1` for the exact compatible component set used by a run.

This repository currently still contains an older local run-contract schema and examples:

- [docs/run-contract.md](docs/run-contract.md)
- [schemas/run-contract-v1.schema.json](schemas/run-contract-v1.schema.json)
- [examples/run-contract-v1.json](examples/run-contract-v1.json)

Treat those files as a transitional compatibility mirror, not as the source of truth for new cross-repository integrations. New integrations must target the corresponding `agent-contracts` identities, and the local mirror should be retired as the implementation migrates.

## Intended vertical slice

1. Create a work item with dependencies and declared scope.
2. Select it when dependency-ready.
3. Create an isolated Git worktree.
4. Start one worker with explicit authority.
5. Discover and run deterministic capabilities through `coding-tooling`.
6. Record the resulting commit or patch as a candidate.
7. Capture additional evidence such as runtime measurements through a producer like `runtime-profiler` when required by policy.
8. Evaluate baseline against candidate through Moonlight or another evaluator.
9. Store neutral evidence references and evaluation results in the durable run.
10. Request a human or policy decision.
11. Integrate locally or explicitly publish through an adapter.

The orchestrator decides **when** collectors and evaluators run. It does not decide how runtime measurements are normalized or how Moonlight classifies differences.

## Boundary

The orchestrator owns coordination and durable run state. It does not own coding conventions, invent repository checks, capture profiler-specific measurements, decide semantic equivalence itself, or embed the implementation logic of specialist workers.

Shared cross-repository semantics belong in `agent-contracts`. Provider adapters, database tables, HTTP payloads, and internal Rust types may remain orchestrator-specific as long as they preserve those contract semantics at the repository boundary.
