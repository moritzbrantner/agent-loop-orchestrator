# Installed conventions

This directory is managed by `coding-tooling conventions`. Do not edit these snapshots directly.
Repository-specific rules and exceptions belong in `AGENTS.md`.

## Rule briefing

Read this section first. Open the linked managed source when a rule is relevant, ambiguous, or needs its full context.

- **PRINCIPLE-001 — Prefer determinism over inference** — Prefer executable checks, deterministic mappings, explicit baselines, and structured ownership over semantic inference. ([details](modules/base/principles/README.md))
- **PRINCIPLE-002 — Structure should encode agent-relevant information** — Use paths, hierarchy, names, and local instructions to communicate scope, ownership, relevance, and dependencies. ([details](modules/base/principles/README.md))
- **PRINCIPLE-003 — Validate progressively** — Run the narrowest, cheapest affected checks first; expand only after they pass. ([details](modules/base/principles/README.md))
- **PRINCIPLE-004 — Make completion observable** — Completion is defined by repository-owned, independently repeatable gates—not agent confidence. ([details](modules/base/principles/README.md))
- **PRINCIPLE-005 — Document decisions, not defaults** — Document consequential choices agents cannot reliably infer. ([details](modules/base/principles/README.md))
- **PRINCIPLE-006 — Escalate complexity only when the workload requires it** — Treat direct human-to-agent work as a first-class execution mode. ([details](modules/base/principles/README.md))
- **PRINCIPLE-007 — Keep capabilities replaceable; internalize with evidence** — External libraries, services, processes, and hosted infrastructure are valid bootstrap implementations; avoid unnecessary domain coupling to a particular implementation. ([details](modules/base/principles/README.md))
- **PRINCIPLE-008 — Compute and validate once; reuse the trusted representation** — At a semantic or trust boundary, parse, normalize, probe, or otherwise compute the required representation once, validate it there, and pass the resulting trusted typed object downstream. ([details](modules/base/principles/README.md))
- **AGENT-003 — Separate execution from orchestration** — Keep the development loop independent of its local, CI, or hosted orchestration adapter. ([details](modules/base/conventions/agents/README.md))
- **AGENT-004 — The harness defines completion** — The harness owns the completion gates; agents propose and repair changes. ([details](modules/base/conventions/agents/README.md))
- **AGENT-005 — Integration is its own workspace** — Combine and validate independently produced changes in a dedicated integration workspace. ([details](modules/base/conventions/agents/README.md))
- **AGENT-006 — Prefer mechanical discovery before semantic search** — Derive relationships from paths, names, metadata, or indexes before searching semantically. ([details](modules/base/conventions/agents/README.md))
- **AGENT-009 — Delegate one bounded capability per implementation run** — Give each delegated implementation run one independently verifiable capability slice. ([details](modules/base/conventions/agents/README.md))
- **AGENT-010 — Apply progressive composition to agent execution** — Resolve execution-layer choices to `PRINCIPLE-006` and progressive verification to `PRINCIPLE-003`; AGENT-010 is the agent-category pointer and adds no second copy of those policies. ([details](modules/base/conventions/agents/README.md))
- **AGENT-011 — Classify failures before repairing them** — Classify a failing signal before changing production behavior: candidate regression, pre-existing repository defect, repository-owned test or harness defect, environment or infrastructure failure, or unavailable/incomparable evidence. ([details](modules/base/conventions/agents/README.md))
- **DESIGN-001 — Prefer deep modules over pass-through layers** — Prefer a small, stable interface that hides meaningful behavior. ([details](modules/base/conventions/codebase-design/README.md))
- **DESIGN-002 — Treat seam placement as a design decision** — Introduce a seam where behavior actually varies or where a stable public testing/calling surface is valuable. ([details](modules/base/conventions/codebase-design/README.md))
- **DESIGN-003 — Make the interface the natural verification surface** — Design modules so callers and tests can exercise important behavior through the same stable interface. ([details](modules/base/conventions/codebase-design/README.md))
- **DESIGN-004 — Optimize for locality and leverage, not line-count ratios** — Judge depth by what callers gain and what maintainers can change locally, not by implementation-lines divided by interface-lines. ([details](modules/base/conventions/codebase-design/README.md))
- **DESIGN-005 — Resolve contradictory structural rules at the correct level** — A narrower module must not silently contradict a broader architectural truth. ([details](modules/base/conventions/codebase-design/README.md))
- **DESIGN-006 — Give each semantic decision one authoritative owner** — A domain fact, state transition, invariant, algorithmic decision, or canonical representation has one authoritative owner at a time. ([details](modules/base/conventions/codebase-design/README.md))
- **REPO-001 — Repository structure encodes agent-relevant relationships** — Prefer layouts whose relationships are mechanically derivable from paths, hierarchy, naming, or local metadata. ([details](modules/base/conventions/repository/README.md))
- **REPO-002 — More specific conventions override broader conventions** — On conflict, use the narrowest applicable rule; non-conflicting broader rules remain in force. ([details](modules/base/conventions/repository/README.md))
- **REPO-009 — Use conventional roots for durable agent-authored project knowledge** — `CONTEXT.md` for the concise domain glossary and project-level domain overview; ([details](modules/base/conventions/repository/README.md))
- **REPO-010 — Keep TODOs machine-discoverable and actionable** — Developer-authored TODOs are legitimate follow-up work that agents may later enumerate, implement, or turn into issues. ([details](modules/base/conventions/repository/README.md))
- **REPO-011 — Verify only meaningful file permissions** — Directly executable scripts and tools have the executable bit when the target platform uses it; ordinary source/config files should not accidentally become executable. ([details](modules/base/conventions/repository/README.md))
- **REPO-012 — Treat symlinks as explicit filesystem boundaries** — Deterministic traversal does not recursively follow symlinks by default. ([details](modules/base/conventions/repository/README.md))
- **REPO-013 — Keep path casing portable** — Imports and references use the exact on-disk path casing. ([details](modules/base/conventions/repository/README.md))
- **REPO-014 — Public agent tools expose a stable machine discovery document** — A public tool intentionally designed for coding-agent consumption exposes a versioned machine discovery document at a stable published URL, preferably `agent-tool.json` at the tool's Pages root. ([details](modules/base/conventions/repository/README.md))
- **REPO-015 — Prefer vertical growth before creating another repository** — Before creating a new repository or lab, identify the capability, ownership boundary, runtime constraint, or validation need that cannot be expressed coherently in an existing repository. ([details](modules/base/conventions/repository/README.md))
- **REPO-016 — Public repositories provide a useful GitHub Pages surface** — Public repositories should expose a maintained GitHub Pages site unless the repository records an explicit reason to opt out. ([details](modules/base/conventions/repository/README.md))
- **REPO-017 — Public claims must not exceed verified capability** — README text, Pages surfaces, generated reports, release notes, PR descriptions, benchmark summaries, and demos must distinguish verified capability from experimental, advisory, simulated, or planned behavior when the distinction matters. ([details](modules/base/conventions/repository/README.md))
- **REP-001 — Green verification introduces no new warnings** — New and cleaned repositories should treat compiler and linter warnings as failures. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-002 — Mechanize stable preferences before instructing agents** — Prefer ecosystem-native compiler options, formatters, linters, analyzers, schemas, tests, and configuration over prose instructions for mechanically decidable policy. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-003 — Generated outputs are disposable local state by default** — Do not commit generated artifacts when source inputs plus a deterministic generator can reproduce them. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-004 — Observable output has stable ordering** — Define deterministic ordering for values that are serialized, persisted, hashed, snapshotted, compared, or emitted through user- or machine-visible interfaces. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-005 — Control time, randomness, and generated identities where behavior depends on them** — Put clocks, randomness, and ID generation behind a controllable seam only when behavior depends on them. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-006 — Keep machine time and locale semantics explicit** — Represent instants and machine-readable timestamps in UTC unless the domain explicitly requires another zone. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-007 — Compare floating-point results by their semantic contract** — Exact comparison is appropriate when exactness is part of the contract or a value is merely carried through unchanged. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-008 — Pin toolchains exactly and keep canonical baselines current** — Repositories pin exact versions of the toolchains that participate in build and verification using the ecosystem's normal native mechanism. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-009 — Unused implementation code is not part of a green baseline** — Unused private code, imports, variables, parameters, and unreachable implementation paths should fail deterministic verification where the language can detect them. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-010 — Handle closed variants exhaustively and unknown external values explicitly** — Closed enums, variants, and discriminated unions owned by the application should be handled exhaustively so adding a new case creates a deterministic failure at affected callers. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-011 — Keep repository text portable** — Source, configuration, documentation, scripts, fixtures, and machine-readable text use UTF-8 and LF by default. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-012 — Separate verification from mutation** — Commands used as gates such as `check`, `verify`, format-check, lint, tests, CI, and release verification do not modify tracked source, lockfiles, generated outputs, or configuration. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-013 — Use semantic paths internally and canonical paths at observable boundaries** — Use the language/runtime path APIs rather than manual path-string concatenation. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-014 — Keep disposable output in declared disposable locations** — Temporary files use OS temporary storage or an explicit ignored workspace directory. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-015 — Treat caches as transparent optimizations** — Correctness must not depend on a warm cache; important verification can succeed from a cold cache. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-016 — Separate dependency acquisition from deterministic build work** — Restore/install/fetch phases may use the network to acquire declared dependencies and external inputs. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-017 — Control ambient environment at deterministic boundaries** — Ordinary interactive development may inherit the user's shell environment. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-018 — Keep build artifacts reproducible from declared inputs** — Artifact contents do not accidentally depend on wall-clock time, host name, user name, absolute checkout path, random build IDs, or other ambient machine state. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-019 — Validate configuration against real contracts** — Use official or ecosystem-native schemas and validators when a configuration format has an established contract. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-020 — Normalize Unicode where byte identity represents textual identity** — Use an explicit Unicode normalization policy for identifiers, generated filenames, canonical serialized forms, hashes, search/deduplication keys, or cross-system equality when canonically equivalent text must compare the same. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-021 — Reconcile deterministic mutations instead of repeating work** — A deterministic mutating operation inspects current state, derives the desired state, compares them, applies only the required delta, and verifies the resulting state. ([details](modules/base/conventions/reproducibility/README.md))
- **REP-022 — Deterministic systems replay from declared inputs** — When deterministic replay is part of a system's contract, the replay identity includes the authoritative initial state, ordered external inputs or events, seed or random stream, relevant configuration, and implementation/content identity needed to interpret them. ([details](modules/base/conventions/reproducibility/README.md))
- **REL-001 — Handle errors specifically inside the system and broadly only at deliberate boundaries** — Internal code handles expected failures it can meaningfully recover from or translate; otherwise propagate them. ([details](modules/base/conventions/reliability/README.md))
- **REL-002 — Bound external operations** — Network calls, database operations, queues, RPC, subprocesses, and similar cross-process operations use an explicit finite timeout or cancellation bound. ([details](modules/base/conventions/reliability/README.md))
- **REL-003 — Retry only failures classified as transient and safe to repeat** — Retries are bounded, use a defined backoff strategy, and continue to respect timeout and cancellation. ([details](modules/base/conventions/reliability/README.md))
- **REL-004 — Give scarce resources an obvious owner and deterministic cleanup** — Files, sockets, database connections, subprocesses, locks, streams, subscriptions, GPU/audio handles, and similar resources must have an obvious lifetime owner. ([details](modules/base/conventions/reliability/README.md))
- **REL-005 — Propagate cancellation from lifecycle boundaries through blocking work** — Requests, jobs, CLI operations, workers, database calls, network I/O, and subprocesses propagate cancellation when their surrounding lifecycle supports it. ([details](modules/base/conventions/reliability/README.md))
- **REL-006 — Detached asynchronous work requires explicit ownership** — Async work is awaited or returned by default. ([details](modules/base/conventions/reliability/README.md))
- **REL-007 — Make shared mutable state and synchronization explicit** — Prefer clear ownership, immutable transfer, or message passing when they keep concurrency simpler. ([details](modules/base/conventions/reliability/README.md))
- **REL-008 — Use structured logging where logs are operational data** — Services and applications use stable event names and structured fields for identifiers, durations, statuses, error categories, and other queryable values. ([details](modules/base/conventions/reliability/README.md))
- **REL-009 — Make public compatibility an executable concern** — Versioned/public APIs, schemas, persisted formats, CLI contracts, package exports, and protocol messages should have deterministic compatibility checks where established tooling can provide them. ([details](modules/base/conventions/reliability/README.md))
- **REL-010 — Give CLIs stable exit and stream semantics** — Successful commands return exit code `0`; failures return a non-zero exit code. ([details](modules/base/conventions/reliability/README.md))
- **REL-011 — Replace durable machine-managed state atomically** — State/config manifests, generated indexes, cache metadata, downloaded artifacts, and similar machine-managed files use temporary output plus validation and atomic replacement where the filesystem supports it. ([details](modules/base/conventions/reliability/README.md))
- **TEST-001 — Test location follows dependency scope** — Place a test at the lowest source-tree directory containing all production code it covers. ([details](modules/base/conventions/testing/README.md))
- **TEST-003 — Keep test scope separate from test kind** — Use location for coverage scope and independent names or metadata for execution kind. ([details](modules/base/conventions/testing/README.md))
- **TEST-004 — Test authorization as a decision matrix** — Cover relevant authentication, role, relationship, and context combinations, including denial cases. ([details](modules/base/conventions/testing/README.md))
- **TEST-005 — Behavior changes require executable evidence** — Add or update the smallest automated test that would fail without a behavior change or bug fix. ([details](modules/base/conventions/testing/README.md))
- **TEST-006 — Prefer stable public behavior seams** — Test through the highest practical stable interface that exercises the real behavior. ([details](modules/base/conventions/testing/README.md))
- **TEST-007 — Infer testing strategy from the repository before inventing one** — Reuse the repository's established test layers, commands, fixtures, and public seams. ([details](modules/base/conventions/testing/README.md))
- **TEST-008 — Keep behavior change and structural cleanup distinct** — For approved behavior changes, establish the failing evidence before the production change and return it to green. ([details](modules/base/conventions/testing/README.md))
- **TEST-009 — Keep the deterministic test gate hermetic** — Unit and ordinary integration tests must not depend on the public internet or an uncontrolled external service. ([details](modules/base/conventions/testing/README.md))
- **TEST-010 — Build minimal deterministic fixtures** — Tests create only the state they need through small reusable builders or factories rather than a giant shared mutable seed database. ([details](modules/base/conventions/testing/README.md))
- **TEST-011 — Use snapshots only for stable structured output** — Prefer explicit semantic assertions for ordinary business behavior. ([details](modules/base/conventions/testing/README.md))
- **TEST-012 — Do not retry a deterministic gate until it turns green** — A required deterministic test failing once is a failed gate. ([details](modules/base/conventions/testing/README.md))
- **TEST-013 — Prefer risk-based differential coverage over a global percentage target** — Changed behavior requires executable evidence; use changed-line or changed-branch coverage where tooling can measure it reliably. ([details](modules/base/conventions/testing/README.md))
- **TEST-014 — Tests do not depend on execution order** — Tests must be runnable individually and must not rely on state left by another test. ([details](modules/base/conventions/testing/README.md))
- **TEST-015 — Run expensive correctness analyzers in risk-appropriate tiers** — Keep the fast development gate focused on normal compiler, linter, and test evidence. ([details](modules/base/conventions/testing/README.md))
- **TEST-016 — Allocate dynamic ports for disposable processes** — Temporary test servers and parallel disposable services ask the OS for an available port rather than assuming a fixed port is free. ([details](modules/base/conventions/testing/README.md))
- **TEST-017 — Choose database-test isolation by the semantics under test** — Transaction rollback is preferred when it faithfully represents behavior and provides cheap isolation. ([details](modules/base/conventions/testing/README.md))
- **TEST-019 — Verify browser-specific risk at the browser boundary** — Use real-browser verification when the changed behavior materially depends on browser semantics such as layout, scrolling, pointer geometry, media, browser APIs, or navigation that a lower layer cannot prove reliably. ([details](modules/base/conventions/testing/README.md))
- **TEST-020 — Prefer semantic browser targets** — Browser automation targets stable user-facing semantics such as roles with accessible names, labels, and visible text before implementation-shaped selectors. ([details](modules/base/conventions/testing/README.md))
- **TEST-021 — Keep browser test doubles protocol-faithful** — A browser failure that crosses a mocked or stubbed network boundary is not sufficient evidence of a product defect until the double preserves the production protocol semantics that materially affect the behavior. ([details](modules/base/conventions/testing/README.md))
- **SCRIPTS-001 — Scripts are idempotent and self-verifying by default** — Canonical setup, bootstrap, generation, cache-population, installation, and maintenance scripts are safe to run repeatedly and converge on the same valid state. ([details](modules/base/conventions/scripts/README.md))
- **SCRIPTS-002 — Use language-appropriate shell strictness** — Bash scripts use `set -euo pipefail` and pass ShellCheck by default. ([details](modules/base/conventions/scripts/README.md))
- **SCRIPTS-003 — Guard destructive filesystem operations** — Resolve and validate deletion/overwrite targets before destructive mutation. ([details](modules/base/conventions/scripts/README.md))
- **SEC-001 — Scan secrets in risk-appropriate layers** — Pre-commit or equivalent fast checks scan staged/changed content for high-confidence secrets. ([details](modules/base/conventions/security/README.md))
- **SEC-002 — Audit dependency vulnerabilities and licenses by risk** — Security/full tiers use ecosystem-native vulnerability and license audits where practical. ([details](modules/base/conventions/security/README.md))
- **SEC-003 — Validate data when it crosses a trust boundary** — Validate HTTP/API input, CLI input, files, messages, external-service responses, deserialized records, and other untrusted data when it enters the trusted domain. ([details](modules/base/conventions/security/README.md))
- **SEC-004 — Verify downloaded executable/build inputs** — External tools, binaries, archives, and artifacts used in deterministic setup, build, or release paths are version-pinned and verified with a cryptographic checksum, signature, immutable release identity, or equivalent ecosystem-native integrity mechanism. ([details](modules/base/conventions/security/README.md))
- **SEC-005 — Treat reusable browser state and captured sessions as sensitive** — Cookies, storage state, session tokens, authenticated profiles, HARs, traces, screenshots, and videos may contain credentials, personal data, or other sensitive information. ([details](modules/base/conventions/security/README.md))
- **DEP-001 — Keep publication out of ordinary development** — Develop cross-repository changes against source revisions rather than publishing packages to unblock feature work. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-002 — Version bumps belong to release work** — Keep package versions compatible during source-development work when possible. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-003 — Bound cross-repository task expansion by ownership** — A normal implementation task may cross repository boundaries when each changed repository owns a necessary part of the same capability or contract. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-004 — Require a reason for a new independently versioned package** — Add functionality to an existing coherent package by default. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-005 — Separate development proof from release proof** — Source-mode checks prove that the working source graph is correct. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-006 — Publish frontend packages only for real external consumers** — Keep application-local JavaScript or TypeScript packages source-local. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-007 — Keep private source graphs local to the coding workspace** — For private cross-repository dependencies, prefer exact sibling repositories or worktrees owned by the outer coding workspace rather than authenticated Git fallback inside the dependency resolver. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-008 — Keep repository dependencies directional** — Put broadly reusable contracts and primitives below the domain repositories that consume them. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-009 — Depend on capability surfaces, not upstream topology** — Consume the smallest stable public surface that represents the required capability. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-010 — Give every versioned package one canonical owner** — A versioned package or crate must have one canonical repository responsible for source changes, compatibility, tests, and releases. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-011 — Treat source overrides as development mechanics** — Exact source overrides may substitute unpublished revisions during cross-repository development, but they must preserve the intended public dependency direction. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-012 — Declare dependency versions according to the consumer contract** — Applications, internal tooling, and other leaf deliverables prefer exact dependency versions for predictable resolution. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-013 — Make dependency changes explicit transactions** — Ordinary install and verification commands use the committed lockfile without modifying it. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-014 — Keep production dependency graphs acyclic by default** — Production package, module, project, and repository dependency graphs should be acyclic where the ecosystem can model and verify the relationship. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-015 — Centralize automated dependency update policy** — Use Renovate as the canonical routine dependency-update engine for repositories adopting these conventions. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-016 — Choose distribution by source ownership** — When upstream remains responsible for implementation changes, compatibility, fixes, and releases, consume the capability through the ecosystem's normal package or dependency mechanism. ([details](modules/dependencies/conventions/dependencies/README.md))
- **DEP-017 — Prove dependency ranges as a clean consumer** — Treat the repository's locked development graph, the minimum compatibility point declared by peer ranges, and a fresh registry resolution as three different pieces of evidence. ([details](modules/dependencies/conventions/dependencies/README.md))
- **ENV-001 — Keep irreplaceable development state outside disposable containers** — Containers provide reproducible execution, not source, Git, credentials, worktrees, or agent-session state. ([details](modules/environment/conventions/environment/README.md))
- **ENV-002 — Use one canonical local service topology when services are required** — When a repository requires multiple local services or a reproducible service topology, define one canonical Docker Compose topology and reuse it across development and tests. ([details](modules/environment/conventions/environment/README.md))
- **ENV-003 — .env.example is the committed environment contract** — Keep .env local and uncommitted; commit a secret-free .env.example covering supported setup. ([details](modules/environment/conventions/environment/README.md))
- **ENV-004 — Fingerprint semantic environment identity and verify it before interpreting failures** — Derive an expected environment fingerprint deterministically from repository-owned semantic inputs rather than from raw machine state. Initial layers should distinguish toolchains, native capabilities, locked dependencies, source-development mode, and environment configuration so a changed layer is diagnosable without treating the combined digest as another source of truth. ([details](modules/environment/conventions/environment/README.md))
- **GIT-001 — Every agent run has an explicit baseline** — Define the source-of-truth starting point; do not assume a local or remote ref is current. ([details](modules/git/conventions/git/README.md))
- **GIT-002 — Separate implementation from publishing** — Implementation produces candidate changes; integration, pushing, merging, and publishing are separate steps. ([details](modules/git/conventions/git/README.md))
- **GIT-003 — Use tiered local hooks without duplicating validation logic** — Pre-commit runs only very fast deterministic checks such as format checks, linting, schema/config validation, forbidden-pattern checks, and secret scanning. ([details](modules/git/conventions/git/README.md))
- **GIT-004 — Pin CI action dependencies immutably** — Verification, security, release, and deployment workflows pin external GitHub Actions or reusable workflows to a full commit SHA rather than a mutable version tag. ([details](modules/git/conventions/git/README.md))
- **GIT-005 — Bind integration evidence to the exact candidate** — Required acceptance evidence identifies the exact source revision, and the exact built artifact when artifact identity matters. ([details](modules/git/conventions/git/README.md))
- **TS-002 — Model invalid states out of the type system** — Prefer types, especially discriminated unions, that make invalid combinations unrepresentable. ([details](modules/typescript/technologies/typescript/README.md))
- **TS-003 — Prefer type over interface** — Use type aliases instead of interfaces for TypeScript models and declarations. ([details](modules/typescript/technologies/typescript/README.md))
- **TS-004 — Use strict compiler options that expose missing-state mistakes** — Enable `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noImplicitOverride`, and `noFallthroughCasesInSwitch` by default. ([details](modules/typescript/technologies/typescript/README.md))
- **TS-005 — Enforce exhaustive owned variants in TypeScript** — TypeScript code applies `REP-010` to application-owned enums and discriminated unions. ([details](modules/typescript/technologies/typescript/README.md))
- **TS-006 — Enforce owned asynchronous work in TypeScript** — TypeScript code applies `REL-006`: promises are observed unless an explicit abstraction owns detached work. ([details](modules/typescript/technologies/typescript/README.md))
- **TS-007 — Use structurally explicit control flow and side effects** — Require braces around control-flow bodies. ([details](modules/typescript/technologies/typescript/README.md))
- **TS-008 — Keep imports mechanically canonical** — Combine duplicate compatible imports from the same resolved module; namespace imports may remain separate when syntax requires it. ([details](modules/typescript/technologies/typescript/README.md))
- **REACT-001 — Colocate components and directly related artifacts** — Keep a component and its focused tests, styles, hooks, and types in their smallest shared directory. ([details](modules/react/technologies/typescript/react/README.md))
- **REACT-002 — Keep React state local by default** — Own state in the smallest subtree that needs it; widen only for real shared ownership. ([details](modules/react/technologies/typescript/react/README.md))
- **REACT-003 — Put important navigational state in URL query parameters** — Put durable, shareable view state in query parameters; keep ephemeral and sensitive state out of URLs. ([details](modules/react/technologies/typescript/react/README.md))
- **REACT-004 — Use effects for external synchronization** — Use effects for systems outside React, not derived values or ordinary control flow. ([details](modules/react/technologies/typescript/react/README.md))
- **REACT-005 — Prefer composition over highly configurable mega-components** — Give a reusable component one coherent responsibility and compose larger experiences from smaller parts. ([details](modules/react/technologies/typescript/react/README.md))
- **REACT-007 — Reuse shared UI before creating local primitives** — Inspect and reuse the established UI package before creating local primitives. ([details](modules/react/technologies/typescript/react/README.md))
- **REACT-008 — Separate update domains by frequency** — Keep fast-changing state in the smallest component, subscription, or renderer boundary that actually consumes it; do not make broad parents or providers rerender at animation-frame, pointer-hover, streaming, or simulation-tick cadence without a concrete need. ([details](modules/react/technologies/typescript/react/README.md))
- **REACT-009 — Avoid redundant state and synchronization** — Derive values from current props, state, URL state, or authoritative external data when practical instead of copying them into additional React state. ([details](modules/react/technologies/typescript/react/README.md))
- **REACT-010 — Optimize boundaries before memoization** — Reduce work first through ownership boundaries, focused subscriptions, virtualization, and data shaping. Do not blanket a component tree with `memo`, `useMemo`, or `useCallback`. ([details](modules/react/technologies/typescript/react/README.md))
- **REACT-011 — Build component systems in layers** — Prefer a component vocabulary that composes upward: low-level primitives and behavior, reusable domain building blocks, feature-level compositions, then pages or screens. ([details](modules/react/technologies/typescript/react/README.md))
- **REACT-012 — Expose explicit composition seams** — Make optional regions explicit composition points when consumers reasonably need to replace, omit, or reorder them. Prefer named subcomponents, slots, children, or small compound-component APIs over a growing collection of `showX`, `hideY`, `variant`, and `mode` props. ([details](modules/react/technologies/typescript/react/README.md))
- **REACT-013 — Extract stable reuse, not speculative universality** — Keep a domain-specific component local while its semantics are still changing. Do not generalize a one-off component only because it is large. ([details](modules/react/technologies/typescript/react/README.md))
- **RUST-001 — Encode invariants in types** — Prefer types that make invalid domain states unrepresentable. ([details](modules/rust/technologies/rust/README.md))
- **RUST-002 — Avoid `unwrap` and `expect` in normal production control flow** — Handle or propagate recoverable errors explicitly. ([details](modules/rust/technologies/rust/README.md))
- **RUST-003 — Do not clone merely to satisfy the borrow checker** — Treat unnecessary cloning as a signal to inspect ownership boundaries first. ([details](modules/rust/technologies/rust/README.md))
- **BUN-001 — Use Bun as the default JavaScript toolchain** — Use Bun for packages, scripts, and JavaScript/TypeScript where required tooling supports it. ([details](modules/tooling/technologies/tooling/README.md))
- **TAILWIND-001 — Prefer Tailwind CSS when practical** — Prefer Tailwind for application styling when utility classes preserve ownership near the markup. ([details](modules/tooling/technologies/tooling/README.md))
- **TAILWIND-002 — Use semantic tokens and named variants** — Use semantic tokens and named variants for visual decisions. ([details](modules/tooling/technologies/tooling/README.md))

## Installed modules

### base

- [modules/base/principles/README.md](modules/base/principles/README.md)
- [modules/base/docs/adr/0001-capability-internalization.md](modules/base/docs/adr/0001-capability-internalization.md)
- [modules/base/conventions/agents/README.md](modules/base/conventions/agents/README.md)
- [modules/base/conventions/codebase-design/README.md](modules/base/conventions/codebase-design/README.md)
- [modules/base/conventions/repository/README.md](modules/base/conventions/repository/README.md)
- [modules/base/conventions/repository/REPO-010.json](modules/base/conventions/repository/REPO-010.json)
- [modules/base/conventions/repository/REPO-012.json](modules/base/conventions/repository/REPO-012.json)
- [modules/base/conventions/repository/REPO-013.json](modules/base/conventions/repository/REPO-013.json)
- [modules/base/conventions/reproducibility/README.md](modules/base/conventions/reproducibility/README.md)
- [modules/base/conventions/reproducibility/REP-011.json](modules/base/conventions/reproducibility/REP-011.json)
- [modules/base/conventions/reliability/README.md](modules/base/conventions/reliability/README.md)
- [modules/base/conventions/testing/README.md](modules/base/conventions/testing/README.md)
- [modules/base/conventions/scripts/README.md](modules/base/conventions/scripts/README.md)
- [modules/base/conventions/security/README.md](modules/base/conventions/security/README.md)

### dependencies

- [modules/dependencies/conventions/dependencies/README.md](modules/dependencies/conventions/dependencies/README.md)

### environment

- [modules/environment/conventions/environment/README.md](modules/environment/conventions/environment/README.md)
- [modules/environment/conventions/environment/ENV-003.json](modules/environment/conventions/environment/ENV-003.json)

### git

- [modules/git/conventions/git/README.md](modules/git/conventions/git/README.md)
- [modules/git/conventions/git/GIT-004.json](modules/git/conventions/git/GIT-004.json)

### agent-delegation

- [modules/agent-delegation/conventions/agents/AGENT-009-delegate-one-bounded-capability-per-run.md](modules/agent-delegation/conventions/agents/AGENT-009-delegate-one-bounded-capability-per-run.md)
- [modules/agent-delegation/conventions/agents/AGENT-010-do-not-require-higher-level-execution-machinery.md](modules/agent-delegation/conventions/agents/AGENT-010-do-not-require-higher-level-execution-machinery.md)

### typescript

- [modules/typescript/technologies/typescript/README.md](modules/typescript/technologies/typescript/README.md)
- [modules/typescript/technologies/typescript/TS-003.json](modules/typescript/technologies/typescript/TS-003.json)
- [modules/typescript/technologies/typescript/TS-005.json](modules/typescript/technologies/typescript/TS-005.json)
- [modules/typescript/technologies/typescript/TS-006.json](modules/typescript/technologies/typescript/TS-006.json)
- [modules/typescript/technologies/typescript/TS-007.json](modules/typescript/technologies/typescript/TS-007.json)
- [modules/typescript/technologies/typescript/TS-008.json](modules/typescript/technologies/typescript/TS-008.json)

### react

- [modules/react/technologies/typescript/react/README.md](modules/react/technologies/typescript/react/README.md)
- [modules/react/technologies/typescript/react/REACT-004.json](modules/react/technologies/typescript/react/REACT-004.json)

### rust

- [modules/rust/technologies/rust/README.md](modules/rust/technologies/rust/README.md)
- [modules/rust/technologies/rust/RUST-002.json](modules/rust/technologies/rust/RUST-002.json)

### tooling

- [modules/tooling/technologies/tooling/README.md](modules/tooling/technologies/tooling/README.md)
- [modules/tooling/technologies/tooling/BUN-001.json](modules/tooling/technologies/tooling/BUN-001.json)

### vite

- [modules/vite/technologies/tooling/vite/README.md](modules/vite/technologies/tooling/vite/README.md)

## Companion configuration assets

### typescript

- [modules/typescript/technologies/typescript/TS-003.oxlint.json](modules/typescript/technologies/typescript/TS-003.oxlint.json)
- [modules/typescript/technologies/typescript/TS-007.oxlint.json](modules/typescript/technologies/typescript/TS-007.oxlint.json)
- [modules/typescript/technologies/typescript/TS-008.oxlint.json](modules/typescript/technologies/typescript/TS-008.oxlint.json)
- [modules/typescript/technologies/typescript/TS-008.oxfmt.json](modules/typescript/technologies/typescript/TS-008.oxfmt.json)

### react

- [modules/react/technologies/typescript/react/generators/react-component/generator.json](modules/react/technologies/typescript/react/generators/react-component/generator.json)
- [modules/react/technologies/typescript/react/generators/react-component/templates/Component.tsx.tmpl](modules/react/technologies/typescript/react/generators/react-component/templates/Component.tsx.tmpl)
- [modules/react/technologies/typescript/react/generators/react-component/templates/index.ts.tmpl](modules/react/technologies/typescript/react/generators/react-component/templates/index.ts.tmpl)
