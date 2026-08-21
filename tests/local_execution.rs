#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
    sync::{Arc, Barrier},
    thread,
    time::{Duration, Instant},
};

use agent_loop_orchestrator::{
    adapters::Provider,
    config::ProjectConfig,
    execution::{CreateWorkItem, DecisionRequest, ExecutionService, LocalRunStatus},
    repository::RegisteredProject,
};
use tempfile::TempDir;

const SUCCESSFUL_PROVIDER: &str = "set -eu\nprintf 'hello from provider\\n' > greeting.txt\ngit add greeting.txt\ngit commit -m 'candidate' >/dev/null\nprintf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"fake-thread\"}'";
const PASSED_CHECK: &str = "set -eu\ncandidate=$(git rev-parse HEAD)\nprintf '{\"schemaVersion\":1,\"checkId\":\"fake-check\",\"capability\":\"test\",\"candidate\":{\"kind\":\"git-commit\",\"identity\":\"%s\"},\"outcome\":\"passed\",\"required\":true,\"startedAt\":\"2026-08-15T10:00:00Z\",\"finishedAt\":\"2026-08-15T10:00:01Z\",\"exitCode\":0,\"evidence\":[]}\\n' \"$candidate\"";

#[test]
fn successful_run_integrates_the_exact_checked_candidate() {
    let fixture = Fixture::new();
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();
    let run_id = run.id;
    assert_eq!(
        run.status,
        LocalRunStatus::AwaitingDecision,
        "run failed: {:?}",
        run.error
    );
    let candidate = run.contract.candidates[0].git_sha.clone().unwrap();
    assert_eq!(run.contract.checks.len(), 1);
    assert!(!run.contract.authority.may_integrate);
    assert_eq!(
        run.contract.authority.network.mode,
        agent_loop_orchestrator::contracts::NetworkMode::Unrestricted
    );
    assert!(
        fixture
            .data
            .path()
            .join("runs")
            .join(run.id.to_string())
            .join("task-packet.json")
            .exists()
    );
    assert_eq!(
        git(
            fixture.repository.path(),
            &[
                "rev-parse",
                &format!("refs/agent-loop/candidates/{}", run.id)
            ]
        ),
        candidate
    );
    assert_eq!(
        git(fixture.repository.path(), &["rev-parse", "main"]),
        work_item.baseline.git_sha
    );

    let integrated = service
        .decide(
            run_id,
            DecisionRequest::Approve {
                actor: "local-user".into(),
                reason: None,
            },
        )
        .unwrap();

    assert_eq!(integrated.status, LocalRunStatus::Completed);
    assert_eq!(
        git(fixture.repository.path(), &["rev-parse", "main"]),
        candidate
    );
    assert_eq!(
        fs::read_to_string(fixture.repository.path().join("greeting.txt")).unwrap(),
        "hello from provider\n"
    );
    assert!(!integrated.worktree_path.exists());
}

#[test]
fn claude_provider_uses_the_same_isolated_candidate_lifecycle() {
    let fixture = Fixture::new();
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Claude, None, |_| {})
        .unwrap();

    assert_eq!(run.provider, Provider::Claude);
    assert_eq!(run.status, LocalRunStatus::AwaitingDecision);
    assert_eq!(run.contract.agent.adapter, "claude");
    assert!(run.contract.candidates[0].git_sha.is_some());
}

#[test]
fn provider_sandbox_blocks_registered_checkout_and_target_ref_mutation() {
    let fixture = Fixture::with_scripts(
        "set -eu\ncommon=$(git rev-parse --git-common-dir)\nrepository=$(dirname \"$common\")\nif printf 'intrusion\\n' > \"$repository/intrusion.txt\"; then exit 80; fi\nif git update-ref refs/heads/main HEAD; then exit 81; fi\nprintf 'hello from provider\\n' > greeting.txt\ngit add greeting.txt\ngit commit -m 'candidate' >/dev/null",
        PASSED_CHECK,
    );
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();

    assert_eq!(run.status, LocalRunStatus::AwaitingDecision);
    assert!(!fixture.repository.path().join("intrusion.txt").exists());
    assert_eq!(
        git(fixture.repository.path(), &["rev-parse", "main"]),
        work_item.baseline.git_sha
    );
}

#[test]
fn provider_failure_is_recorded_and_never_creates_a_candidate() {
    let fixture = Fixture::with_scripts("echo provider-broke >&2\nexit 7", "exit 99");
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();

    assert_eq!(run.status, LocalRunStatus::Failed);
    assert!(run.contract.candidates.is_empty());
    assert!(run.error.as_deref().unwrap().contains("provider failed"));
    assert!(!run.contract.attempts[0].evidence.is_empty());
    assert!(!run.worktree_path.exists());
}

#[test]
fn successful_provider_without_a_commit_is_a_missing_candidate_failure() {
    let fixture = Fixture::with_scripts(
        "printf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"fake-thread\"}'",
        "exit 99",
    );
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();

    assert_eq!(run.status, LocalRunStatus::Failed);
    assert!(run.contract.candidates.is_empty());
    assert!(run.error.as_deref().unwrap().contains("candidate missing"));
}

#[test]
fn failed_deterministic_check_records_evidence_and_stops_before_decision() {
    let fixture = Fixture::with_scripts(
        SUCCESSFUL_PROVIDER,
        "candidate=$(git rev-parse HEAD)\nprintf '{\"schemaVersion\":1,\"checkId\":\"fake-check\",\"capability\":\"test\",\"candidate\":{\"kind\":\"git-commit\",\"identity\":\"%s\"},\"outcome\":\"failed\",\"required\":true,\"startedAt\":\"2026-08-15T10:00:00Z\",\"finishedAt\":\"2026-08-15T10:00:01Z\",\"exitCode\":1,\"reason\":\"tests failed\",\"evidence\":[]}\\n' \"$candidate\"",
    );
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();

    assert_eq!(run.status, LocalRunStatus::Failed);
    assert_eq!(run.contract.checks.len(), 1);
    assert!(!run.contract.checks[0].evidence.is_empty());
    assert!(run.contract.decisions.is_empty());
    assert_eq!(
        git(fixture.repository.path(), &["rev-parse", "main"]),
        work_item.baseline.git_sha
    );
}

#[test]
fn unavailable_coding_tooling_is_explicit_and_stops_the_run() {
    let fixture = Fixture::new();
    let mut config = ProjectConfig::load(fixture.repository.path()).unwrap();
    config.execution.coding_tooling_executable = fixture
        .data
        .path()
        .join("does-not-exist")
        .display()
        .to_string();
    write_config(&fixture, &config);
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();

    assert_eq!(run.status, LocalRunStatus::Failed);
    assert_eq!(run.contract.checks.len(), 1);
    assert_eq!(
        run.contract.checks[0].outcome,
        agent_loop_orchestrator::contracts::CheckOutcome::Unavailable
    );
    assert!(
        run.contract.checks[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("unavailable")
    );
}

#[test]
fn malformed_coding_tooling_response_is_an_explicit_check_error() {
    let fixture = Fixture::with_scripts(SUCCESSFUL_PROVIDER, "printf 'not-json\\n'");
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();

    assert_eq!(run.status, LocalRunStatus::Failed);
    assert_eq!(
        run.contract.checks[0].outcome,
        agent_loop_orchestrator::contracts::CheckOutcome::Error
    );
    assert!(
        run.contract.checks[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("malformed")
    );
}

#[test]
fn nonzero_coding_tooling_exit_cannot_report_a_passing_run() {
    let fixture = Fixture::with_scripts(SUCCESSFUL_PROVIDER, &format!("{PASSED_CHECK}\nexit 9"));
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();

    assert_eq!(run.status, LocalRunStatus::Failed);
    assert!(run.contract.checks.iter().any(|check| {
        check.outcome == agent_loop_orchestrator::contracts::CheckOutcome::Error
            && check
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("exited"))
    }));
}

#[test]
fn structurally_parseable_but_noncanonical_check_result_is_rejected() {
    let fixture = Fixture::with_scripts(
        SUCCESSFUL_PROVIDER,
        "candidate=$(git rev-parse HEAD)\nprintf '{\"schemaVersion\":2,\"checkId\":\"fake-check\",\"capability\":\"test\",\"candidate\":{\"kind\":\"git-commit\",\"identity\":\"%s\"},\"outcome\":\"passed\",\"required\":true,\"startedAt\":\"2026-08-15T10:00:00Z\",\"finishedAt\":\"2026-08-15T10:00:01Z\",\"evidence\":[]}\\n' \"$candidate\"",
    );
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();

    assert_eq!(run.status, LocalRunStatus::Failed);
    assert_eq!(
        run.contract.checks[0].outcome,
        agent_loop_orchestrator::contracts::CheckOutcome::Error
    );
    assert!(
        run.contract.checks[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("schema version")
    );
}

#[test]
fn failed_legacy_envelope_cannot_be_hidden_by_passed_result_rows() {
    let fixture = Fixture::with_scripts(
        SUCCESSFUL_PROVIDER,
        "printf '%s\\n' '{\"schemaVersion\":1,\"operation\":\"run\",\"status\":\"failed\",\"durationMs\":12,\"data\":{\"results\":[{\"capability\":\"test\",\"component\":\"demo\",\"status\":\"passed\",\"exitCode\":0}],\"missing\":[]},\"diagnostics\":[{\"message\":\"tier failed\"}]}'",
    );
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();

    assert_eq!(run.status, LocalRunStatus::Failed);
    assert!(run.contract.checks.iter().any(|check| {
        check.required && check.outcome == agent_loop_orchestrator::contracts::CheckOutcome::Failed
    }));
}

#[test]
fn rejection_records_the_decision_without_integrating_the_candidate() {
    let fixture = Fixture::new();
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);
    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();
    let candidate = run.contract.candidates[0].git_sha.clone().unwrap();

    let rejected = service
        .decide(
            run.id,
            DecisionRequest::Reject {
                actor: "local-user".into(),
                reason: Some("not the desired change".into()),
            },
        )
        .unwrap();

    assert_eq!(rejected.status, LocalRunStatus::Completed);
    assert_eq!(rejected.contract.decisions.len(), 1);
    assert!(rejected.contract.publications.is_empty());
    assert_ne!(
        git(fixture.repository.path(), &["rev-parse", "main"]),
        candidate
    );
    assert_eq!(
        service.snapshot().work_items[0].status,
        agent_loop_orchestrator::execution::WorkItemStatus::Rejected
    );
}

#[test]
fn concurrent_decisions_record_exactly_one_terminal_outcome() {
    let fixture = Fixture::new();
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);
    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();
    let run_id = run.id;
    let candidate = run.contract.candidates[0].git_sha.clone().unwrap();
    let data_root = fixture.data.path().to_path_buf();
    let barrier = Arc::new(Barrier::new(2));

    let approve_barrier = Arc::clone(&barrier);
    let approve_root = data_root.clone();
    let approve = thread::spawn(move || {
        let mut service = ExecutionService::load(approve_root).unwrap();
        approve_barrier.wait();
        service.decide(
            run_id,
            DecisionRequest::Approve {
                actor: "approver".into(),
                reason: None,
            },
        )
    });
    let reject_barrier = Arc::clone(&barrier);
    let reject_root = data_root.clone();
    let reject = thread::spawn(move || {
        let mut service = ExecutionService::load(reject_root).unwrap();
        reject_barrier.wait();
        service.decide(
            run_id,
            DecisionRequest::Reject {
                actor: "rejecter".into(),
                reason: None,
            },
        )
    });

    let outcomes = [approve.join().unwrap(), reject.join().unwrap()];
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    let persisted = ExecutionService::load(&data_root).unwrap().snapshot().runs[0].clone();
    assert_eq!(persisted.contract.decisions.len(), 1);
    assert_eq!(persisted.status, LocalRunStatus::Completed);
    match &persisted.contract.decisions[0].decision {
        agent_loop_orchestrator::contracts::DecisionOutcome::Approved => assert_eq!(
            git(fixture.repository.path(), &["rev-parse", "main"]),
            candidate
        ),
        agent_loop_orchestrator::contracts::DecisionOutcome::Rejected => assert_eq!(
            git(fixture.repository.path(), &["rev-parse", "main"]),
            work_item.baseline.git_sha
        ),
        agent_loop_orchestrator::contracts::DecisionOutcome::ChangesRequested => {
            panic!("unexpected local decision")
        }
    }
}

#[test]
fn candidate_outside_declared_scope_is_rejected_at_the_boundary() {
    let fixture = Fixture::with_scripts(
        "set -eu\nprintf 'outside\\n' > other.txt\ngit add other.txt\ngit commit -m 'out of scope' >/dev/null",
        "exit 99",
    );
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();

    assert_eq!(run.status, LocalRunStatus::Failed);
    assert!(run.error.as_deref().unwrap().contains("scope mismatch"));
    assert!(run.contract.candidates.is_empty());
}

#[test]
fn changed_target_baseline_blocks_approval_and_keeps_the_run_awaiting() {
    let fixture = Fixture::new();
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);
    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();
    fs::write(fixture.repository.path().join("concurrent.txt"), "change\n").unwrap();
    git_ok(fixture.repository.path(), &["add", "concurrent.txt"]);
    git_ok(
        fixture.repository.path(),
        &["commit", "-m", "concurrent change"],
    );

    let error = service
        .decide(
            run.id,
            DecisionRequest::Approve {
                actor: "local-user".into(),
                reason: None,
            },
        )
        .unwrap_err();

    assert!(error.to_string().contains("baseline mismatch"));
    let persisted = service
        .snapshot()
        .runs
        .into_iter()
        .find(|stored| stored.id == run.id)
        .unwrap();
    assert_eq!(persisted.status, LocalRunStatus::AwaitingDecision);
    assert!(persisted.contract.decisions.is_empty());
}

#[test]
fn awaiting_run_survives_service_restart_with_candidate_checks_and_evidence() {
    let fixture = Fixture::new();
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);
    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();
    drop(service);

    let restarted = ExecutionService::load(fixture.data.path()).unwrap();
    let snapshot = restarted.snapshot();
    let persisted = snapshot
        .runs
        .into_iter()
        .find(|stored| stored.id == run.id)
        .unwrap();

    assert_eq!(persisted.status, LocalRunStatus::AwaitingDecision);
    assert_eq!(persisted.contract.candidates, run.contract.candidates);
    assert_eq!(persisted.contract.checks, run.contract.checks);
    assert!(!persisted.contract.attempts[0].evidence.is_empty());
    assert_eq!(snapshot.work_items[0].run_id, Some(run.id));
}

#[test]
fn service_startup_reconciles_an_interrupted_attempt_without_losing_records() {
    let fixture = Fixture::new();
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);
    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();
    drop(service);
    let state_path = fixture.data.path().join("execution-state.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state["workItems"][0]["status"] = "running".into();
    state["runs"][0]["status"] = "running".into();
    state["runs"][0]["finishedAt"] = serde_json::Value::Null;
    state["runs"][0]["contract"]["state"] = "running".into();
    fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let mut restarted = ExecutionService::load(fixture.data.path()).unwrap();
    restarted.recover_interrupted_runs().unwrap();
    let persisted = restarted.snapshot().runs[0].clone();

    assert_eq!(persisted.status, LocalRunStatus::Failed);
    assert_eq!(persisted.contract.candidates, run.contract.candidates);
    assert_eq!(persisted.contract.checks, run.contract.checks);
    assert!(!persisted.contract.attempts[0].evidence.is_empty());
    assert!(persisted.error.as_deref().unwrap().contains("restarted"));
}

#[test]
fn durable_store_preserves_concurrent_work_items_and_allows_only_one_active_run() {
    let fixture = Fixture::with_scripts(
        &format!(
            "printf '%s\\n' '{{\"type\":\"thread.started\",\"thread_id\":\"early-thread\"}}'\nsleep 1\n{SUCCESSFUL_PROVIDER}"
        ),
        PASSED_CHECK,
    );
    let mut first_service = ExecutionService::load(fixture.data.path()).unwrap();
    let first = create_default_work_item(&fixture, &mut first_service);
    let data_root = fixture.data.path().to_path_buf();
    let first_id = first.id;
    let worker = thread::spawn(move || {
        first_service
            .run_work_item(&first_id, Provider::Codex, None, |_| {})
            .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = ExecutionService::load(&data_root).unwrap().snapshot();
        if snapshot
            .runs
            .iter()
            .any(|run| run.status == LocalRunStatus::Running && !run.output.is_empty())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "first run did not expose durable live output"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let mut recovery_service = ExecutionService::load(&data_root).unwrap();
    recovery_service.recover_interrupted_runs().unwrap();
    assert!(
        recovery_service
            .snapshot()
            .runs
            .iter()
            .any(|run| run.status == LocalRunStatus::Running),
        "recovery must not claim a run still owned by another process"
    );
    let mut second_service = ExecutionService::load(&data_root).unwrap();
    let second = create_default_work_item(&fixture, &mut second_service);
    let error = second_service
        .run_work_item(&second.id, Provider::Claude, None, |_| {})
        .unwrap_err();
    assert!(error.to_string().contains("another local run is active"));
    let first_run = worker.join().unwrap();
    assert_eq!(first_run.status, LocalRunStatus::AwaitingDecision);
    let error = second_service
        .run_work_item(&second.id, Provider::Claude, None, |_| {})
        .unwrap_err();
    assert!(error.to_string().contains("another local run is active"));
    let snapshot = ExecutionService::load(&data_root).unwrap().snapshot();
    assert_eq!(snapshot.work_items.len(), 2);
    assert!(snapshot.work_items.iter().any(|item| item.id == second.id));
}

#[test]
fn current_coding_tooling_envelope_is_normalized_to_canonical_check_results() {
    let fixture = Fixture::with_scripts(
        SUCCESSFUL_PROVIDER,
        "printf '%s\\n' '{\"schemaVersion\":1,\"operation\":\"run\",\"status\":\"passed\",\"durationMs\":12,\"data\":{\"results\":[{\"capability\":\"test\",\"component\":\"demo\",\"status\":\"passed\",\"exitCode\":0,\"durationMs\":10}],\"missing\":[]},\"diagnostics\":[]}'",
    );
    let mut service = ExecutionService::load(fixture.data.path()).unwrap();
    let work_item = create_default_work_item(&fixture, &mut service);

    let run = service
        .run_work_item(&work_item.id, Provider::Codex, None, |_| {})
        .unwrap();

    assert_eq!(run.status, LocalRunStatus::AwaitingDecision);
    assert_eq!(run.contract.checks[0].schema_version, 1);
    assert_eq!(run.contract.checks[0].capability, "test");
    assert_eq!(
        run.contract.checks[0].candidate.identity,
        run.contract.candidates[0].git_sha.clone().unwrap()
    );
}

struct Fixture {
    data: TempDir,
    repository: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self::with_scripts(SUCCESSFUL_PROVIDER, PASSED_CHECK)
    }

    fn with_scripts(provider_body: &str, tooling_body: &str) -> Self {
        let data = tempfile::tempdir().unwrap();
        let repository = tempfile::tempdir().unwrap();
        git_ok(repository.path(), &["init", "-b", "main"]);
        git_ok(
            repository.path(),
            &["config", "user.name", "Agent Loop Test"],
        );
        git_ok(
            repository.path(),
            &["config", "user.email", "agent-loop@example.test"],
        );
        fs::write(repository.path().join("README.md"), "baseline\n").unwrap();
        git_ok(repository.path(), &["add", "README.md"]);
        git_ok(repository.path(), &["commit", "-m", "baseline"]);

        let provider = data.path().join("fake-codex");
        executable(&provider, &format!("#!/bin/sh\n{provider_body}\n"));
        let tooling = data.path().join("fake-coding-tooling");
        executable(&tooling, &format!("#!/bin/sh\n{tooling_body}\n"));

        let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        config.providers.codex.executable = provider.display().to_string();
        config.providers.claude.executable = provider.display().to_string();
        config.execution.coding_tooling_executable = tooling.display().to_string();
        fs::create_dir(repository.path().join(".agent-loop")).unwrap();
        fs::write(
            repository.path().join(".agent-loop/config.toml"),
            config.to_toml().unwrap(),
        )
        .unwrap();
        git_ok(repository.path(), &["add", ".agent-loop/config.toml"]);
        git_ok(repository.path(), &["commit", "-m", "configure agent loop"]);
        Self { data, repository }
    }
}

fn executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn git_ok(root: &Path, args: &[&str]) {
    let _ = git(root, args);
}

fn create_default_work_item(
    fixture: &Fixture,
    service: &mut ExecutionService,
) -> agent_loop_orchestrator::execution::WorkItem {
    service
        .create_work_item(CreateWorkItem {
            project: RegisteredProject {
                id: "demo".into(),
                repository_root: fixture.repository.path().into(),
            },
            title: "Implement greeting".into(),
            prompt: "Add greeting.txt".into(),
            declared_scope: vec!["greeting.txt".into()],
            baseline_ref: "main".into(),
            target_branch: "main".into(),
        })
        .unwrap()
}

fn write_config(fixture: &Fixture, config: &ProjectConfig) {
    fs::write(
        fixture.repository.path().join(".agent-loop/config.toml"),
        config.to_toml().unwrap(),
    )
    .unwrap();
    git_ok(
        fixture.repository.path(),
        &["add", ".agent-loop/config.toml"],
    );
    git_ok(
        fixture.repository.path(),
        &["commit", "-m", "update config"],
    );
}
