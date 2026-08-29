#![cfg(target_os = "linux")]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use agent_loop_orchestrator::{
    adapters::Provider,
    config::ProjectConfig,
    contracts::{PublicationKind, PublicationStatus},
    execution::ExecutionService,
    remote::{
        CheckState, LocalRepairExecutor, Mergeability, PullRequestSnapshot, RepairExecutor,
        RepairReason, RepairRequest, RepairResult,
    },
    repository::RegisteredProject,
};
use tempfile::TempDir;

#[test]
fn remote_repair_publishes_from_an_automation_checkout_without_touching_local_development() {
    let fixture = Fixture::new();
    let main_before = git(&fixture.developer, &["rev-parse", "main"]);
    fs::write(
        fixture.developer.join("local-development.txt"),
        "unfinished\n",
    )
    .unwrap();
    let project = RegisteredProject {
        id: "demo".into(),
        repository_root: fixture.developer.clone(),
    };
    let pull_request = PullRequestSnapshot {
        number: 42,
        title: "Feature".into(),
        url: "https://github.example/owner/demo/pull/42".into(),
        author: "trusted".into(),
        state: "OPEN".into(),
        draft: false,
        head_ref: "feature".into(),
        head_sha: fixture.feature_sha.clone(),
        head_repository: "owner/demo".into(),
        base_ref: "main".into(),
        cross_repository: false,
        checks: CheckState::Failed,
        failed_checks: Vec::new(),
        mergeability: Mergeability::Mergeable,
        merge_state: "UNSTABLE".into(),
        review_decision: None,
    };
    let mut repairs = LocalRepairExecutor::new(&fixture.data, fixture.github.display().to_string());

    let result = repairs
        .repair(RepairRequest {
            project: &project,
            config: &fixture.config,
            repository: "owner/demo",
            pull_request: &pull_request,
            reason: RepairReason::FailedChecks,
            failure_details: "the test job failed",
        })
        .unwrap();

    let RepairResult::Published { run_id, head_sha } = result else {
        panic!("expected a published repair candidate");
    };
    assert_ne!(head_sha, fixture.feature_sha);
    assert_eq!(
        git_bare(&fixture.remote, &["rev-parse", "refs/heads/feature"]),
        head_sha
    );
    assert_eq!(git(&fixture.developer, &["rev-parse", "main"]), main_before);
    assert_eq!(
        fs::read_to_string(fixture.developer.join("local-development.txt")).unwrap(),
        "unfinished\n"
    );
    assert!(git(&fixture.developer, &["status", "--porcelain"]).contains("local-development.txt"));

    let run = ExecutionService::load(&fixture.data)
        .unwrap()
        .snapshot()
        .runs
        .into_iter()
        .find(|run| run.id == run_id)
        .unwrap();
    assert!(run.contract.publications.iter().any(|publication| {
        publication.kind == PublicationKind::PullRequest
            && publication.status == PublicationStatus::Succeeded
            && publication.candidate_identity == head_sha
    }));
}

struct Fixture {
    _root: TempDir,
    data: PathBuf,
    developer: PathBuf,
    remote: PathBuf,
    github: PathBuf,
    config: ProjectConfig,
    feature_sha: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let developer = root.path().join("developer");
        let remote = root.path().join("remote.git");
        fs::create_dir_all(&developer).unwrap();
        git_ok(&developer, &["init", "-b", "main"]);
        git_ok(&developer, &["config", "user.name", "Remote Test"]);
        git_ok(&developer, &["config", "user.email", "remote@example.test"]);

        let provider = root.path().join("fake-codex");
        executable(
            &provider,
            "#!/bin/sh\nset -eu\nprintf 'repaired\\n' > repaired.txt\ngit add repaired.txt\ngit -c user.name='Repair Agent' -c user.email='repair@example.test' commit -m repair >/dev/null\nprintf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"remote-thread\"}'\n",
        );
        let tooling = root.path().join("fake-coding-tooling");
        executable(
            &tooling,
            "#!/bin/sh\nset -eu\ncandidate=$(git rev-parse HEAD)\nprintf '{\"schemaVersion\":1,\"checkId\":\"tests\",\"capability\":\"test\",\"candidate\":{\"kind\":\"git-commit\",\"identity\":\"%s\"},\"outcome\":\"passed\",\"required\":true,\"startedAt\":\"2026-08-27T10:00:00Z\",\"finishedAt\":\"2026-08-27T10:00:01Z\",\"exitCode\":0,\"evidence\":[]}\\n' \"$candidate\"\n",
        );
        let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        config.providers.codex.executable = provider.display().to_string();
        config.execution.coding_tooling_executable = tooling.display().to_string();
        config.remote.enabled = true;
        config.remote.repository = Some("owner/demo".into());
        config.remote.repair_failures = true;
        config.remote.trusted_authors = vec!["trusted".into()];
        fs::create_dir_all(developer.join(".agent-loop")).unwrap();
        fs::write(
            developer.join(".agent-loop/config.toml"),
            config.to_toml().unwrap(),
        )
        .unwrap();
        fs::write(developer.join("README.md"), "baseline\n").unwrap();
        git_ok(&developer, &["add", ".agent-loop/config.toml", "README.md"]);
        git_ok(&developer, &["commit", "-m", "baseline"]);
        git_ok(&developer, &["switch", "-c", "feature"]);
        fs::write(developer.join("feature.txt"), "feature\n").unwrap();
        git_ok(&developer, &["add", "feature.txt"]);
        git_ok(&developer, &["commit", "-m", "feature"]);
        let feature_sha = git(&developer, &["rev-parse", "HEAD"]);
        git_ok(&developer, &["switch", "main"]);

        git_bare_ok(&remote, &["init", "--bare"]);
        git_ok(
            &developer,
            &["remote", "add", "origin", &remote.display().to_string()],
        );
        git_ok(&developer, &["push", "origin", "main", "feature"]);

        let github = root.path().join("fake-gh");
        executable(
            &github,
            &format!(
                "#!/bin/sh\nset -eu\n[ \"$1\" = repo ]\n[ \"$2\" = clone ]\ngit clone '{}' \"$4\" >/dev/null\n",
                remote.display()
            ),
        );

        Self {
            data,
            developer,
            remote,
            github,
            config,
            feature_sha,
            _root: root,
        }
    }
}

fn executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn git_ok(root: &Path, arguments: &[&str]) {
    let _ = git(root, arguments);
}

fn git_bare(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(root)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git --git-dir {} {arguments:?}: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn git_bare_ok(root: &Path, arguments: &[&str]) {
    let _ = git_bare(root, arguments);
}
