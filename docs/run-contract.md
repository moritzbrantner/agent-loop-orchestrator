# Shared Run Contract v1

Status: **Proposed**

The shared run contract is the durable interchange model between the orchestrator, agent-loop workers, deterministic tooling, evaluators, specialist workers, and publication adapters.

## Design principles

1. **Exact inputs are binding.** A run identifies its work item version, Git baseline, convention selection, tooling manifest, agent configuration, and authority.
2. **Authority is declared before execution.** Filesystem, network, tools, secrets, duration, retries, integration, and publication permissions are explicit data rather than prompt-only instructions.
3. **Evidence is immutable and attributable.** Checks and evaluations point to content-addressed evidence and identify the exact candidate they evaluated.
4. **Coordination is adapter-neutral.** Local Postgres, GitHub, Codex, other agents, and future execution providers map onto the same domain model.
5. **State changes are observable.** Run history is append-only at the event boundary even when projections such as the current run row are updated.
6. **Absence is not success.** Missing, skipped, failed, and passed checks are distinct outcomes.
7. **Integration and publication are separate decisions.** Producing a valid candidate never implicitly authorizes merging, pushing, opening a pull request, or publishing a release.

## Aggregate

A `Run` is the aggregate root. It contains or references:

| Entity | Meaning |
| --- | --- |
| Project | Repository identity and local root |
| WorkItem | Requested outcome, dependencies, and declared scope |
| Baseline | Exact repository state from which work starts |
| ConventionSelection | Applicable convention IDs and a digest of their resolved content |
| ToolingManifest | Discovered deterministic capabilities and its digest |
| Agent | Execution adapter, identity, model, and prompt/configuration digest |
| Authority | Enforceable permissions and resource limits |
| Attempt | One execution try, including workspace and outcome |
| Candidate | Commit or patch produced by an attempt |
| CheckResult | Deterministic verification result |
| Evaluation | Semantic or comparative assessment, such as Moonlight |
| Evidence | Content-addressed report, log, diff, trace, or artifact |
| Decision | Human or policy approval, rejection, or requested changes |
| Publication | Explicit local integration, pull request, or release result |

## Lifecycle

The v1 lifecycle is intentionally small:

```text
queued
  -> preparing
  -> running
  -> candidate_ready
  -> evaluating
  -> awaiting_decision
  -> integrating
  -> completed
```

Any active state may transition to `failed` or `cancelled`. A retry creates a new `Attempt`; it does not rewrite an earlier attempt.

Required transition conditions:

| Transition | Required facts |
| --- | --- |
| queued -> preparing | Dependencies complete and write scopes do not conflict |
| preparing -> running | Baseline resolved, workspace created, authority materialized |
| running -> candidate_ready | Worker finished and candidate identity recorded |
| candidate_ready -> evaluating | Candidate is immutable for this evaluation |
| evaluating -> awaiting_decision | Required checks and evaluations have terminal outcomes |
| awaiting_decision -> integrating | Current candidate approved and integration authorized |
| integrating -> completed | Integration/publication result and final evidence recorded |

A failed required check cannot be represented as approval. A candidate change invalidates prior candidate-bound checks, evaluations, and decisions.

## Identity and digests

Identifiers are stable opaque strings. Git state uses a full object ID. Other content uses an algorithm-qualified digest such as `sha256:<hex>`.

At minimum, a run binds:

- `runId`
- `project.id`
- `workItem.id`
- `baseline.gitSha`
- resolved convention digest, when conventions are selected
- tooling manifest digest, when capabilities are discovered
- agent adapter and identity
- authority snapshot
- every candidate Git SHA or patch digest
- every check/evaluation to its candidate
- every decision to its candidate

Mutable labels such as branch names, model aliases, or work-item titles are descriptive and never replace immutable identities.

## Authority contract

Authority is an execution input, not advisory metadata.

| Field | Purpose |
| --- | --- |
| `readRoots` | Filesystem roots visible to the worker |
| `writeRoots` | Filesystem roots the worker may mutate |
| `network.mode` | `none`, `allowlist`, or `unrestricted` |
| `network.allowedDomains` | Required when network mode is `allowlist` |
| `tools` | Tool or capability names available to the worker |
| `secretRefs` | References to configured secrets; never secret values |
| `maxDurationSeconds` | Per-attempt wall-clock limit |
| `maxAttempts` | Maximum attempts for the run |
| `mayIntegrate` | Permission to update a local integration target |
| `mayPublish` | Permission to affect a remote system or release |

The executor must fail closed when it cannot enforce the declared authority.

## Candidate-bound evidence

Every check, evaluation, and decision records the candidate Git SHA or patch digest it concerns. Evidence references include:

- kind
- URI or orchestrator resource identifier
- digest
- optional media type
- creation time

Logs alone do not prove completion. A required check result must also record its semantic capability, outcome, start/end times, exit code when applicable, and evidence references.

## Events

The relational model may maintain current-state projections, but the orchestrator should append domain events for meaningful transitions:

- `work_item.created`
- `run.queued`
- `run.state_changed`
- `workspace.created`
- `attempt.started`
- `tool.invoked`
- `candidate.produced`
- `check.finished`
- `evaluation.finished`
- `decision.recorded`
- `candidate.integrated`
- `candidate.published`
- `attempt.failed`
- `run.cancelled`

Each event contains an event ID, run ID, event type, occurred-at timestamp, actor, payload schema version, and payload. Events are suitable for replay and may be exported as telemetry, but telemetry is not the source of truth.

## Component mappings

### coding-agent-conventions

Supplies selected convention IDs, precedence resolution, and the digest of the resolved convention set. The orchestrator records the selection; it does not interpret prose conventions during scheduling.

### coding-tooling

Supplies the deterministic tooling manifest, affected scope, environment diagnostics, and named check results. The orchestrator schedules and stores results; it does not invent substitute commands for missing capabilities.

### agent-loop-setup

Supplies installable skills, worker procedures, repository initialization, and adapters. GitHub-backed and local-first operation must map to the same run contract.

### Moonlight

Consumes an immutable baseline/candidate pair plus evaluation configuration and returns a structured evaluation with evidence. Moonlight decides comparison semantics; the orchestrator decides when the evaluation is required.

### local-refactor

Acts as a specialist executor. Its protected paths, patch journal, validation results, and candidate identity map onto authority, evidence, checks, and candidate fields.

## Versioning

The top-level `schemaVersion` is required and uses an integer major version.

Backward-compatible additions may extend v1 with optional fields. Renaming fields, changing meaning, changing requiredness, or removing enum values requires a new major version and an explicit migration.

Stored contracts remain readable under the schema version with which they were written.

## Deliberate exclusions

v1 does not standardize:

- database table layout
- HTTP routes
- agent prompts
- transport protocol
- UI representation
- model-provider request formats
- the internal output format of deterministic tools or evaluators

Those are adapters or implementation details unless experience proves that another shared boundary is required.
