# Git conventions

## GIT-001 — Every agent run has an explicit baseline

- Define the source-of-truth starting point; do not assume a local or remote ref is current.

## GIT-002 — Separate implementation from publishing

- Implementation produces candidate changes; integration, pushing, merging, and publishing are separate steps.

## GIT-003 — Use tiered local hooks without duplicating validation logic

- Pre-commit runs only very fast deterministic checks such as format checks, linting, schema/config validation, forbidden-pattern checks, and secret scanning.
- Pre-push runs broader affected-scope checks such as typechecking and focused unit/integration tests.
- Full-repository, E2E, benchmark, compatibility, and release verification remain explicit broader tiers rather than making every commit expensive.
- Hooks invoke the same canonical repository capabilities used by humans, agents, and CI; do not implement a second copy of validation logic inside hook scripts.

## GIT-004 — Pin CI action dependencies immutably

- Verification, security, release, and deployment workflows pin external GitHub Actions or reusable workflows to a full commit SHA rather than a mutable version tag.
- Keep a human-readable version comment where useful so reviewers can see the intended release without sacrificing immutability.
- Local actions such as `./.github/actions/...` are repository source and do not need an external SHA pin.

## GIT-005 — Bind integration evidence to the exact candidate

- Required acceptance evidence identifies the exact source revision, and the exact built artifact when artifact identity matters.
- Moving the candidate head invalidates earlier completion evidence unless the check is explicitly content-addressed and proves it still applies to the new candidate.
- A skipped, cancelled, missing, timed-out, unavailable, or incomparable check is not green. Classify it explicitly as not applicable or unavailable and preserve why.
- Integration must verify that the evidence being used belongs to the candidate being integrated rather than a previous head, base revision, or neighboring stacked change.
