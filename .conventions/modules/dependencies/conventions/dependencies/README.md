# Dependency development conventions

## DEP-001 — Keep publication out of ordinary development

- Develop cross-repository changes against source revisions rather than publishing packages to unblock feature work.
- Prefer the repository's declared source-development mechanism and pin exact revisions.
- Do not start a crates.io, npm, or other registry release train unless the task is explicitly a release task.

## DEP-002 — Version bumps belong to release work

- Keep package versions compatible during source-development work when possible.
- Put version bumps, changelogs, tags, registry publication, and registry-only consumer updates in a dedicated release change.
- Do not treat a missing published version as a feature blocker when an exact source dependency can prove the change.

## DEP-003 — Bound cross-repository task expansion by ownership

- A normal implementation task may cross repository boundaries when each changed repository owns a necessary part of the same capability or contract.
- Keep the changed source graph as small as the architecture permits and make each repository's responsibility independently explainable and verifiable.
- If implementation starts recursively pulling in unrelated transitive repositories, release work, or opportunistic cleanup, stop the expansion and treat the newly discovered boundary as separate architecture, migration, or follow-up work.
- Do not impose an arbitrary repository-count limit when a coherent change genuinely spans more owners, and do not use a broad task as permission to repair unrelated dependencies.

## DEP-004 — Require a reason for a new independently versioned package

- Add functionality to an existing coherent package by default.
- Create a new package or crate only when there is a second independent consumer, a hard dependency/isolation boundary, or another concrete reason for independent versioning.
- Conceptual separability alone is not sufficient.

## DEP-005 — Separate development proof from release proof

- Source-mode checks prove that the working source graph is correct.
- Registry-only resolution in a clean checkout is a release/distribution gate, not an inner-loop development gate.
- Before release, remove or deactivate source overrides and prove the published dependency graph independently.

## DEP-006 — Publish frontend packages only for real external consumers

- Keep application-local JavaScript or TypeScript packages source-local.
- Do not publish to npm solely because a workspace package boundary exists.
- Publish only when another independently versioned consumer requires the package or distribution itself is the product.

## DEP-007 — Keep private source graphs local to the coding workspace

- For private cross-repository dependencies, prefer exact sibling repositories or worktrees owned by the outer coding workspace rather than authenticated Git fallback inside the dependency resolver.
- Require each local source checkout to match the exact declared revision before using it; missing or mismatched source must fail explicitly.
- Do not add repository secrets, personal access tokens, or hosted-CI private checkouts merely to reproduce an ordinary multi-repository development workspace.
- Keep hosted CI repository-local when private source access would otherwise be required. Record the exact source revisions and local verification evidence as the implementation proof.
- Authentication may still exist in an explicit release/distribution workflow when that workflow genuinely needs protected release inputs; do not make those credentials a prerequisite for feature development.

## DEP-008 — Keep repository dependencies directional

- Put broadly reusable contracts and primitives below the domain repositories that consume them.
- Domain repositories should depend downward on shared contracts or foundations rather than sideways on another domain's implementation merely to exchange data.
- Put genuine cross-domain behavior in an explicit adapter or composition layer that depends on both domains.
- Applications may compose several domain capabilities directly; that composition is not itself unwanted coupling.

## DEP-009 — Depend on capability surfaces, not upstream topology

- Consume the smallest stable public surface that represents the required capability.
- Do not make callers understand an upstream repository's internal crate/package decomposition when a cohesive public facade or adapter can hide it.
- If ordinary callers must pin or update several same-owner implementation packages together to obtain one capability, improve the owning repository's public boundary instead of spreading that topology into more consumers.
- Do not create a facade that merely forwards APIs without reducing caller knowledge or concentrating compatibility logic.

## DEP-010 — Give every versioned package one canonical owner

- A versioned package or crate must have one canonical repository responsible for source changes, compatibility, tests, and releases.
- During a repository migration, document the temporary old/new ownership state and the exact cutover condition.
- After cutover, the former repository may keep compatibility shims or provenance, but must not remain a competing release or source authority.

## DEP-011 — Treat source overrides as development mechanics

- Exact source overrides may substitute unpublished revisions during cross-repository development, but they must preserve the intended public dependency direction.
- Do not use source patches to normalize permanent sideways dependencies, duplicate ownership, or consumer knowledge of implementation internals.
- Repeated feature work that requires coordinated source heads across several sibling repositories is an architecture signal: introduce or improve a contract, capability surface, adapter, or ownership boundary before expanding the patch graph further.

## DEP-012 — Declare dependency versions according to the consumer contract

- Applications, internal tooling, and other leaf deliverables prefer exact dependency versions for predictable resolution.
- Published libraries declare the narrowest semver-compatible range that truthfully describes the versions consumers may use.
- Lockfiles remain the reproducible resolution record; do not use `latest`, wildcard ranges, or similarly floating declarations for leaf repositories.

## DEP-013 — Make dependency changes explicit transactions

- Ordinary install and verification commands use the committed lockfile without modifying it.
- A dependency-manifest change includes the corresponding lockfile change; unexplained lockfile-only changes are not normal feature-work noise.
- Dependency updates use an explicit update command or workflow. Do not run a generic update merely to make dependency resolution or a build start passing.
- Verification should use the ecosystem's frozen/locked mode where available.

## DEP-014 — Keep production dependency graphs acyclic by default

- Production package, module, project, and repository dependency graphs should be acyclic where the ecosystem can model and verify the relationship.
- Test-only, generated, framework-required, or otherwise unavoidable cycles use a narrow explicit exception rather than weakening cycle detection globally.
- Treat a newly introduced cycle as an ownership or boundary problem to resolve, not as normal dependency noise.

## DEP-015 — Centralize automated dependency update policy

- Use Renovate as the canonical routine dependency-update engine for repositories adopting these conventions.
- Keep the shared Renovate policy in this repository's `default.json`; consumer repositories should normally contain only a small `renovate.json` extending `github>moritzbrantner/coding-agent-conventions` plus narrow repository-specific exceptions when genuinely required.
- Keep dependency-update policy in conventions and installation/scaffolding behavior in coding tooling; do not duplicate the policy across repository templates or consumers.
- Ordinary dependency updates run on the shared cadence and limits. Major updates require explicit Dependency Dashboard approval, and broad automerge stays disabled until a repository has a trustworthy deterministic gate.
- Keep GitHub dependency/vulnerability detection available, but do not run overlapping Dependabot version-update PRs after a repository has migrated to Renovate. Existing Dependabot updater configuration must be migrated explicitly rather than deleted implicitly by tooling.

## DEP-016 — Choose distribution by source ownership

- When upstream remains responsible for implementation changes, compatibility, fixes, and releases, consume the capability through the ecosystem's normal package or dependency mechanism.
- When the consumer is expected to own and intentionally customize copied implementation source, use an explicit source-copy contract that records the immutable upstream revision, source-to-target mapping, and content fingerprints in committed provenance metadata.
- Treat consumer-owned copied source as legitimately divergent after installation. Preserve the recorded upstream base so tooling can distinguish clean, modified, and missing files and can reconcile base, local, and newer-upstream content deterministically.
- Use managed snapshots when copied policy, configuration, or generated distribution material remains upstream-owned and consumers should not edit the installed snapshot directly.
- Keep source-copy registries lightweight. Do not grow them into replacement package managers with general version solving, broad transitive dependency semantics, or automatic semantic conflict merging.
- Content fingerprints prove source identity and drift, not runtime or API compatibility; consumer tests, type checks, and benchmarks remain the compatibility gate.

## DEP-017 — Prove dependency ranges as a clean consumer

- Treat the repository's locked development graph, the minimum compatibility point declared by peer ranges, and a fresh registry resolution as three different pieces of evidence.
- Consumer verification for a deterministic compatibility point installs exact versions derived from the package's declared contract rather than floating caret, tilde, or inequality ranges that change meaning over time.
- For publishable packages, use a clean consumer and the real package-manager resolver to prove both the deterministic minimum compatibility point and the current fresh-range resolution before release or distribution integration.
- Do not use `--force`, `--legacy-peer-deps`, or equivalent resolver bypasses as proof that a full consumer dependency graph is valid.
- Registry or network unavailability is an unavailable result, never a passing compatibility result and never evidence that repository code is defective.
- A locked repository graph passing while a clean consumer fails is an explicit distribution-contract finding; repair the verifier or declared peer contract according to the failing boundary instead of weakening resolution.
