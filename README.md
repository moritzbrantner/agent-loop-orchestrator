# Agent Loop Orchestrator

A single-user localhost control plane for creating, scheduling, executing, evaluating, and integrating coding-agent work.

The orchestrator is the local system of record that connects the rest of the coding-agent landscape without absorbing their responsibilities:

| Component | Responsibility |
| --- | --- |
| `coding-agent-conventions` | Policy, principles, convention profiles, and stable convention IDs |
| `coding-tooling` | Deterministic repository discovery, affected-scope analysis, and checks |
| `agent-loop-setup` | Reusable worker procedures and environment-specific installation composition |
| `agent-loop-orchestrator` | Repository bootstrap, execution adapters, tasks, scheduling, run state, authority, evidence, decisions, and integration |
| `moonlight` | Baseline/candidate comparison and evaluation |
| `local-refactor` | A specialized refactoring worker |

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

## Interchange contracts

[`moritzbrantner/agent-contracts`](https://github.com/moritzbrantner/agent-contracts) owns all interchange semantics. This orchestrator pins revision `cf0d0c15a743cbf5358f4f3bdd83f38b6371cd98` and emits `agent.run/v1`, with its `agent.authority/v1`, `agent.task-packet/v1`, `agent.candidate/v1`, and `agent.component-lock/v1` records. The local [checksum-verified snapshot](contracts/agent-contracts/PROVENANCE.json) exists only so conformance tests run offline; it is not a fork or a normative schema source.

The orchestrator owns lifecycle and state: scheduling, dashboard projections, provider execution, durable evidence, and integration coordination. Provider-specific commands and UI payloads are local implementation details, not replacement contract models.

## Intended vertical slice

1. Create a work item with dependencies and declared scope.
2. Select it when dependency-ready.
3. Create an isolated Git worktree.
4. Start one worker with explicit authority.
5. Discover and run deterministic capabilities through `coding-tooling`.
6. Record the resulting commit or patch as a candidate.
7. Evaluate baseline against candidate through Moonlight.
8. Store immutable evidence.
9. Request a human or policy decision.
10. Integrate locally or explicitly publish through an adapter.

## Boundary

The orchestrator owns coordination and durable run state. It does not own coding conventions, invent repository checks, decide semantic equivalence itself, or embed the implementation logic of specialist workers.

Contract evolution happens in `agent-contracts`; this repository updates its pin deliberately and validates emitted records against that exact revision.
