# Agent Loop Orchestrator

The Agent Loop Orchestrator is a single-user local control plane for coordinating coding-agent work across registered repositories.

## Language

**Run Dashboard**:
The localhost workspace that creates local work items, starts their agent runs, displays output and checks, and records an explicit candidate decision.
_Avoid_: Admin panel, control panel, web page

**Run**:
A durable canonical lifecycle for one local Work Item, including its exact baseline, single isolated Attempt, Candidate, checks, evidence, and decision.
_Avoid_: Job, task, execution

**Active Run**:
The single Run currently launching, executing, evaluating, awaiting a decision, or integrating through the local execution lease. This may be a human-created local Run, an Issue Implementation, or a Pull Request Repair; no other Run may become active until it reaches a terminal outcome.
_Avoid_: Current task, worker

**Work Item**:
A durable local request bound to one Registered Project, declared write scope, target branch, and exact baseline commit before execution begins.
_Avoid_: GitHub Issue, pending run

**Candidate**:
The exact clean Git commit produced by an Attempt and retained by an immutable local ref for checking and decision.
_Avoid_: Working tree, latest changes

**Decision**:
An approval or rejection bound to an exact Candidate. Human-created local Runs require an explicit human decision. Queue-created Runs may record the configured queue policy as their actor before guarded publication; rejection never integrates.
_Avoid_: Agent confidence, implicit approval

**Queue Runner**:
The bounded serial scheduler that refreshes one GitHub repository, delegates pull-request integration to coding-tooling, repairs exact PR heads when allowed, and publishes checked Candidates for unblocked agent-ready issues.
_Avoid_: Infinite loop, webhook

**Pull Request Repair**:
A Run created from an exact same-repository pull-request head in an ordinary detached Git worktree to fix a repairable integration failure. Its Provider cannot publish; the Queue Runner may publish the checked Candidate with an exact-head lease.
_Avoid_: CI rerun, direct bot push

**Issue Implementation**:
A Run created from an unblocked `ready-for-agent` GitHub Issue and the current target-branch head. Its checked Candidate may be published as a ready pull request by the Queue Runner.
_Avoid_: Raw issue triage, PRD decomposition

**Completion Notification**:
An in-app and optional browser desktop notice that an Active Run has reached a terminal outcome.
_Avoid_: Alert, message

**Cancelled Run**:
A Run whose user explicitly stopped its agent process before it reached its normal terminal outcome.
_Avoid_: Failed run, deleted run

**Registered Project**:
A local Git repository known to the orchestrator and configured for agent-loop use.
_Avoid_: Repository, workspace, codebase

**Provider**:
A configured coding-agent runtime that can perform a Run, currently Codex or Claude.
_Avoid_: Model, agent

**Run History**:
The indefinitely retained local record of Runs, including attempts, output, candidates, checks, evidence, decisions, and local integration.
_Avoid_: Logs, activity feed

**Access Token**:
The short-lived shared secret generated for one service startup and required to use the Agent Loop Orchestrator on a local network.
_Avoid_: Password, user account
