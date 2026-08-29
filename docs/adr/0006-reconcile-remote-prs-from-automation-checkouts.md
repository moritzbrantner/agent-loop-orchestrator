# ADR 0006: Reconcile remote pull requests from automation checkouts

- Status: Superseded by ADR 0007
- Date: 2026-08-27

## Context

The local execution lifecycle deliberately binds work to an exact commit, runs providers in isolated worktrees, retains an immutable candidate, and separates candidate production from integration. Remote pull requests need the same guarantees while a developer may have dirty or divergent local branches. The current LAN dashboard is not an Internet-facing webhook receiver, and a provider must not receive GitHub publication authority.

## Decision

Add an opt-in Remote Pull Request Loop with one external interface: reconcile all open pull requests for one explicitly configured GitHub repository.

- Poll GitHub through a remote-host seam instead of exposing the LAN server to public webhooks.
- Key decisions and repair limits to the repository, pull-request number, and exact observed head SHA.
- Require an explicit trusted-author allowlist before any merge or repair mutation is enabled.
- Merge only a non-draft pull request whose checks passed and whose GitHub merge state is `CLEAN`, using GitHub's exact-head guard.
- Run failed-check and merge-conflict repairs in an orchestrator-owned checkout below the per-user data directory, never in the developer checkout.
- Keep provider publication authority denied. After deterministic checks pass, let the orchestrator push the exact candidate to a same-repository pull-request branch with an exact `--force-with-lease`.
- Defer contributor-fork publication, review/policy blockers, untrusted authors, and exhausted repair limits to a human. Do not let an agent close or reject a remote pull request.
- Serialize repair agents through the existing execution lease. Waiting for GitHub checks does not consume that lease.

## Consequences

Local development can continue without branch switching, cleaning, resetting, or integration by the remote loop. Duplicate polling events and process restarts are safe because remote state is durable and every mutation rechecks the head SHA.

GitHub CLI authentication and branch protection remain deployment prerequisites. Local deterministic checks execute repository code, so the trusted-author allowlist is also an execution trust decision. Cross-repository pull requests may still be merged when green and policy-eligible, but automatic repair cannot assume permission to push into the contributor's repository.
