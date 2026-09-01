#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

use agent_loop_orchestrator::{
    adapters::Provider, config::ProjectConfig, queue::GitHubQueuePlatform,
    repository::RegisteredProject,
};
use tempfile::TempDir;

#[test]
fn queue_uses_registered_repository_instead_of_creating_a_private_clone() {
    let root = TempDir::new().unwrap();
    let repository = root.path().join("repository");
    let data = root.path().join("data");
    fs::create_dir_all(&repository).unwrap();
    git_ok(&repository, &["init", "-b", "main"]);
    git_ok(
        &repository,
        &["config", "user.name", "Queue Workspace Test"],
    );
    git_ok(
        &repository,
        &["config", "user.email", "queue-workspace@example.test"],
    );
    fs::write(repository.join("README.md"), "baseline\n").unwrap();
    git_ok(&repository, &["add", "README.md"]);
    git_ok(&repository, &["commit", "-m", "baseline"]);

    let github = root.path().join("fake-gh");
    executable(
        &github,
        "#!/bin/sh\nset -eu\nif [ \"$1 $2\" = 'repo view' ]; then\n  printf '%s\\n' '{\"nameWithOwner\":\"owner/demo\"}'\n  exit 0\nfi\nexit 99\n",
    );
    let mut config = ProjectConfig::default_for("demo".into(), Provider::Codex);
    config.publication.github_executable = github.display().to_string();
    let project = RegisteredProject {
        id: "demo".into(),
        repository_root: repository.clone(),
    };

    let platform = GitHubQueuePlatform::prepare(&data, &project, &config).unwrap();

    assert_eq!(platform.checkout(), repository);
    assert!(!data.join("queue-checkouts").exists());
}

fn executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn git_ok(root: &Path, args: &[&str]) {
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
}
