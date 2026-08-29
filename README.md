# Agent Loop Orchestrator

An optional single-user localhost coordination layer for coding-agent workloads that need durable work items, isolated execution, run state, candidate decisions, or local integration.

Direct human-to-agent work, standalone skills, deterministic tooling, and lightweight iterative loops do **not** require this repository, `agent-loop init`, or orchestrator registration. The orchestrator is an escalation layer: use it when coordination complexity justifies durable control state.

When selected, the orchestrator is the local system of record that connects the rest of the coding-agent landscape without absorbing their responsibilities:

| Component | Responsibility |
| --- | --- |
| `agent-contracts` | Neutral, versioned interchange contracts shared across repositories |
| `coding-agent-conventions` | Stable engineering policy, principles, convention profiles, and convention IDs |
| `coding-agent-skills` | General reusable reasoning skills, executable flows, and automatic-use profiles |
| `coding-tooling` | Deterministic repository discovery, affected-scope analysis, checks, and capability-source/profile resolution |
| `runtime-profiler` | Reproducible runtime evidence capture and immutable evidence bundles |
| `agent-loop-setup` | Machine bootstrap, shared component registration, independent agent profiles, and setup documentation |
| `agent-loop-orchestrator` | Optional repository bootstrap, work items, isolated execution, run state, authority, evidence references, decisions, and local integration |
| `moonlight` | Baseline/candidate comparison and evaluation |
| `local-refactor` | A specialized refactoring worker |

The orchestrator should know **that** evidence and evaluations exist, but should not need to understand profiler metrics, Moonlight comparison internals, or repository-specific check formats. Those boundaries are represented by `agent-contracts`.

## When to use the orchestrator

Start with a direct run or independently invokable procedure when one agent can safely own the requested change and repository-owned checks can establish completion.

The current runtime earns its additional ceremony when the workload benefits from one or more of:

- durable work-item/run state or resumability;
- explicit readiness dependencies between work items;
- one isolated provider attempt with bounded authority and retained output;
- immutable candidate identity plus explicit approve/reject decisions;
- exact-candidate local integration when the target branch still matches the candidate's bound baseline.

Dependency metadata currently gates readiness but does not refresh a dependent work item's frozen baseline after an earlier item integrates. Create or resume downstream work against the current baseline rather than assuming a pre-created chain will be automatically rebased and integrated.

The queue runner adds deterministic serial scheduling for GitHub pull requests and agent-ready issues. Parallel workers and coordinated integration of a pre-created dependent chain remain future work.

Do not create a work item merely to invoke a reusable skill or to make a small sequential code change. The surrounding coding-agent landscape follows a progressive model: direct run → reusable procedures → iterative loop → work items → orchestration.

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

The wider coding-agent component locations are registered once by `agent-loop-setup` in `${XDG_CONFIG_HOME:-~/.config}/moenarch/environment.toml`. `agent-loop doctor` reads that registry and reports the core component set (`coding-agent-conventions`, `coding-agent-skills`, and `coding-tooling`) together with provider readiness. If `coding-tooling` is not installed on `PATH`, startup exposes the executable source CLI from the registered checkout through a process-local runtime shim; nothing is copied into the target repository.

## Opt a repository into orchestrated mode

From a Git repository that needs orchestrated execution:

```bash
agent-loop init
agent-loop doctor
```

`init` creates the versioned `.agent-loop/config.toml`, ignores local run evidence, and registers the repository in the per-user orchestrator registry. It does not overwrite an existing configuration unless you pass `--force`. Repositories using only direct agents, standalone procedures, or lightweight loops do not need this step.

Create a bounded work item, start it, inspect the candidate, then approve or reject it:

```bash
agent-loop work-item create --title "Add health endpoint" --prompt-file task.md --scope src --scope tests
agent-loop start <work-item-id> --provider codex
agent-loop show <run-id>
agent-loop approve <run-id>
# or: agent-loop reject <run-id> --reason "Not the intended behavior"
```

`agent-loop run --prompt "..."` is the one-command shorthand **inside orchestrated mode** for creating and starting a whole-repository work item. It still stops at `awaiting_decision`; integration always requires an explicit approval. A truly direct external coding-agent invocation can bypass the orchestrator entirely.

Execution settings are repository-local and backwards-compatible with existing v1 config files:

```toml
[execution]
coding_tooling_executable = "coding-tooling"
check_tier = "fast"
target_branch = "main"
```

The default executable name participates in machine-registry fallback. An explicit non-default executable remains repository-controlled.

See [Claude and Codex adapters](docs/providers.md) for command mappings, permission defaults, event normalization, and the authority boundary.

## Stable control surface for orchestration adapters

`agent-loop control` is the machine-readable integration boundary for callers, `coding-agent-skills` flows/adapters, and local automation **that opt into orchestrated mode**. Independently invoked skills do not need to parse this interface merely to run. Consumers participating in orchestration should not parse the orchestrator's runtime files or database layout.

Create bounded intent with explicit objective, acceptance criteria, dependencies, and scope:

```bash
agent-loop control work-item create \
  --title "Add health endpoint" \
  --objective "Add GET /health without changing existing API behavior" \
  --acceptance tests=test:unit \
  --acceptance build=build \
  --scope src \
  --scope tests
```

Then inspect readiness, execute, and make an exact-candidate decision:

```bash
agent-loop control work-item list
agent-loop control start <work-item-id> --provider codex
agent-loop control status <run-id>
agent-loop control approve <run-id>
# or: agent-loop control reject <run-id> --reason "Not the intended behavior"
```

A prior provider session can be continued with:

```bash
agent-loop control resume <prior-run-id>
```

Every `agent-loop control` invocation emits a versioned JSON envelope. `agent-loop doctor --json` remains the provider/environment preflight. See [Stable control surface](docs/control-surface.md) for the result shapes, readiness semantics, resume behavior, and error codes.

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

[`moritzbrantner/agent-contracts`](https://github.com/moritzbrantner/agent-contracts) owns all interchange semantics used by this orchestrated boundary. This orchestrator pins revision `cf0d0c15a743cbf5358f4f3bdd83f38b6371cd98` and emits `agent.run/v1`, with its `agent.authority/v1`, `agent.task-packet/v1`, `agent.candidate/v1`, `agent.check-result/v1`, and `agent.component-lock/v1` records. The local [checksum-verified snapshot](contracts/agent-contracts/PROVENANCE.json) exists only so conformance tests run offline; it is not a fork or a normative schema source.

These contracts do not imply that every direct/local coding-agent invocation must first become an orchestrator work item. They apply when independently owned components exchange state through this orchestration boundary.

Neutral `agent.evidence/v1` references represent runtime, check, trace, or other artifacts, while `agent.evaluation-result/v1` represents evaluator outcomes such as future Moonlight results. The orchestrator owns lifecycle and state for runs it coordinates; it does not own the internal formats or logic of those producers. Provider-specific commands and UI payloads are local implementation details, not replacement contract models.

## Implemented local execution slice

1. A local work item binds a registered project, declared write scope, target branch, and exact baseline SHA.
2. One clean detached Git worktree is created for its single attempt.
3. The selected Claude or Codex adapter receives the canonical `agent.task-packet/v1` and normally runs inside an OS filesystem sandbox with a read-only host view and write access only to the attempt worktree, detached worktree metadata, a run-local Git object store, and temporary files. A repository may explicitly opt Codex into `danger-full-access` for trusted local automation; that bypasses both filesystem sandboxes and is never the default. Provider API access requires network, so the authority snapshot records network as unrestricted rather than claiming a domain boundary this slice cannot enforce.
4. A successful provider must leave a clean descendant commit whose changed paths are within scope. The commit is retained under an immutable local candidate ref before the worktree is removed.
5. The external `coding-tooling run --tier <tier> --strict --json` process discovers and executes repository checks. The executable may come from `PATH` or the shared machine registry fallback. Canonical check results are ingested directly; the currently installed legacy envelope is translated only inside the typed adapter. Missing or malformed tooling stops the run explicitly.
6. Passed required checks move the run to `awaiting_decision`. Rejection records a candidate-bound decision and leaves the target unchanged. Approval verifies the target still equals the bound baseline and integrates the exact candidate locally with a fast-forward.
7. Work items, runs, attempts, provider output, task packets, candidates, checks, evidence, decisions, integration results, and control intent metadata are persisted below the per-user Agent Loop data directory.

The local execution slice never pushes or publishes. Pull-request publication is a separate, opt-in module used by the queue runner after a candidate passes its full deterministic checks. Providers never receive publication or merge authority.

## Publish and run the GitHub queue

Queue automation is opt-in. It uses a separate automation checkout below the per-user data directory and never switches, resets, cleans, or merges the developer checkout.

Configure one registered project explicitly:

```toml
[publication]
mode = "pull-request"
remote = "origin"

[queue]
max_repair_attempts = 2
max_items_per_run = 20
merge_method = "squash"
provider = "codex"
trusted_authors = ["YOUR_GITHUB_LOGIN"]
```

Install GitHub CLI and run `gh auth login` with permission to read issues and pull requests, edit issue labels/comments, push same-repository branches, create pull requests, and merge through the repository's normal policy. `agent-loop doctor` checks GitHub CLI authentication and repository access when pull-request publication is configured.

Run the queue until it becomes blocked, becomes empty, or reaches its item bound:

```bash
agent-loop queue run
agent-loop queue run --until-blocked
```

Each cycle refreshes the automation checkout, gives open non-draft pull requests to `coding-tooling pr integrate`, and reacts to that command's structured result. A merge is therefore possible only through coding-tooling's exact-head, full-check integration gate; the orchestrator never invokes `gh pr merge` itself.

Repairable integration failures create one isolated repair Work Item at the exact observed PR head. A checked repair candidate is pushed back to the same branch with `--force-with-lease=<branch>:<old-sha>` and integration is retried. A moved head fails closed and refreshes. After two failed repair cycles for one PR, that item stops and is reported.

When no PR is actionable, the runner selects the lowest-numbered open, non-PRD issue labeled `ready-for-agent` whose YAML frontmatter declares `scope` and whose `blocked_by` issues are closed. It creates a bounded Work Item, runs the standard provider procedure with the `full` check tier, pushes the exact checked candidate to `agent-loop/issue-<number>`, creates a ready (non-draft) PR that closes the issue, verifies the PR head, and records its number-bearing URL as the candidate's pull-request publication.

PRs published by this queue are remembered as queue-owned. Other existing PRs are actionable only when their author appears in `queue.trusted_authors`; this is also the trust decision that allows their code and checks to execute locally. Pending checks, required reviews, drafts, contributor-fork repairs, blocked issues, missing issue scope, and exhausted repair budgets are reported as blockers. `max_items_per_run = 20` is an independent safety bound even for an `--until-blocked` invocation.

## Boundary

When selected, the orchestrator owns coordination and durable run state. It does not own coding conventions, reusable reasoning procedures, repository check discovery, profiler-specific measurements, semantic equivalence, Moonlight internals, or specialist implementation logic. It decides when future collectors and evaluators run, not how they normalize measurements or classify differences.

Outside orchestrated mode, those lower-level components and agent procedures remain independently usable. Contract evolution happens in `agent-contracts`; this repository updates its pin deliberately and validates emitted records against that exact revision.
