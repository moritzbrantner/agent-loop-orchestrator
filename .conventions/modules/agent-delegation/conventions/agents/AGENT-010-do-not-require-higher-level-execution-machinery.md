# AGENT-010 — Apply progressive composition to agent execution

**Status:** Accepted
**Category:** Agents
**Derived from:** `PRINCIPLE-003`, `PRINCIPLE-006`

## Rule

Use `PRINCIPLE-006 — Escalate complexity only when the workload requires it` as the single normative source when choosing between deterministic tooling, direct repository work, reusable skills, iterative agent loops, environment-backed debugging, work items, and orchestration. Use `PRINCIPLE-003` for the corresponding progressive verification behavior.

This convention introduces no additional execution-layer policy. Its purpose is to give agent-focused documents and tooling a stable convention identifier that points to the repository-level principles without duplicating them.

## Agent behavior

When an agent-facing procedure needs to decide whether a higher execution layer is warranted, follow `PRINCIPLE-006` directly.

In particular:

- gather cheap deterministic evidence before spending a coding-model invocation when the evidence can materially narrow the task;
- preserve relevant failure evidence between attempts instead of asking later attempts to rediscover it;
- use focused validation while iterating and rely on the coordinating harness for the independently repeated completion gate;
- escalate when the current layer stops producing useful information or the problem inherently requires runtime/environment feedback;
- do not repeatedly invoke the same expensive worker on materially unchanged evidence merely because a retry budget remains.

## Automatable check

Agent documentation may reference `AGENT-010` as the agent-category pointer, but checks should resolve the normative rules to `PRINCIPLE-003` and `PRINCIPLE-006` rather than maintaining a second copy of the escalation or verification criteria.
