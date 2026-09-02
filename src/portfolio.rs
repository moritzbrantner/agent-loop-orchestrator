use std::{
    collections::BTreeMap,
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

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioFindingsOptions {
    pub limit: Option<usize>,
    pub new_only: bool,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioFindingsSummary {
    pub repositories_with_findings: usize,
    pub repository_statuses: BTreeMap<String, usize>,
    pub severities: BTreeMap<String, usize>,
    pub states: BTreeMap<String, usize>,
    pub expectations: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioFindingsReport {
    pub schema_version: u32,
    pub root: String,
    pub repository_count: usize,
    pub finding_count: usize,
    pub new_only: bool,
    pub summary: PortfolioFindingsSummary,
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

    if let Some(registry) = environment::load_default()?
        && let Some(component) = registry.components.get("coding-tooling")
    {
        let entrypoint = component.path.join("src").join("entry.ts");
        if entrypoint.is_file() {
            return Ok(CodingToolingInvocation {
                executable: PathBuf::from("bun"),
                prefix_args: vec![entrypoint.as_os_str().to_owned()],
                label: format!("bun {}", entrypoint.display()),
            });
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

fn collect_repository(
    tool: &CodingToolingInvocation,
    repository: &Path,
    new_only: bool,
) -> RepositoryFindings {
    let mut command = Command::new(&tool.executable);
    command.args(&tool.prefix_args).arg("findings");
    if new_only {
        command.arg("--new");
    }
    command.arg("--json").current_dir(repository);

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

fn increment(map: &mut BTreeMap<String, usize>, key: &str) {
    *map.entry(key.to_owned()).or_default() += 1;
}

fn string_field<'a>(finding: &'a Value, name: &str, fallback: &'a str) -> &'a str {
    finding.get(name).and_then(Value::as_str).unwrap_or(fallback)
}

fn summarize_repositories(repositories: &[RepositoryFindings]) -> PortfolioFindingsSummary {
    let mut summary = PortfolioFindingsSummary::default();
    for repository in repositories {
        increment(&mut summary.repository_statuses, &repository.status);
        if !repository.findings.is_empty() {
            summary.repositories_with_findings += 1;
        }
        for finding in &repository.findings {
            increment(
                &mut summary.severities,
                string_field(finding, "severity", "unknown"),
            );
            increment(
                &mut summary.states,
                string_field(finding, "state", "unknown"),
            );
            increment(
                &mut summary.expectations,
                string_field(finding, "expectationId", "unknown"),
            );
        }
    }
    summary
}

pub fn collect_findings(root: &Path, limit: Option<usize>) -> Result<PortfolioFindingsReport> {
    collect_findings_with_options(
        root,
        PortfolioFindingsOptions {
            limit,
            new_only: false,
        },
    )
}

pub fn collect_findings_with_options(
    root: &Path,
    options: PortfolioFindingsOptions,
) -> Result<PortfolioFindingsReport> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("portfolio root {} is unavailable", root.display()))?;
    let tool = resolve_coding_tooling()?;
    let mut repository_paths = discover_repositories(&root)?;
    if let Some(limit) = options.limit {
        repository_paths.truncate(limit);
    }
    let repositories = repository_paths
        .iter()
        .map(|repository| collect_repository(&tool, repository, options.new_only))
        .collect::<Vec<_>>();
    let finding_count = repositories
        .iter()
        .map(|repository| repository.findings.len())
        .sum();
    let summary = summarize_repositories(&repositories);

    Ok(PortfolioFindingsReport {
        schema_version: 1,
        root: root.display().to_string(),
        repository_count: repositories.len(),
        finding_count,
        new_only: options.new_only,
        summary,
        repositories,
    })
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }
    Ok(())
}

pub fn write_findings_report(path: &Path, report: &PortfolioFindingsReport) -> Result<()> {
    ensure_parent(path)?;
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(report)?))
        .with_context(|| format!("failed to write portfolio report {}", path.display()))
}

fn markdown_text(value: &str) -> String {
    value
        .replace('\n', " ")
        .replace('\r', " ")
        .replace('|', "\\|")
}

fn severity_rank(value: &Value) -> u8 {
    match string_field(value, "severity", "unknown") {
        "error" => 0,
        "warning" => 1,
        "info" => 2,
        _ => 3,
    }
}

fn state_rank(value: &Value) -> u8 {
    match string_field(value, "state", "unknown") {
        "new" => 0,
        "baseline" => 1,
        _ => 2,
    }
}

fn append_count_table(output: &mut String, heading: &str, values: &BTreeMap<String, usize>) {
    output.push_str(&format!("## {heading}\n\n"));
    if values.is_empty() {
        output.push_str("None.\n\n");
        return;
    }
    output.push_str("| Value | Count |\n| --- | ---: |\n");
    for (value, count) in values {
        output.push_str(&format!("| {} | {count} |\n", markdown_text(value)));
    }
    output.push('\n');
}

pub fn render_markdown_report(
    report: &PortfolioFindingsReport,
    max_findings_per_repository: usize,
) -> String {
    let mut output = String::new();
    output.push_str("# Repository gap report\n\n");
    output.push_str(&format!("Portfolio root: `{}`  \n", markdown_text(&report.root)));
    output.push_str(&format!("Repositories scanned: {}  \n", report.repository_count));
    output.push_str(&format!("Findings: {}  \n", report.finding_count));
    output.push_str(&format!(
        "Repositories with findings: {}  \n",
        report.summary.repositories_with_findings
    ));
    output.push_str(&format!(
        "Scope: {} findings.\n\n",
        if report.new_only { "new-only" } else { "all active" }
    ));

    append_count_table(
        &mut output,
        "Repository status",
        &report.summary.repository_statuses,
    );
    append_count_table(&mut output, "Severity", &report.summary.severities);
    append_count_table(&mut output, "Finding state", &report.summary.states);
    append_count_table(&mut output, "Expectation", &report.summary.expectations);

    output.push_str("## Repository details\n\n");
    for repository in &report.repositories {
        if repository.findings.is_empty() && repository.diagnostics.is_empty() {
            continue;
        }
        output.push_str(&format!("### {}\n\n", markdown_text(&repository.repository)));
        output.push_str(&format!(
            "Status: `{}`; findings: {}.\n\n",
            markdown_text(&repository.status),
            repository.findings.len()
        ));

        if !repository.diagnostics.is_empty() {
            output.push_str("Diagnostics:\n\n");
            for diagnostic in &repository.diagnostics {
                output.push_str(&format!("- {}\n", markdown_text(diagnostic)));
            }
            output.push('\n');
        }

        let mut findings = repository.findings.iter().collect::<Vec<_>>();
        findings.sort_by(|left, right| {
            severity_rank(left)
                .cmp(&severity_rank(right))
                .then_with(|| state_rank(left).cmp(&state_rank(right)))
                .then_with(|| {
                    string_field(left, "expectationId", "")
                        .cmp(string_field(right, "expectationId", ""))
                })
                .then_with(|| string_field(left, "id", "").cmp(string_field(right, "id", "")))
        });

        for finding in findings.iter().take(max_findings_per_repository) {
            let severity = string_field(finding, "severity", "unknown").to_uppercase();
            let state = string_field(finding, "state", "unknown");
            let expectation = string_field(finding, "expectationId", "unknown");
            let id = string_field(finding, "id", "unknown");
            let message = string_field(finding, "message", "finding has no message");
            let subject = finding
                .pointer("/subject/key")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            output.push_str(&format!(
                "- **{} · {} · {} · {}** — {} (`{}`)\n",
                markdown_text(&severity),
                markdown_text(state),
                markdown_text(expectation),
                markdown_text(id),
                markdown_text(message),
                markdown_text(subject)
            ));
        }
        if findings.len() > max_findings_per_repository {
            output.push_str(&format!(
                "- … {} more finding(s) omitted from this summary; see the JSON report for full evidence.\n",
                findings.len() - max_findings_per_repository
            ));
        }
        output.push('\n');
    }
    output
}

pub fn write_markdown_report(
    path: &Path,
    report: &PortfolioFindingsReport,
    max_findings_per_repository: usize,
) -> Result<()> {
    ensure_parent(path)?;
    fs::write(
        path,
        render_markdown_report(report, max_findings_per_repository),
    )
    .with_context(|| format!("failed to write portfolio Markdown report {}", path.display()))
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
                "findings": [{
                    "id": "CT-ABCDEF012345",
                    "expectationId": "source-unimplemented-stub",
                    "severity": "error",
                    "state": "new"
                }]
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
    fn summarizes_status_severity_state_and_expectation_counts() {
        let repositories = vec![
            RepositoryFindings {
                repository: "alpha".into(),
                path: "/tmp/alpha".into(),
                status: "passed".into(),
                exit_code: Some(0),
                counts: json!({}),
                findings: vec![json!({
                    "id": "CT-AAAAAAAAAAAA",
                    "expectationId": "source-debt-marker",
                    "severity": "info",
                    "state": "baseline"
                })],
                diagnostics: vec![],
            },
            RepositoryFindings {
                repository: "beta".into(),
                path: "/tmp/beta".into(),
                status: "failed".into(),
                exit_code: Some(1),
                counts: json!({}),
                findings: vec![json!({
                    "id": "CT-BBBBBBBBBBBB",
                    "expectationId": "source-unimplemented-stub",
                    "severity": "error",
                    "state": "new"
                })],
                diagnostics: vec![],
            },
        ];

        let summary = summarize_repositories(&repositories);

        assert_eq!(summary.repositories_with_findings, 2);
        assert_eq!(summary.repository_statuses.get("passed"), Some(&1));
        assert_eq!(summary.repository_statuses.get("failed"), Some(&1));
        assert_eq!(summary.severities.get("info"), Some(&1));
        assert_eq!(summary.severities.get("error"), Some(&1));
        assert_eq!(summary.states.get("new"), Some(&1));
        assert_eq!(summary.states.get("baseline"), Some(&1));
        assert_eq!(summary.expectations.get("source-debt-marker"), Some(&1));
    }

    #[test]
    fn markdown_prioritizes_errors_and_keeps_full_json_separate() {
        let repository = RepositoryFindings {
            repository: "alpha".into(),
            path: "/tmp/alpha".into(),
            status: "passed".into(),
            exit_code: Some(0),
            counts: json!({}),
            findings: vec![
                json!({
                    "id": "CT-AAAAAAAAAAAA",
                    "expectationId": "source-debt-marker",
                    "severity": "info",
                    "state": "new",
                    "message": "later",
                    "subject": { "key": "src/a.ts" }
                }),
                json!({
                    "id": "CT-BBBBBBBBBBBB",
                    "expectationId": "source-unimplemented-stub",
                    "severity": "error",
                    "state": "new",
                    "message": "first",
                    "subject": { "key": "src/b.ts" }
                }),
            ],
            diagnostics: vec![],
        };
        let report = PortfolioFindingsReport {
            schema_version: 1,
            root: "/tmp".into(),
            repository_count: 1,
            finding_count: 2,
            new_only: false,
            summary: summarize_repositories(std::slice::from_ref(&repository)),
            repositories: vec![repository],
        };

        let markdown = render_markdown_report(&report, 1);

        assert!(markdown.find("CT-BBBBBBBBBBBB").unwrap() < markdown.find("omitted").unwrap());
        assert!(!markdown.contains("CT-AAAAAAAAAAAA"));
        assert!(markdown.contains("1 more finding(s) omitted"));
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
