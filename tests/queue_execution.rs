#![cfg(target_os = "linux")]

use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use agent_loop_orchestrator::{
    adapters::Provider,
    config::{ProjectConfig, PublicationMode},
    contracts::{PublicationKind, PublicationStatus},
    execution::ExecutionService,
    queue::{LocalQueueWorker, QueueIssue, QueuePullRequest, QueueWorkResult, QueueWorker},
    repository::RegisteredProject,
};
use tempfile::TempDir;

#[test]
fn checked_issue_candidate_is_pushed_and_recorded_as_a_ready_pull_request() {
    let fixture = Fixture::new();
    let mut worker = fixture.worker();
    let issue = QueueIssue {
        number: 3,
        title: "Add the repair marker".into(),
        body: "---\nscope:\n  - repaired.txt\n---\n\nAdd the marker.".into(),
        url: "https://github.example/owner/demo/issues/3".into(),
        labels: BTreeSet::from(["ready-for-agent".into()]),
        scope: vec!["repaired.txt".into()],
        blocked_by: Vec::new(),
    };

    let result = worker.implement(&issue).unwrap();

    let QueueWorkResult::Published {
        run_id,
        publication,
    } = result
    else {
        panic!("expected a pull request publication");
    };
    assert_eq!(publication.number, 7);
    assert_eq!(
        git_bare(
            &fixture.remote,
            &["rev-parse", "refs/heads/agent-loop/issue-3"]
        ),
        publication.head_sha
    );
    let run = ExecutionService::load(&fixture.data)
        .unwrap()
        .snapshot()
        .runs
        .into_iter()
        .find(|run| run.id == run_id)
        .unwrap();
    assert!(run.contract.publications.iter().any(|stored| {
        stored.kind == PublicationKind::PullRequest
            && stored.status == PublicationStatus::Succeeded
            && stored.candidate_identity == publication.head_sha
            && stored.external_id.as_deref() == Some(publication.url.as_str())
    }));
}

#[test]
fn checked_repair_candidate_updates_the_same_branch_with_an_exact_lease() {
    let fixture = Fixture::new();
    git_ok(&fixture.checkout, &["switch", "-c", "feature"]);
    fs::write(fixture.checkout.join("feature.txt"), "feature\n").unwrap();
    git_ok(&fixture.checkout, &["add", "feature.txt"]);
    git_ok(&fixture.checkout, &["commit", "-m", "feature"]);
    let old_head = git(&fixture.checkout, &["rev-parse", "HEAD"]);
    git_ok(&fixture.checkout, &["push", "origin", "feature"]);
    git_ok(&fixture.checkout, &["switch", "main"]);
    let mut worker = fixture.worker();

    let result = worker
        .repair(
            &QueuePullRequest {
                number: 9,
                title: "Feature".into(),
                url: "https://github.example/owner/demo/pull/9".into(),
                author: "trusted".into(),
                draft: false,
                head_ref: "feature".into(),
                head_sha: old_head.clone(),
                base_ref: "main".into(),
                same_repository: true,
            },
            "the full test tier failed",
        )
        .unwrap();

    let QueueWorkResult::Repaired { run_id, head_sha } = result else {
        panic!("expected a repaired pull request");
    };
    assert_ne!(head_sha, old_head);
    assert_eq!(
        git_bare(&fixture.remote, &["rev-parse", "refs/heads/feature"]),
        head_sha
    );
    let run = ExecutionService::load(&fixture.data)
        .unwrap()
        .snapshot()
        .runs
        .into_iter()
        .find(|run| run.id == run_id)
        .unwrap();
    assert!(run.contract.publications.iter().any(|stored| {
        stored.kind == PublicationKind::PullRequest
            && stored.status == PublicationStatus::Succeeded
            && stored.candidate_identity == head_sha
    }));
}

struct Fixture {
    _root: TempDir,
    data: PathBuf,
    checkout: PathBuf,
    remote: PathBuf,
    config: ProjectConfig,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let checkout = root.path().join("checkout");
        let remote = root.path().join("remote.git");
        fs::create_dir_all(&checkout).unwrap();
        git_ok(&checkout, &["init", "-b", "main"]);
        git_ok(&checkout, &["config", "user.name", "Queue Test"]);
        git_ok(&checkout, &["config", "user.email", "queue@example.test"]);
        fs::write(checkout.join("README.md"), "baseline\n").unwrap();
        git_ok(&checkout, &["add", "README.md"]);
        git_ok(&checkout, &["commit", "-m", "baseline"]);
        let output = Command::new("git")
            .args(["init", "--bare"])
            .arg(&remote)
            .output()
            .unwrap();
        assert!(output.status.success());
        git_ok(
            &checkout,
            &["remote", "add", "origin", &remote.display().to_string()],
        );
        git_ok(&checkout, &["push", "-u", "origin", "main"]);

        let provider = root.path().join("fake-codex");
        executable(
            &provider,
            "#!/bin/sh\nset -eu\nprintf 'repaired\n' > repaired.txt\ngit add repaired.txt\ngit -c user.name='Queue Agent' -c user.email='queue-agent@example.test' commit -m candidate >/dev/null\nprintf '%s\n' '{\"type\":\"thread.started\",\"thread_id\":\"queue-thread\"}'\n",
        );
        let tooling = root.path().join("fake-coding-tooling");
        executable(
            &tooling,
            "#!/bin/sh\nset -eu\nif [ \"${1:-}\" = \"environment\" ] && [ \"${2:-}\" = \"verify\" ]; then\n  profile=${4:-default}\n  printf '{\"schemaVersion\":1,\"operation\":\"environment\",\"status\":\"passed\",\"durationMs\":1,\"data\":{\"action\":\"verify\",\"fingerprintVersion\":\"environment-fingerprint-v1\",\"profile\":\"%s\",\"expectedFingerprint\":\"env-v1:sha256:queue\",\"verifiedFingerprint\":\"env-v1:sha256:queue\"},\"diagnostics\":[]}\\n' \"$profile\"\n  exit 0\nfi\ncandidate=$(git rev-parse HEAD)\nprintf '{\"schemaVersion\":1,\"checkId\":\"full\",\"capability\":\"test\",\"candidate\":{\"kind\":\"git-commit\",\"identity\":\"%s\"},\"outcome\":\"passed\",\"required\":true,\"startedAt\":\"2026-08-29T10:00:00Z\",\"finishedAt\":\"2026-08-29T10:00:01Z\",\"exitCode\":0,\"evidence\":[]}\n' \"$candidate\"\n",
        );
        let github = root.path().join("fake-gh");
        executable(
            &github,
            "#!/bin/sh\nset -eu\nif [ \"$1 $2\" = 'pr create' ]; then\n  printf '%s\n' 'https://github.example/owner/demo/pull/7'\nelif [ \"$1 $2\" = 'pr view' ]; then\n  sha=$(git ls-remote origin refs/heads/agent-loop/issue-3 | awk '{print $1}')\n  printf '{\"number\":7,\"url\":\"https://github.example/owner/demo/pull/7\",\"headRefName\":\"agent-loop/issue-3\",\"headRefOid\":\"%s\",\"isDraft\":false}\n' \"$sha\"\nelse\n  printf '%s\n' '[]'\nfi\n",
        );
        let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
        config.providers.codex.executable = provider.display().to_string();
        config.execution.coding_tooling_executable = tooling.display().to_string();
        config.publication.mode = PublicationMode::PullRequest;
        config.publication.github_executable = github.display().to_string();

        Self {
            _root: root,
            data,
            checkout,
            remote,
            config,
        }
    }

    fn worker(&self) -> LocalQueueWorker {
        LocalQueueWorker::new(
            &self.data,
            RegisteredProject {
                id: "demo".into(),
                repository_root: self.checkout.clone(),
            },
            self.config.clone(),
            "owner/demo",
            &self.checkout,
        )
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
