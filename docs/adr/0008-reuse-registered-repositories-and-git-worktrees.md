# ADR 0008: Reuse registered repositories and ordinary Git worktrees

- Status: Accepted
- Date: 2026-08-30
- Supersedes: the workspace/checkout decision in ADR 0007

## Context

ADR 0007 introduced a separate queue automation checkout below the Agent Loop data directory so queue work would not operate in the registered repository checkout. That duplicated repository-local generated state even though the repository was already available locally. In dogfooding, the queue checkout for `nlp-stack` accumulated roughly 68 GiB in its own `target/` directory.

Follow-up changes proposed an Agent Loop-specific shared Cargo target and generated-data garbage collection. Those mechanisms compensate for the duplicate workspace instead of removing it.

## Decision

The Queue Runner uses the registered repository checkout for queue-level Git operations, `coding-tooling` pull-request integration, and guarded pull-request publication. The registered checkout must be clean before queue processing begins.

Issue implementations and pull-request repairs continue to use the Execution Service's ordinary detached Git worktrees. These are standard Git worktrees backed by the registered repository's common Git object store; Agent Loop does not create a second full repository clone below its data directory.

Agent Loop does not introduce a repository-keyed build-cache directory, override ordinary toolchain cache policy, or require a generated-data garbage collector to compensate for duplicate checkouts. Repository and toolchain caches remain owned by their normal tools and locations.

Legacy `queue-checkouts` directories are no longer runtime state.

## Consequences

- Pull-request integration and its checks reuse the registered repository's normal ignored build and dependency state.
- Issue and repair isolation remains standard Git worktree behavior without duplicating Git object storage.
- A dirty registered checkout blocks queue processing instead of silently creating another repository copy.
- Existing legacy `queue-checkouts` data can be deleted once no older Agent Loop process is using it.
