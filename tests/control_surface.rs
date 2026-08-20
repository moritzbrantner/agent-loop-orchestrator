#![cfg(target_os = "linux")]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use agent_loop_orchestrator::config::ProjectConfig;
use tempfile::TempDir;

const PROVIDER: &str = r#"#!/bin/sh
set -eu
if [ -f greeting.txt ]; then
  printf 'again\n' >> greeting.txt
else
  printf 'hello from provider\n' > greeting.txt
fi
git add greeting.txt
git commit -m 'candidate' >/dev/null
printf '%s\n' '{"type":"thread.started","thread_id":"fake-thread"}'
"#;

const TOOLING: &str = r#"#!/bin/sh
set -eu
candidate=$(git rev-parse HEAD)
printf '{"schemaVersion":1,"checkId":"fake-check","capability":"test","candidate":{"kind":"git-commit","identity":"%s"},"outcome":"passed","required":true,"startedAt":"2026-08-20T20:00:00Z","finishedAt":"2026-08-20T20:00:01Z","exitCode":0,"evidence":[]}\n' "$candidate"
"#;

#[test]
fn control_surface_covers_bounded_work_readiness_run_decision_and_resume() {
    let fixture = Fixture::new();

    let foundation = fixture.control(&[
        "work-item",
        "create",
        "--title",
        "Foundation",
        "--objective",
        "Add greeting.txt",
        "--acceptance",
        "tests=test",
        "--scope",
        "greeting.txt",
    ]);
    assert_eq!(foundation["schemaVersion"], 1);
    assert_eq!(foundation["ok"], true);
    assert_eq!(foundation["data"]["readiness"], "ready");
    assert_eq!(foundation["data"]["acceptance"][0]["id"], "tests");
    let foundation_id = foundation["data"]["workItem"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let dependent = fixture.control(&[
        "work-item",
        "create",
        "--title",
        "Dependent",
        "--objective",
        "Use the foundation",
        "--dependency",
        &foundation_id,
        "--acceptance",
        "tests=test",
        "--scope",
        "web",
    ]);
    let dependent_id = dependent["data"]["workItem"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(dependent["data"]["readiness"], "blocked");
    assert_eq!(
        dependent["data"]["blockers"][0]["workItemId"],
        foundation_id
    );

    let blocked = fixture.control(&["start", &dependent_id, "--provider", "codex"]);
    assert_eq!(blocked["ok"], false);
    assert_eq!(blocked["error"]["code"], "dependency_blocked");

    let started = fixture.control(&["start", &foundation_id, "--provider", "codex"]);
    assert_eq!(started["ok"], true);
    assert_eq!(
        started["data"]["run"]["status"],
        "awaiting_decision",
        "unexpected run payload: {started:#}"
    );
    let run_id = started["data"]["run"]["id"].as_str().unwrap().to_owned();
    let candidate = started["data"]["run"]["contract"]["candidates"][0]["gitSha"]
        .as_str()
        .unwrap()
        .to_owned();

    let status = fixture.control(&["status", &run_id]);
    assert_eq!(status["kind"], "run");
    assert_eq!(
        status["data"]["run"]["contract"]["baseline"]["gitSha"],
        started["data"]["run"]["contract"]["baseline"]["gitSha"]
    );
    assert_eq!(
        status["data"]["run"]["contract"]["checks"][0]["outcome"],
        "passed"
    );

    let approved = fixture.control(&["approve", &run_id]);
    assert_eq!(approved["data"]["run"]["status"], "completed");
    assert_eq!(
        approved["data"]["run"]["contract"]["decisions"][0]["candidateIdentity"],
        candidate
    );
    assert_eq!(
        git(fixture.repository.path(), &["rev-parse", "main"]),
        candidate
    );

    let ready = fixture.control(&["status", &dependent_id]);
    assert_eq!(ready["data"]["readiness"], "ready");
    assert!(ready["data"]["blockers"].as_array().unwrap().is_empty());

    let resumed = fixture.control(&["resume", &run_id]);
    assert_eq!(resumed["ok"], true);
    assert_eq!(resumed["data"]["run"]["status"], "awaiting_decision");
    assert_eq!(resumed["data"]["workItem"]["objective"], "Add greeting.txt");
    assert_eq!(
        resumed["data"]["run"]["contract"]["attempts"][0]["providerSessionId"],
        "fake-thread"
    );
}

struct Fixture {
    data: TempDir,
    repository: TempDir,
    provider: PathBuf,
    tooling: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let repository = tempfile::tempdir().unwrap();
        git_ok(repository.path(), &["init", "-b", "main"]);
        git_ok(
            repository.path(),
            &["config", "user.name", "Agent Loop Control Test"],
        );
        git_ok(
            repository.path(),
            &["config", "user.email", "control@example.test"],
        );
        fs::write(repository.path().join("README.md"), "baseline\n").unwrap();
        git_ok(repository.path(), &["add", "README.md"]);
        git_ok(repository.path(), &["commit", "-m", "baseline"]);

        let provider = data.path().join("fake-codex");
        let tooling = data.path().join("fake-coding-tooling");
        executable(&provider, PROVIDER);
        executable(&tooling, TOOLING);

        let fixture = Self {
            data,
            repository,
            provider,
            tooling,
        };
        let init = fixture.command(&["init", "--provider", "codex"]);
        assert!(
            init.status.success(),
            "init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );

        let mut config = ProjectConfig::load(fixture.repository.path()).unwrap();
        config.providers.codex.executable = fixture.provider.display().to_string();
        config.execution.coding_tooling_executable = fixture.tooling.display().to_string();
        fs::write(
            fixture.repository.path().join(".agent-loop/config.toml"),
            config.to_toml().unwrap(),
        )
        .unwrap();
        fixture
    }

    fn control(&self, args: &[&str]) -> serde_json::Value {
        let mut command = vec!["control"];
        command.extend_from_slice(args);
        let output = self.command(&command);
        let stdout = String::from_utf8(output.stdout).unwrap();
        serde_json::from_str(stdout.trim()).unwrap_or_else(|error| {
            panic!(
                "control output was not JSON: {error}\nstdout: {stdout}\nstderr: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }

    fn command(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_agent-loop"))
            .args(args)
            .current_dir(self.repository.path())
            .env("XDG_DATA_HOME", self.data.path())
            .env("HOME", self.data.path())
            .output()
            .unwrap()
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
