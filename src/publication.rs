use std::{
    path::Path,
    process::{Command, Output},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct NewPullRequest<'a> {
    pub checkout: &'a Path,
    pub repository: &'a str,
    pub remote: &'a str,
    pub candidate_sha: &'a str,
    pub branch: &'a str,
    pub base: &'a str,
    pub title: &'a str,
    pub body: &'a str,
}

#[derive(Debug, Clone)]
pub struct PullRequestRepair<'a> {
    pub checkout: &'a Path,
    pub remote: &'a str,
    pub branch: &'a str,
    pub expected_head_sha: &'a str,
    pub candidate_sha: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestPublication {
    pub number: u64,
    pub url: String,
    pub branch: String,
    pub head_sha: String,
}

pub struct PullRequestPublisher {
    github_executable: String,
}

impl PullRequestPublisher {
    pub fn new(github_executable: impl Into<String>) -> Self {
        Self {
            github_executable: github_executable.into(),
        }
    }

    pub fn repository_slug(&self, checkout: &Path) -> Result<String> {
        let output = self.github(&["repo", "view", "--json", "nameWithOwner"], checkout)?;
        let repository: RepositoryIdentity =
            serde_json::from_slice(&output).context("parse GitHub repository identity")?;
        validate_repository_slug(&repository.name_with_owner)?;
        Ok(repository.name_with_owner)
    }

    pub fn publish_new(&self, request: NewPullRequest<'_>) -> Result<PullRequestPublication> {
        validate_repository_slug(request.repository)?;
        validate_branch(request.checkout, request.branch)?;
        ensure_commit(request.checkout, request.candidate_sha)?;

        let remote_ref = format!("refs/heads/{}", request.branch);
        match remote_sha(request.checkout, request.remote, &remote_ref)? {
            None => git_ok(
                request.checkout,
                &[
                    "push",
                    request.remote,
                    &format!("{}:{remote_ref}", request.candidate_sha),
                ],
            )?,
            Some(observed) if observed == request.candidate_sha => {}
            Some(observed) => bail!(
                "refusing to replace existing branch `{}` at {}; expected an unpublished branch or exact candidate {}",
                request.branch,
                observed,
                request.candidate_sha
            ),
        }
        verify_remote_candidate(
            request.checkout,
            request.remote,
            &remote_ref,
            request.candidate_sha,
        )?;

        let create = self.github_output(
            &[
                "pr",
                "create",
                "--repo",
                request.repository,
                "--base",
                request.base,
                "--head",
                request.branch,
                "--title",
                request.title,
                "--body",
                request.body,
            ],
            request.checkout,
        )?;
        let url = if create.status.success() {
            String::from_utf8(create.stdout)?
                .lines()
                .map(str::trim)
                .find(|line| line.starts_with("http://") || line.starts_with("https://"))
                .context("GitHub did not return the created pull request URL")?
                .to_owned()
        } else {
            self.find_open_pull_request_url(&request)?
                .with_context(|| github_failure(&self.github_executable, &create))?
        };

        let output = self.github(
            &[
                "pr",
                "view",
                &url,
                "--repo",
                request.repository,
                "--json",
                "number,url,headRefName,headRefOid,isDraft",
            ],
            request.checkout,
        )?;
        let pull_request: PublishedPullRequest =
            serde_json::from_slice(&output).context("parse created pull request")?;
        if pull_request.is_draft {
            bail!(
                "created pull request #{} is unexpectedly a draft",
                pull_request.number
            );
        }
        if pull_request.head_ref_name != request.branch
            || pull_request.head_ref_oid != request.candidate_sha
        {
            bail!(
                "created pull request #{} does not point at exact candidate {} on branch `{}`",
                pull_request.number,
                request.candidate_sha,
                request.branch
            );
        }
        Ok(PullRequestPublication {
            number: pull_request.number,
            url: pull_request.url,
            branch: request.branch.into(),
            head_sha: request.candidate_sha.into(),
        })
    }

    pub fn publish_repair(&self, request: PullRequestRepair<'_>) -> Result<String> {
        validate_branch(request.checkout, request.branch)?;
        ensure_commit(request.checkout, request.expected_head_sha)?;
        ensure_commit(request.checkout, request.candidate_sha)?;
        let ancestor = Command::new("git")
            .args([
                "merge-base",
                "--is-ancestor",
                request.expected_head_sha,
                request.candidate_sha,
            ])
            .current_dir(request.checkout)
            .output()
            .context("verify repair candidate ancestry")?;
        if !ancestor.status.success() {
            bail!(
                "repair candidate {} is not a descendant of observed pull request head {}",
                request.candidate_sha,
                request.expected_head_sha
            );
        }

        let remote_ref = format!("refs/heads/{}", request.branch);
        let observed = remote_sha(request.checkout, request.remote, &remote_ref)?
            .context("pull request branch is missing before repair publication")?;
        if observed != request.expected_head_sha {
            bail!(
                "pull request head moved before publication: expected {}, observed {observed}",
                request.expected_head_sha
            );
        }
        git_ok(
            request.checkout,
            &[
                "push",
                request.remote,
                &format!("{}:{remote_ref}", request.candidate_sha),
                &format!(
                    "--force-with-lease={remote_ref}:{}",
                    request.expected_head_sha
                ),
            ],
        )?;
        verify_remote_candidate(
            request.checkout,
            request.remote,
            &remote_ref,
            request.candidate_sha,
        )?;
        Ok(request.candidate_sha.into())
    }

    fn find_open_pull_request_url(&self, request: &NewPullRequest<'_>) -> Result<Option<String>> {
        let output = self.github(
            &[
                "pr",
                "list",
                "--repo",
                request.repository,
                "--state",
                "open",
                "--head",
                request.branch,
                "--json",
                "url,headRefOid",
            ],
            request.checkout,
        )?;
        let matches: Vec<ExistingPullRequest> =
            serde_json::from_slice(&output).context("parse existing pull requests")?;
        Ok(matches
            .into_iter()
            .find(|pull_request| pull_request.head_ref_oid == request.candidate_sha)
            .map(|pull_request| pull_request.url))
    }

    fn github(&self, arguments: &[&str], checkout: &Path) -> Result<Vec<u8>> {
        let output = self.github_output(arguments, checkout)?;
        if !output.status.success() {
            bail!(github_failure(&self.github_executable, &output));
        }
        Ok(output.stdout)
    }

    fn github_output(&self, arguments: &[&str], checkout: &Path) -> Result<Output> {
        Command::new(&self.github_executable)
            .args(arguments)
            .current_dir(checkout)
            .output()
            .with_context(|| format!("run {} {}", self.github_executable, arguments.join(" ")))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryIdentity {
    name_with_owner: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishedPullRequest {
    number: u64,
    url: String,
    head_ref_name: String,
    head_ref_oid: String,
    is_draft: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExistingPullRequest {
    url: String,
    head_ref_oid: String,
}

fn validate_repository_slug(value: &str) -> Result<()> {
    let Some((owner, repository)) = value.split_once('/') else {
        bail!("GitHub repository must use OWNER/REPOSITORY format");
    };
    if owner.is_empty()
        || repository.is_empty()
        || repository.contains('/')
        || [owner, repository].iter().any(|segment| {
            !segment.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
        })
    {
        bail!("GitHub repository must use OWNER/REPOSITORY format");
    }
    Ok(())
}

fn validate_branch(checkout: &Path, branch: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["check-ref-format", "--branch", branch])
        .current_dir(checkout)
        .output()
        .context("validate publication branch")?;
    if !status.status.success() {
        bail!("invalid publication branch `{branch}`");
    }
    Ok(())
}

fn ensure_commit(checkout: &Path, revision: &str) -> Result<()> {
    git(checkout, &["rev-parse", &format!("{revision}^{{commit}}")])?;
    Ok(())
}

fn remote_sha(checkout: &Path, remote: &str, remote_ref: &str) -> Result<Option<String>> {
    let output = git(checkout, &["ls-remote", remote, remote_ref])?;
    Ok(output
        .split_whitespace()
        .next()
        .filter(|value| !value.is_empty())
        .map(str::to_owned))
}

fn verify_remote_candidate(
    checkout: &Path,
    remote: &str,
    remote_ref: &str,
    candidate_sha: &str,
) -> Result<()> {
    let published = remote_sha(checkout, remote, remote_ref)?
        .context("published branch is missing from the remote")?;
    if published != candidate_sha {
        bail!("published branch points at {published}, not exact candidate {candidate_sha}");
    }
    Ok(())
}

fn git(checkout: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(checkout)
        .output()
        .with_context(|| format!("run git {}", arguments.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}

fn git_ok(checkout: &Path, arguments: &[&str]) -> Result<()> {
    let _ = git(checkout, arguments)?;
    Ok(())
}

fn github_failure(executable: &str, output: &Output) -> String {
    format!(
        "{executable} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

#[cfg(all(test, unix))]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn new_pull_request_pushes_and_records_the_exact_candidate() {
        let fixture = Fixture::new();
        let candidate = fixture.commit("candidate.txt", "candidate\n", "candidate");
        let log = fixture.root.path().join("gh.log");
        let github = fixture.root.path().join("fake-gh");
        executable(
            &github,
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1 $2\" = 'pr create' ]; then\n  printf '%s\\n' 'https://github.example/owner/demo/pull/7'\nelif [ \"$1 $2\" = 'pr view' ]; then\n  printf '%s\\n' '{{\"number\":7,\"url\":\"https://github.example/owner/demo/pull/7\",\"headRefName\":\"agent-loop/issue-3\",\"headRefOid\":\"{}\",\"isDraft\":false}}'\nelse\n  printf '%s\\n' '[]'\nfi\n",
                log.display(),
                candidate
            ),
        );
        let publisher = PullRequestPublisher::new(github.display().to_string());

        let publication = publisher
            .publish_new(NewPullRequest {
                checkout: &fixture.checkout,
                repository: "owner/demo",
                remote: "origin",
                candidate_sha: &candidate,
                branch: "agent-loop/issue-3",
                base: "main",
                title: "Implement issue",
                body: "Closes #3",
            })
            .unwrap();

        assert_eq!(publication.number, 7);
        assert_eq!(publication.head_sha, candidate);
        assert_eq!(
            git_bare(
                &fixture.remote,
                &["rev-parse", "refs/heads/agent-loop/issue-3"]
            ),
            candidate
        );
        let calls = fs::read_to_string(log).unwrap();
        assert!(calls.contains("pr create"));
        assert!(!calls.contains("--draft"));
        assert!(!calls.contains("pr merge"));
    }

    #[test]
    fn repair_refuses_to_overwrite_a_head_that_moved() {
        let fixture = Fixture::new();
        let old_head = fixture.commit("feature.txt", "old\n", "feature");
        git_ok_test(
            &fixture.checkout,
            &["push", "origin", &format!("{old_head}:refs/heads/feature")],
        );
        let candidate = fixture.commit("repair.txt", "repair\n", "repair");
        let moved_head = fixture.commit("interference.txt", "moved\n", "interference");
        git_ok_test(
            &fixture.checkout,
            &[
                "push",
                "origin",
                &format!("{moved_head}:refs/heads/feature"),
                "--force",
            ],
        );
        let publisher = PullRequestPublisher::new("unused-gh");

        let error = publisher
            .publish_repair(PullRequestRepair {
                checkout: &fixture.checkout,
                remote: "origin",
                branch: "feature",
                expected_head_sha: &old_head,
                candidate_sha: &candidate,
            })
            .unwrap_err();

        assert!(format!("{error:#}").contains("head moved before publication"));
        assert_eq!(
            git_bare(&fixture.remote, &["rev-parse", "refs/heads/feature"]),
            moved_head
        );
    }

    struct Fixture {
        root: TempDir,
        checkout: PathBuf,
        remote: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = TempDir::new().unwrap();
            let checkout = root.path().join("checkout");
            let remote = root.path().join("remote.git");
            fs::create_dir_all(&checkout).unwrap();
            git_ok_test(&checkout, &["init", "-b", "main"]);
            git_ok_test(&checkout, &["config", "user.name", "Publication Test"]);
            git_ok_test(
                &checkout,
                &["config", "user.email", "publication@example.test"],
            );
            fs::write(checkout.join("README.md"), "base\n").unwrap();
            git_ok_test(&checkout, &["add", "README.md"]);
            git_ok_test(&checkout, &["commit", "-m", "base"]);
            let output = Command::new("git")
                .args(["init", "--bare"])
                .arg(&remote)
                .output()
                .unwrap();
            assert!(output.status.success());
            git_ok_test(
                &checkout,
                &["remote", "add", "origin", &remote.display().to_string()],
            );
            git_ok_test(&checkout, &["push", "origin", "main"]);
            Self {
                root,
                checkout,
                remote,
            }
        }

        fn commit(&self, path: &str, contents: &str, message: &str) -> String {
            fs::write(self.checkout.join(path), contents).unwrap();
            git_ok_test(&self.checkout, &["add", path]);
            git_ok_test(&self.checkout, &["commit", "-m", message]);
            git_test(&self.checkout, &["rev-parse", "HEAD"])
        }
    }

    fn executable(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn git_test(checkout: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(checkout)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }

    fn git_ok_test(checkout: &Path, arguments: &[&str]) {
        let _ = git_test(checkout, arguments);
    }

    fn git_bare(repository: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .arg("--git-dir")
            .arg(repository)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git --git-dir {} {arguments:?}: {}",
            repository.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
}
