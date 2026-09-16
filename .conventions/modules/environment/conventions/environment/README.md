# Environment conventions

## ENV-001 — Keep irreplaceable development state outside disposable containers

- Containers provide reproducible execution, not source, Git, credentials, worktrees, or agent-session state.

## ENV-002 — Use one canonical local service topology when services are required

- When a repository requires multiple local services or a reproducible service topology, define one canonical Docker Compose topology and reuse it across development and tests.
- Express differences with configuration, profiles, or explicit overrides rather than maintaining competing service definitions.
- Libraries, CLIs, browser-only applications, and self-contained tests do not need Compose merely for uniformity; unit tests need no external topology.

## ENV-003 — .env.example is the committed environment contract

- Keep .env local and uncommitted; commit a secret-free .env.example covering supported setup.
- Update .env.example whenever an environment variable changes.

## ENV-004 — Fingerprint semantic environment identity and verify it before interpreting failures

- Derive an expected environment fingerprint deterministically from repository-owned semantic inputs rather than from raw machine state. Initial layers should distinguish toolchains, native capabilities, locked dependencies, source-development mode, and environment configuration so a changed layer is diagnosable without treating the combined digest as another source of truth.
- Parse native declarations and configuration into canonical semantic values before hashing. Reformatting, comments, checkout paths, host names, users, timestamps, cache contents, and other irrelevant ambient state must not change the fingerprint; lockfiles may use content digests when their bytes are the deterministic dependency identity.
- An observed machine does not obtain an identity by hashing its filesystem. Verification measures declared requirements and emits a machine-readable receipt; only a passing receipt may claim that the observed environment satisfies the expected fingerprint. Different operating-system/container implementations may therefore satisfy the same repository environment contract.
- Verification happens before build/test failures are classified as repository or implementation regressions. Report expected values, observations, unavailable prerequisites, and blocking mismatches rather than exposing only opaque hashes.
- Never include secret values or hashes of secret values in fingerprints or receipts. A contract may record that a named secret is required and verification may report presence/absence without reading it into evidence.
- Source-development modes that deliberately change exact source revisions produce distinct fingerprint profiles; local source mode must not silently redefine the default Codex/CI identity.
- Fingerprints and receipts are derived, disposable evidence. Do not commit a second hand-maintained version manifest merely to store them, and do not let a matching cache key replace fresh verification of the restored environment.
