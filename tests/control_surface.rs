#![cfg(target_os = "linux")]

use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use agent_loop_orchestrator::config::ProjectConfig;
use tempfile::TempDir;

const PROVIDER: &str = r#"#!/bin/sh
set -eu
case "$*" in
  *'Add greeting.txt'*'tests: test'*) ;;
  *)
    printf 'worker context did not contain the stored objective and acceptance criterion\n' >&2
    exit 3
    ;;
esac
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
if [ "${1:-}" = "environment" ] && [ "${2:-}" = "verify" ]; then
  profile=${4:-default}
  printf '{"schemaVersion":1,"operation":"environment","status":"passed","durationMs":1,"data":{"action":"verify","fingerprintVersion":"environment-fingerprint-v1","profile":"%s","expectedFingerprint":"env-v1:sha256:control","verifiedFingerprint":"env-v1:sha256:control"},"diagnostics":[]}\n' "$profile"
  exit 0
fi
candidate=$(git rev-parse HEAD)
printf '{"schemaVersion":1,"checkId":"tests","capability":"test","candidate":{"kind":"git-commit","identity":"%s"},"outcome":"passed","required":true,"startedAt":"2026-08-20T20:00:00Z","finishedAt":"2026-08-20T20:00:01Z","exitCode":0,"evidence":[]}\n' "$candidate"
"#;

const BWRAP: &str = r#"#!/bin/sh
set -eu
chdir=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --setenv)
      export "$2=$3"
      shift 3
      ;;
    --chdir)
      chdir=$2
      shift 2
      ;;
    --ro-bind|--bind)
      shift 3
      ;;
    --dev|--proc)
      shift 2
      ;;
    --die-with-parent|--new-session)
      shift
      ;;
    --)
      shift
      break
      ;;
    *)
      printf 'unexpected fake bwrap argument: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done
[ -n "$chdir" ] && cd "$chdir"
exec "$@"
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
        started["data"]["run"]["status"], "awaiting_decision",
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
    assert_eq!(
        status["data"]["run"]["contract"]["checks"][0]["checkId"],
        "tests"
    );
    assert_eq!(
        status["data"]["run"]["contract"]["checks"][0]["capability"],
        "test"
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
    bin: PathBuf,
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
        let bin = data.path().join("bin");
        fs::create_dir(&bin).unwrap();
        executable(&provider, PROVIDER);
        executable(&tooling, TOOLING);
        executable(&bin.join("bwrap"), BWRAP);

        let fixture = Self {
            data,
            repository,
            provider,
            tooling,
            bin,
        };
        let init = fixture.command(&["init", "--provider", "codex"]);
        assert!(
            init.status.success(),
            "init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        assert!(
            !fixture.repository.path().join(".gitignore").exists(),
            "init must not add runtime-ignore state to the target repository"
        );

        let mut config = ProjectConfig::load(fixture.repository.path()).unwrap();
        config.providers.codex.executable = fixture.provider.display().to_string();
        config.execution.coding_tooling_executable = fixture.tooling.display().to_string();
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
            &["commit", "-m", "configure agent loop"],
        );
        fixture
    }

    fn control(&self, args: &[&str]) -> serde_json::Value {
        let mut command = vec!["control"];
        command.extend_from_slice(args);
        let output = self.command(&command);
        let stdout = String::from_utf8(output.stdout).unwrap();
        serde_json::from_str(stdout.lines().last().unwrap_or_default()).unwrap_or_else(|error| {
            panic!(
                "control output was not JSON: {error}\nstdout: {stdout}\nstderr: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }

    fn command(&self, args: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_agent-loop"))
            .args(args)
            .current_dir(self.repository.path())
            .env("XDG_DATA_HOME", self.data.path())
            .env("HOME", self.data.path())
            .env("PATH", path)
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
