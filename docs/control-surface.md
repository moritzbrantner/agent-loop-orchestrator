# Stable control surface

Thin skills and local automation should integrate with the orchestrator through the public CLI, not by reading `execution-state.json`, `control-metadata.json`, run directories, database tables, or other runtime internals.

The stable machine-readable boundary is the `agent-loop control` command family. Every invocation emits one JSON object with `schemaVersion: 1`, `ok`, a result `kind`, and `data` on success. Failures emit the same envelope with a stable error `code` and human-readable `message`.

Provider/environment preflight remains available through `agent-loop doctor --json`.

## Create bounded work

```bash
agent-loop control work-item create \
  --title "Add health endpoint" \
  --objective "Add GET /health and preserve existing API behavior" \
  --acceptance tests=test:unit \
  --acceptance build=build \
  --scope src \
  --scope tests
```

Dependencies use durable local work-item UUIDs:

```bash
agent-loop control work-item create \
  --title "Wire UI" \
  --objective "Consume the completed health endpoint" \
  --dependency <work-item-id> \
  --acceptance tests=test \
  --scope web
```

Acceptance values use the canonical `agent.task-packet/v1` `AcceptanceCriterion` shape (`ID=CAPABILITY`). The control layer persists objective, acceptance, dependencies, and scope in orchestrator-owned per-user state; consumers do not maintain a second queue file.

## Inspect readiness

```bash
agent-loop control work-item list
agent-loop control status <work-item-or-run-id>
```

Work-item status includes `readiness` plus explicit dependency blockers. A dependency is satisfied only after the referenced local work item is approved/integrated.

Run status includes the durable run contract: exact baseline, provider attempt/session, candidate identity and changed paths, deterministic checks/evidence, decisions, publication/integration state, and any run error.

## Execute and decide

```bash
agent-loop control start <work-item-id> --provider codex
agent-loop control status <run-id>
agent-loop control approve <run-id>
# or
agent-loop control reject <run-id> --reason "Not the intended behavior"
```

Approval/rejection delegates to the same exact-candidate decision boundary as the existing local execution lifecycle. Stale target baselines still fail closed.

## Resume a provider session

```bash
agent-loop control resume <prior-run-id>
```

Resume creates a fresh bounded work item against the current target branch, reuses the prior objective/acceptance/dependencies/scope, and passes the prior provider session identity through the provider adapter. An awaiting-decision run must be approved or rejected before a continuation is created.

## Error codes

The JSON error envelope distinguishes at least:

- `dependency_blocked`
- `awaiting_decision`
- `not_found`
- `missing_project_config`
- `provider_unavailable`
- `tooling_unavailable_or_failed`
- `invalid_task`
- `conflict`
- `control_error`

A completed command may still return a run whose domain status is `failed`; consumers must inspect the returned run state instead of interpreting process success as candidate success.

## Boundary with agent-loop-setup

`agent-loop-setup` control/planning/handoff skills should call this CLI plus `agent-loop doctor --json`. They must not depend on the orchestrator's internal persistence layout. The worker/task-packet handoff itself is refined separately by the worker-boundary slice; this control surface is the stable operator/skill interface around that lifecycle.
