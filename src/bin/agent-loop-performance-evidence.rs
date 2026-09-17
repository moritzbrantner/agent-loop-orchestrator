use std::{fs, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

const COLLECTOR_NAME: &str = "performance-evidence.agent-loop-efficiency-adapter";
const COLLECTOR_VERSION: &str = "1.0.0";
const SCENARIO_ID: &str = "agent/implementation-attempt";
const WORKLOAD_ID: &str = "repository-task-v1";

#[derive(Debug, Parser)]
#[command(
    name = "agent-loop-performance-evidence",
    version,
    about = "Convert an agent-loop task-efficiency report into canonical per-attempt Performance Evidence"
)]
struct Args {
    /// JSON report produced by agent-loop-efficiency.
    #[arg(long)]
    report: PathBuf,
    /// Directory receiving one canonical Performance Evidence document per attempt.
    #[arg(long)]
    output_dir: PathBuf,
    /// Whether the source baseline represented by the report was dirty. Must be explicit.
    #[arg(long, required = true)]
    source_dirty: Option<bool>,
}

fn sha256_value(value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(value).context("serialize canonical hash input")?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn required_str<'a>(attempt: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    attempt
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("attempt is missing required string field {key}"))
}

fn required_u64(attempt: &Map<String, Value>, key: &str) -> Result<u64> {
    attempt
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("attempt is missing required unsigned integer field {key}"))
}

fn github_repository_uri(path: &str) -> Option<String> {
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(format!("https://github.com/{owner}/{name}"))
}

fn repository_uri(repository: Option<&Value>) -> Option<String> {
    let repository = repository?.as_str()?.trim();
    if repository.is_empty() {
        return None;
    }

    for prefix in [
        "git@github.com:",
        "ssh://git@github.com/",
        "https://github.com/",
        "http://github.com/",
        "git://github.com/",
    ] {
        if let Some(path) = repository.strip_prefix(prefix) {
            return github_repository_uri(path);
        }
    }

    if repository.starts_with("https://")
        || repository.starts_with("http://")
        || repository.starts_with("ssh://")
    {
        return Some(repository.to_owned());
    }

    github_repository_uri(repository)
}

fn measurement(name: &str, value: Value, unit: &str, measurement_type: &str) -> Value {
    json!({
        "name": name,
        "value": value,
        "unit": unit,
        "measurement_type": measurement_type,
    })
}

fn optional_number(source: Option<&Value>, field: &str) -> Result<Option<Value>> {
    let Some(source) = source else {
        return Ok(None);
    };
    if source.is_null() {
        return Ok(None);
    }
    match source {
        Value::Number(number) if number.as_f64().is_some_and(|value| value >= 0.0) => {
            Ok(Some(source.clone()))
        }
        _ => bail!("optional telemetry field {field} must be a non-negative number when present"),
    }
}

fn push_optional_measurement(
    target: &mut Vec<Value>,
    source: Option<&Value>,
    source_field: &str,
    name: &str,
    unit: &str,
    measurement_type: &str,
) -> Result<()> {
    if let Some(value) = optional_number(source, source_field)? {
        target.push(measurement(name, value, unit, measurement_type));
    }
    Ok(())
}

fn convert_attempt(attempt: &Map<String, Value>, source_dirty: bool) -> Result<Value> {
    let task_id = required_str(attempt, "taskId")?;
    let run_id = required_str(attempt, "runId")?;
    let project_id = required_str(attempt, "projectId")?;
    let baseline_sha = required_str(attempt, "baselineSha")?;
    let provider = required_str(attempt, "provider")?;
    let attempt_number = required_u64(attempt, "attemptNumber")?;
    let model = attempt.get("model").cloned().unwrap_or(Value::Null);

    let workload = json!({
        "task_id": task_id,
        "project_id": project_id,
        "repository": attempt.get("repository").cloned().unwrap_or(Value::Null),
        "baseline_revision": baseline_sha,
    });
    let environment_identity = json!({
        "environment": attempt.get("environment").cloned().unwrap_or(Value::Null),
        "provider": provider,
        "model": model,
    });

    let candidate_produced = attempt
        .get("candidateSha")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty());
    let useful_work = vec![measurement(
        "agent.candidate_produced",
        Value::from(u64::from(candidate_produced)),
        "count",
        "counter",
    )];

    let usage = attempt.get("usage").and_then(Value::as_object);
    let mut induced_work = Vec::new();
    push_optional_measurement(
        &mut induced_work,
        usage.and_then(|usage| usage.get("inputTokens")),
        "usage.inputTokens",
        "agent.input_tokens",
        "token",
        "counter",
    )?;
    push_optional_measurement(
        &mut induced_work,
        usage.and_then(|usage| usage.get("outputTokens")),
        "usage.outputTokens",
        "agent.output_tokens",
        "token",
        "counter",
    )?;
    push_optional_measurement(
        &mut induced_work,
        usage.and_then(|usage| usage.get("cachedInputTokens")),
        "usage.cachedInputTokens",
        "agent.cached_input_tokens",
        "token",
        "counter",
    )?;

    let mut outcomes = Vec::new();
    push_optional_measurement(
        &mut outcomes,
        usage.and_then(|usage| usage.get("costUsd")),
        "usage.costUsd",
        "agent.cost",
        "usd",
        "gauge",
    )?;
    push_optional_measurement(
        &mut outcomes,
        attempt.get("executionMs"),
        "executionMs",
        "agent.execution_time",
        "ms",
        "duration",
    )?;
    push_optional_measurement(
        &mut outcomes,
        attempt.get("deterministicTimeToGreenMs"),
        "deterministicTimeToGreenMs",
        "agent.deterministic_time_to_green",
        "ms",
        "duration",
    )?;

    let mut extension = Map::new();
    extension.insert("task_id".into(), Value::from(task_id));
    extension.insert("run_id".into(), Value::from(run_id));
    extension.insert("project_id".into(), Value::from(project_id));
    extension.insert("provider".into(), Value::from(provider));
    extension.insert("attempt_number".into(), Value::from(attempt_number));
    extension.insert(
        "resumed_provider_session".into(),
        Value::from(
            attempt
                .get("resumedProviderSession")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
    );
    for (source, target) in [
        ("model", "model"),
        ("outcome", "outcome"),
        ("failureReason", "failure_reason"),
        ("escalationReason", "escalation_reason"),
        ("ciRunIds", "ci_run_ids"),
        ("candidateSha", "candidate_revision"),
    ] {
        if let Some(value) = attempt.get(source).filter(|value| !value.is_null()) {
            extension.insert(target.into(), value.clone());
        }
    }

    let mut source = Map::new();
    source.insert("revision".into(), Value::from(baseline_sha));
    source.insert("dirty".into(), Value::from(source_dirty));
    if let Some(repository) = repository_uri(attempt.get("repository")) {
        source.insert("repository".into(), Value::from(repository));
    }

    let mut toolchain = Map::new();
    toolchain.insert("agent_provider".into(), Value::from(provider));
    if let Some(model) = attempt.get("model").and_then(Value::as_str) {
        toolchain.insert("agent_model".into(), Value::from(model));
    }

    Ok(json!({
        "schema_version": "1.0.0",
        "scenario": {
            "id": SCENARIO_ID,
            "description": "One coding-agent implementation attempt measured for computational cost and time-to-green.",
            "workload": {
                "id": WORKLOAD_ID,
                "hash": sha256_value(&workload)?,
                "parameters": workload,
            }
        },
        "source": Value::Object(source),
        "environment": {
            "fingerprint": sha256_value(&environment_identity)?,
            "toolchain": Value::Object(toolchain),
            "collector": {
                "name": COLLECTOR_NAME,
                "version": COLLECTOR_VERSION,
            }
        },
        "measurements": {
            "useful_work": useful_work,
            "induced_work": induced_work,
            "outcomes": outcomes,
        },
        "extensions": {
            "agent.execution": Value::Object(extension),
        }
    }))
}

fn output_name(attempt: &Map<String, Value>) -> Result<String> {
    let run_id = required_str(attempt, "runId")?;
    let attempt_number = required_u64(attempt, "attemptNumber")?;
    let safe_run_id: String = run_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect();
    Ok(format!(
        "{safe_run_id}.attempt-{attempt_number}.performance-evidence.json"
    ))
}

fn convert_report(report: &Value, source_dirty: bool) -> Result<Vec<(String, Value)>> {
    let attempts = report
        .get("attempts")
        .and_then(Value::as_array)
        .context("efficiency report must contain an attempts array")?;
    let mut converted = Vec::with_capacity(attempts.len());
    let mut seen_names = std::collections::BTreeSet::new();
    for attempt in attempts {
        let attempt = attempt
            .as_object()
            .context("every efficiency-report attempt must be an object")?;
        let name = output_name(attempt)?;
        if !seen_names.insert(name.clone()) {
            bail!("duplicate output identity: {name}");
        }
        converted.push((name, convert_attempt(attempt, source_dirty)?));
    }
    Ok(converted)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let source_dirty = args
        .source_dirty
        .context("--source-dirty true|false is required")?;
    let report: Value = serde_json::from_slice(
        &fs::read(&args.report).with_context(|| format!("read {}", args.report.display()))?,
    )
    .with_context(|| format!("parse {}", args.report.display()))?;
    let converted = convert_report(&report, source_dirty)?;

    fs::create_dir_all(&args.output_dir)
        .with_context(|| format!("create {}", args.output_dir.display()))?;
    for (name, evidence) in &converted {
        let path = args.output_dir.join(name);
        let mut rendered = serde_json::to_string_pretty(evidence).context("serialize evidence")?;
        rendered.push('\n');
        fs::write(&path, rendered).with_context(|| format!("write {}", path.display()))?;
    }
    eprintln!(
        "wrote {} canonical Performance Evidence artifact(s) to {}",
        converted.len(),
        args.output_dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str =
        include_str!("../../tests/fixtures/performance-evidence/agent-loop-efficiency-report.json");
    const EXPECTED: &str = include_str!("../../tests/fixtures/performance-evidence/expected.json");

    #[test]
    fn matches_authoritative_agent_run_fixture() {
        let report: Value = serde_json::from_str(REPORT).expect("report fixture");
        let expected: Value = serde_json::from_str(EXPECTED).expect("expected fixture");
        let converted = convert_report(&report, false).expect("convert report");
        assert_eq!(converted.len(), 1);
        assert_eq!(
            converted[0].0,
            "018f5d43-4d1c-7fd5-aed5-d451fd71c110.attempt-1.performance-evidence.json"
        );
        assert_eq!(converted[0].1, expected);
    }

    #[test]
    fn normalizes_github_repository_remote_forms() {
        for repository in [
            "moritzbrantner/physics-engine",
            "https://github.com/moritzbrantner/physics-engine.git",
            "http://github.com/moritzbrantner/physics-engine.git",
            "git://github.com/moritzbrantner/physics-engine.git",
            "git@github.com:moritzbrantner/physics-engine.git",
            "ssh://git@github.com/moritzbrantner/physics-engine.git",
        ] {
            assert_eq!(
                repository_uri(Some(&Value::from(repository))).as_deref(),
                Some("https://github.com/moritzbrantner/physics-engine")
            );
        }
    }

    #[test]
    fn provider_session_identifier_never_crosses_portable_boundary() {
        let mut report: Value = serde_json::from_str(REPORT).expect("report fixture");
        report["attempts"][0]["providerSessionId"] = Value::from("secret-provider-session");
        let converted = convert_report(&report, false).expect("convert report");
        let rendered = serde_json::to_string(&converted[0].1).expect("serialize evidence");
        assert!(!rendered.contains("providerSessionId"));
        assert!(!rendered.contains("secret-provider-session"));
    }

    #[test]
    fn missing_optional_telemetry_remains_missing() {
        let mut report: Value = serde_json::from_str(REPORT).expect("report fixture");
        let attempt = report["attempts"][0]
            .as_object_mut()
            .expect("attempt object");
        attempt.remove("executionMs");
        attempt.remove("deterministicTimeToGreenMs");
        attempt.insert("usage".into(), json!({}));
        let converted = convert_report(&report, false).expect("convert report");
        let names: Vec<_> = converted[0].1["measurements"]
            .as_object()
            .expect("measurements")
            .values()
            .flat_map(|group| group.as_array().expect("measurement group"))
            .filter_map(|entry| entry.get("name").and_then(Value::as_str))
            .collect();
        assert_eq!(names, vec!["agent.candidate_produced"]);
    }

    #[test]
    fn bulk_conversion_is_one_document_per_attempt_without_additional_ledger_reads() {
        let fixture: Value = serde_json::from_str(REPORT).expect("report fixture");
        let base = fixture["attempts"][0].clone();
        let attempts: Vec<_> = (1..=2_000_u64)
            .map(|number| {
                let mut attempt = base.clone();
                attempt["runId"] = Value::from(format!("run-{number}"));
                attempt["attemptNumber"] = Value::from(number);
                attempt
            })
            .collect();
        let report = json!({"attempts": attempts});
        let converted = convert_report(&report, false).expect("bulk conversion");
        assert_eq!(converted.len(), 2_000);
        assert_eq!(
            converted.first().expect("first").0,
            "run-1.attempt-1.performance-evidence.json"
        );
        assert_eq!(
            converted.last().expect("last").0,
            "run-2000.attempt-2000.performance-evidence.json"
        );
    }
}
