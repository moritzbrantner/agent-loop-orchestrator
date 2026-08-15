# Agent Loop Orchestrator

The Agent Loop Orchestrator is a single-user local control plane for coordinating coding-agent work across registered repositories.

## Language

**Run Dashboard**:
The localhost workspace that shows registered projects and their agent runs, lets its user start a new run, and displays each run's live status and output.
_Avoid_: Admin panel, control panel, web page

**Run**:
A recorded attempt to perform a user-supplied coding task in one registered project through a configured coding-agent provider.
_Avoid_: Job, task, execution

**Active Run**:
The single Run currently launching or executing on the local service; no other Run may become active until it reaches a terminal outcome.
_Avoid_: Current task, worker

**Pending Run**:
A single saved, editable request for a Run that cannot start while another Run is active and never starts automatically.
_Avoid_: Queue item, scheduled run

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
The indefinitely retained local record of completed and in-progress Runs, including their status and captured output.
_Avoid_: Logs, activity feed

**Access Token**:
The short-lived shared secret generated for one service startup and required to use the Agent Loop Orchestrator on a local network.
_Avoid_: Password, user account
