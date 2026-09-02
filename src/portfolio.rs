use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};

use crate::environment;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioFindingsReport {
    pub schema_version: u32,
    pub root: String,
    pub repository_count: usize,
    pub finding_count: usize,
    pub repositories: Vec<RepositoryFindings>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFindings {
    pub repository: String,
    pub path: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub counts: Value,
    pub findings: Vec<Value>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug)]
struct CodingToolingInvocation {
    executable: PathBuf,
    prefix_args: Vec<OsString>,
    label: String,
}

fn repository_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("repository")
        .to_owned()
}

pub fn discover_repositories(root: &Path) -> Result<Vec<PathBuf>> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("portfolio root {} is unavailable", root.display()))?;
    let mut repositories = Vec::new();
    if root.join(".git").exists() {
        repositories.push(root.clone());
    }

    for entry in fs::read_dir(&root)
        .with_context(|| format!("failed to read portfolio root {}", root.display()))?
    {
        let path = entry?.path();
        if path.is_dir() && path.join(".git").exists() {
            repositories.push(path);
        }
    }

    repositories.sort_by_key(|path| repository_name(path));
    repositories.dedup();
    Ok(repositories)
}

fn resolve_coding_tooling() -> Result<CodingToolingInvocation> {
    if let Some(executable) = env::var_os("CODING_TOOLING_BIN") {
        let executable = PathBuf::from(executable);
        return Ok(CodingToolingInvocation {
            label: executable.display().to_string(),
            executable,
            prefix_args: Vec::new(),
        });
    }

    if let Some(executable) = environment::find_on_path(OsStr::new("coding-tooling")) {
        return Ok(CodingToolingInvocation {
            label: executable.display().to_string(),
            executable,
            prefix_args: Vec::new(),
        });
    }

    if let Some(registry) = environment::load_default()? {
        if let Some(component) = registry.components.get("coding-tooling") {
            let entrypoint = component.path.join("src").join("entry.ts");
            if entrypoint.is_file() {
                return Ok(CodingToolingInvocation {
                    executable: PathBuf::from("bun"),
                    prefix_args: vec![entrypoint.as_os_str().to_owned()],
                    label: format!("bun {}", entrypoint.display()),
                });
            }
        }
    }

    Ok(CodingToolingInvocation {
        executable: PathBuf::from("coding-tooling"),
        prefix_args: Vec::new(),
        label: "coding-tooling".into(),
    })
}

fn parse_findings_output(
    path: &Path,
    exit_code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> RepositoryFindings {
    let repository = repository_name(path);
    match serde_json::from_slice::<Value>(stdout) {
        Ok(envelope) => {
            let status = envelope
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let counts = envelope
                .pointer("/data/counts")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let findings = envelope
                .pointer("/data/findings")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut diagnostics = envelope
                .get("diagnostics")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|diagnostic| {
                    diagnostic
                        .get("message")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>();
            let stderr = String::from_utf8_lossy(stderr).trim().to_owned();
            if !stderr.is_empty() {
                diagnostics.push(stderr);
            }
            RepositoryFindings {
                repository,
                path: path.display().to_string(),
                status,
                exit_code,
                counts,
                findings,
                diagnostics,
            }
        }
        Err(error) => {
            let mut diagnostics = vec![format!("coding-tooling returned invalid JSON: {error}")];
            let stderr = String::from_utf8_lossy(stderr).trim().to_owned();
            if !stderr.is_empty() {
                diagnostics.push(stderr);
            }
            RepositoryFindings {
                repository,
                path: path.display().to_string(),
                status: "error".into(),
                exit_code,
                counts: json!({}),
                findings: Vec::new(),
                diagnostics,
            }
        }
    }
}

fn collect_repository(tool: &CodingToolingInvocation, repository: &Path) -> RepositoryFindings {
    let mut command = Command::new(&tool.executable);
    command
        .args(&tool.prefix_args)
        .args(["findings", "--json"])
        .current_dir(repository);

    match command.output() {
        Ok(output) => parse_findings_output(
            repository,
            output.status.code(),
            &output.stdout,
            &output.stderr,
        ),
        Err(error) => RepositoryFindings {
            repository: repository_name(repository),
            path: repository.display().to_string(),
            status: "unavailable".into(),
            exit_code: None,
            counts: json!({}),
            findings: Vec::new(),
            diagnostics: vec![format!("failed to execute {}: {error}", tool.label)],
        },
    }
}

pub fn collect_findings(root: &Path, limit: Option<usize>) -> Result<PortfolioFindingsReport> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("portfolio root {} is unavailable", root.display()))?;
    let tool = resolve_coding_tooling()?;
    let mut repositories = discover_repositories(&root)?;
    if let Some(limit) = limit {
        repositories.truncate(limit);
    }
    let repositories = repositories
        .iter()
        .map(|repository| collect_repository(&tool, repository))
        .collect::<Vec<_>>();
    let finding_count = repositories
        .iter()
        .map(|repository| repository.findings.len())
        .sum();

    Ok(PortfolioFindingsReport {
        schema_version: 1,
        root: root.display().to_string(),
        repository_count: repositories.len(),
        finding_count,
        repositories,
    })
}

pub fn write_findings_report(path: &Path, report: &PortfolioFindingsReport) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(report)?))
        .with_context(|| format!("failed to write portfolio report {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn discovers_only_git_repositories_in_stable_order() {
        let root = tempdir().unwrap();
        for name in ["zeta", "alpha"] {
            fs::create_dir_all(root.path().join(name).join(".git")).unwrap();
        }
        fs::create_dir_all(root.path().join("not-a-repo")).unwrap();

        let repositories = discover_repositories(root.path()).unwrap();
        let names = repositories
            .iter()
            .map(|path| repository_name(path))
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["alpha", "zeta"]);
    }

    #[test]
    fn preserves_findings_and_blocking_exit_codes_from_valid_json() {
        let root = tempdir().unwrap();
        let envelope = json!({
            "schemaVersion": 1,
            "status": "failed",
            "data": {
                "counts": { "total": 1, "error": 1 },
                "findings": [{ "id": "CT-ABCDEF012345", "severity": "error" }]
            },
            "diagnostics": []
        });

        let parsed = parse_findings_output(
            root.path(),
            Some(1),
            serde_json::to_string(&envelope).unwrap().as_bytes(),
            b"",
        );

        assert_eq!(parsed.status, "failed");
        assert_eq!(parsed.exit_code, Some(1));
        assert_eq!(parsed.findings.len(), 1);
    }

    #[test]
    fn turns_invalid_tool_output_into_repository_diagnostics() {
        let root = tempdir().unwrap();
        let parsed = parse_findings_output(root.path(), Some(2), b"not-json", b"tool failed");

        assert_eq!(parsed.status, "error");
        assert!(parsed.findings.is_empty());
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|value| value.contains("invalid JSON"))
        );
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|value| value.contains("tool failed"))
        );
    }
}
