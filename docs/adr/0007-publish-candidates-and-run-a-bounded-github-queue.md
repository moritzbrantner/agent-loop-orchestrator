# ADR 0007: Publish candidates and run a bounded GitHub queue

- Status: Accepted
- Date: 2026-08-29

## Context

The orchestrator already owns durable Work Items, isolated Runs, exact Candidate identity, deterministic checks, explicit decisions, and repository configuration. It lacked the separate publication primitives and scheduling behavior needed to continuously turn agent-ready issues into pull requests and remediate existing pull requests without granting a Provider remote authority.

The earlier remote reconciler also invoked GitHub's merge command itself. That duplicated the local verification and race checks already owned by `coding-tooling pr integrate`.

## Decision

Implement two modules with small interfaces:

1. Pull-request publication pushes an exact checked Candidate to a new branch without force, creates and verifies a ready non-draft pull request, and records its URL. Repair publication pushes a descendant Candidate to the same PR branch only with an exact `--force-with-lease` against the previously observed head.
2. The Queue Runner repeatedly refreshes an orchestrator-owned checkout, delegates PR integration to `coding-tooling pr integrate`, creates bounded repair Work Items for repairable results, and otherwise selects the next unblocked `ready-for-agent` issue for the standard full-check implementation procedure.

Providers retain `may_publish = false` and never merge. The orchestrator never calls `gh pr merge`; coding-tooling remains the sole merge path. A moved PR head refreshes the queue rather than overwriting remote work.

Repair attempts default to two per PR. Queue invocations default to at most twenty processed items. The runner remembers its own published PRs; any other existing PR requires an explicitly trusted author before local execution. Pending reviews/checks, contributor-fork repairs, issue metadata gaps, architecture decisions, exhausted retries, and other external blockers stop or are reported without inventing authority.

## Consequences

Publication knowledge is local to one module, while scheduling is testable through the Queue Runner interface. The developer checkout remains untouched. Runtime state survives process restarts and binds repair counts to observed PR heads.

Repositories must use the standard `ready-for-agent`, `agent-loop:active`, `agent-loop:blocked`, and `agent-loop:ready-to-merge` labels. Agent-ready slice issues must declare `scope` and optional `blocked_by` lists in YAML frontmatter.
