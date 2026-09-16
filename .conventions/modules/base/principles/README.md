# Principles

## PRINCIPLE-001 — Prefer determinism over inference

- Prefer executable checks, deterministic mappings, explicit baselines, and structured ownership over semantic inference.

## PRINCIPLE-002 — Structure should encode agent-relevant information

- Use paths, hierarchy, names, and local instructions to communicate scope, ownership, relevance, and dependencies.

## PRINCIPLE-003 — Validate progressively

- Run the narrowest, cheapest affected checks first; expand only after they pass.
- Re-run invalidated lower layers after a production-code change.
- During an implementation loop, use focused checks for fast feedback; run the repository-owned full gate as completion evidence rather than after every edit.
- Do not make a worker repeat a full gate that the coordinating layer will immediately and independently run again unless the worker needs that full result to continue reasoning.

## PRINCIPLE-004 — Make completion observable

- Completion is defined by repository-owned, independently repeatable gates—not agent confidence.

## PRINCIPLE-005 — Document decisions, not defaults

- Document consequential choices agents cannot reliably infer.
- Prefer tooling over prose for deterministic behavior.

## PRINCIPLE-006 — Escalate complexity only when the workload requires it

- Treat direct human-to-agent work as a first-class execution mode.
- Add reusable skills when a procedure should be shared; add a loop when iteration should be automated; add tasks or orchestration only when coordination, dependency management, concurrency, durable control state, or multi-worker ownership justify them.
- Higher-level execution layers may compose lower-level capabilities, but lower-level capabilities must not require higher-level machinery merely because it exists.
- Prefer escalation from a simple invocation over configuration that makes a large framework tolerate simple work.
- Treat model inference as a scarce execution resource: obtain deterministic evidence first and invoke stronger or more stateful agent execution only when cheaper layers cannot resolve the task.
- Prefer an escalation funnel such as deterministic inspection/checks → narrow direct or repository edit → focused verification → iterative local agent work → full environment-backed debugging. Skip directly to a later stage when the failure is inherently runtime-, browser-, service-, numerical-, or environment-dependent.
- Escalate because the current layer has stopped producing useful information, not merely because an arbitrary retry count elapsed. Repeated unchanged failures, ambiguous environment behavior, or a need for runtime feedback are strong escalation signals.
- Preserve evidence across attempts—baseline/candidate identity, relevant commands, exit status, focused output, failure classification, prior patch/result, and environment identity where available—so a later agent continues the investigation instead of rediscovering it.
- Let deterministic control logic decide whether another expensive attempt is justified whenever the decision can be made from observed state; do not spend a coding-model invocation merely to decide whether to invoke the coding model again.
- Reuse stable environments, dependency/tool caches, and already-known repository context where doing so preserves correctness and isolation.
- Record enough execution metadata to evaluate the routing policy over time, including attempts, provider/model, verification failures, escalation stage, and time or work required to reach a checked candidate.

## PRINCIPLE-007 — Keep capabilities replaceable; internalize with evidence

- External libraries, services, processes, and hosted infrastructure are valid bootstrap implementations; avoid unnecessary domain coupling to a particular implementation.
- Internalize only the capability actually consumed, and only when evidence supports a concrete benefit such as fewer expensive boundaries, lower latency or resource use, a smaller dependency surface, stronger determinism or portability, or useful cross-project reuse.
- Prefer staged replacement: external implementation → explicit capability contract → native candidate → differential/parity validation → representative benchmarks → real consumer → optional default switch or removal.
- Reimplementation is not justified by implementation cost alone; retain mature external implementations when a replacement has no demonstrated advantage.
- See also: [ADR 0001 — Capability internalization](../docs/adr/0001-capability-internalization.md).

## PRINCIPLE-008 — Compute and validate once; reuse the trusted representation

- At a semantic or trust boundary, parse, normalize, probe, or otherwise compute the required representation once, validate it there, and pass the resulting trusted typed object downstream.
- Do not repeatedly re-read raw input, re-parse equivalent bytes, re-run equivalent validation, or recompute the same derived structure merely because another stage needs the same already-proven facts.
- Keep invalidation explicit: when any deterministic input that gives the object its meaning changes, discard or rebuild the affected trusted object and its derived structures before reuse.
- Recompute when a consumer genuinely needs different evidence, when independent verification is itself part of the contract, or when reuse would blur ownership or freshness semantics.
- Prefer ownership-preserving handles, references, immutable snapshots, or other cheap sharing mechanisms over repeated conversion and cloning where the language/runtime supports them.
- Do not extract a fleet-wide shared type merely to implement this principle. Keep the validated representation with its semantic owner and extract a reusable contract only after multiple real consumers demonstrate the same meaning and lifecycle.
