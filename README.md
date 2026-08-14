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

## Shared run contract

Every execution is represented by a versioned, provider-neutral run contract. It binds the work item, baseline, agent identity, allowed authority, attempts, candidate, checks, evaluations, decisions, and publication to one durable record.

- Normative design: [docs/run-contract.md](docs/run-contract.md)
- JSON Schema: [schemas/run-contract-v1.schema.json](schemas/run-contract-v1.schema.json)
- Example: [examples/run-contract-v1.json](examples/run-contract-v1.json)

The schema is the interchange boundary. Database tables, HTTP payloads, internal Rust types, GitHub adapters, and agent-specific formats may differ internally, but they must preserve its semantics.

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

The contract starts inside this repository. It should be extracted into a separate package or repository only after multiple external consumers require independent compatibility and release management.
