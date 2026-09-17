use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

use agent_loop_orchestrator::{
    contracts::{CheckOutcome, Evidence},
    execution::ExecutionService,
    repository,
};
use anyhow::{Context, Result};
use chrono::{DateTime, TimeDelta, Utc};
use clap::Parser;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "agent-loop-efficiency",
    version,
    about = "Export task-efficiency evidence from the local agent-loop execution ledger"
)]
struct Args {
    /// Agent-loop data root. Defaults to the orchestrator's normal local data directory.
    #[arg(long)]
    data_root: Option<PathBuf>,
    /// Inclusive RFC3339 start time. Defaults to seven days before --until.
    #[arg(long)]
    since: Option<String>,
    /// Inclusive RFC3339 end time. Defaults to now.
    #[arg(long)]
    until: Option<String>,
    /// Optional path for the JSON report. The report is always printed to stdout.
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EfficiencyReport {
    schema_version: u8,
    kind: &'static str,
    generated_at: DateTime<Utc>,
    window: Window,
    summary: Summary,
    attempts: Vec<AttemptEvidence>,
    telemetry_gaps: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Window {
    since: DateTime<Utc>,
    until: DateTime<Utc>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    run_count: usize,
    agent_call_count: usize,
    provider_counts: BTreeMap<String, usize>,
    model_counts: BTreeMap<String, usize>,
    outcome_counts: BTreeMap<String, usize>,
    mean_agent_execution_ms: Option<u64>,
    median_agent_execution_ms: Option<u64>,
    mean_deterministic_time_to_green_ms: Option<u64>,
    median_deterministic_time_to_green_ms: Option<u64>,
    resumed_session_count: usize,
    environment_verified_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AttemptEvidence {
    task_id: Uuid,
    run_id: Uuid,
    attempt_id: String,
    project_id: String,
    repository: Option<String>,
    baseline_sha: String,
    candidate_sha: Option<String>,
    provider: String,
    model: Option<String>,
    provider_session_id: Option<String>,
    resumed_provider_session: bool,
    attempt_number: u32,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    execution_ms: Option<u64>,
    outcome: String,
    failure_reason: Option<String>,
    escalation_reason: Option<String>,
    environment: Option<EnvironmentFingerprint>,
    deterministic_checks: Vec<DeterministicCheck>,
    deterministic_time_to_green_ms: Option<u64>,
    ci_run_ids: Vec<u64>,
    usage: UsageEvidence,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentFingerprint {
    fingerprint_version: Option<String>,
    profile: Option<String>,
    expected: Option<String>,
    verified: Option<String>,
    matched: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeterministicCheck {
    check_id: String,
    capability: String,
    required: bool,
    outcome: String,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    duration_ms: Option<u64>,
    reason: Option<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageEvidence {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    total_tokens: Option<u64>,
    cost_usd: Option<f64>,
    source: Option<&'static str>,
}

fn parse_time(
    value: Option<String>,
    label: &str,
    fallback: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    match value {
        Some(value) => DateTime::parse_from_rfc3339(&value)
            .map(|value| value.with_timezone(&Utc))
            .with_context(|| format!("parse {label} as RFC3339")),
        None => Ok(fallback),
    }
}

fn enum_name(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn elapsed_ms(started_at: DateTime<Utc>, finished_at: Option<DateTime<Utc>>) -> Option<u64> {
    let finished_at = finished_at?;
    let duration = finished_at
        .signed_duration_since(started_at)
        .num_milliseconds();
    u64::try_from(duration).ok()
}

fn environment_fingerprint(data_root: &Path, run_id: Uuid) -> Option<EnvironmentFingerprint> {
    let path = data_root
        .join("runs")
        .join(run_id.to_string())
        .join("environment-verification.json");
    let payload: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let data = payload.get("data")?;
    let expected = data
        .get("expectedFingerprint")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let verified = data
        .get("verifiedFingerprint")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(EnvironmentFingerprint {
        fingerprint_version: data
            .get("fingerprintVersion")
            .and_then(Value::as_str)
            .map(str::to_owned),
        profile: data
            .get("profile")
            .and_then(Value::as_str)
            .map(str::to_owned),
        matched: expected
            .as_ref()
            .zip(verified.as_ref())
            .map(|(expected, verified)| expected == verified),
        expected,
        verified,
    })
}

fn normalize_usage_key(key: &str) -> String {
    key.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn update_u64(target: &mut Option<u64>, value: &Value) {
    if let Some(value) = value.as_u64() {
        *target = Some(target.map_or(value, |current| current.max(value)));
    }
}

fn update_f64(target: &mut Option<f64>, value: &Value) {
    if let Some(value) = value.as_f64() {
        *target = Some(target.map_or(value, |current| current.max(value)));
    }
}

fn collect_usage(value: &Value, usage: &mut UsageEvidence) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                match normalize_usage_key(key).as_str() {
                    "inputtokens" | "prompttokens" => update_u64(&mut usage.input_tokens, value),
                    "outputtokens" | "completiontokens" => {
                        update_u64(&mut usage.output_tokens, value);
                    }
                    "cachedinputtokens" | "cachedprompttokens" => {
                        update_u64(&mut usage.cached_input_tokens, value);
                    }
                    "totaltokens" => update_u64(&mut usage.total_tokens, value),
                    "costusd" | "totalcostusd" => update_f64(&mut usage.cost_usd, value),
                    _ => {}
                }
                collect_usage(value, usage);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_usage(value, usage);
            }
        }
        _ => {}
    }
}

fn usage_evidence(data_root: &Path, run_id: Uuid) -> UsageEvidence {
    let path = data_root
        .join("runs")
        .join(run_id.to_string())
        .join("events.jsonl");
    let Ok(contents) = fs::read_to_string(path) else {
        return UsageEvidence::default();
    };
    let mut usage = UsageEvidence::default();
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            collect_usage(&value, &mut usage);
        }
    }
    if usage.input_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.cached_input_tokens.is_some()
        || usage.total_tokens.is_some()
        || usage.cost_usd.is_some()
    {
        usage.source = Some("max-observed-provider-event-value");
    }
    usage
}

fn ci_run_ids_from_evidence<'a>(evidence: impl IntoIterator<Item = &'a Evidence>) -> Vec<u64> {
    let mut ids = BTreeSet::new();
    for evidence in evidence {
        let mut remaining = evidence.uri.as_str();
        while let Some(index) = remaining.find("actions/runs/") {
            remaining = &remaining[index + "actions/runs/".len()..];
            let digits: String = remaining
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect();
            if let Ok(id) = digits.parse::<u64>() {
                ids.insert(id);
            }
            if digits.is_empty() || digits.len() >= remaining.len() {
                break;
            }
            remaining = &remaining[digits.len()..];
        }
    }
    ids.into_iter().collect()
}

fn deterministic_time_to_green_ms(
    run_started_at: DateTime<Utc>,
    checks: &[agent_loop_orchestrator::contracts::CheckResult],
) -> Option<u64> {
    let required: Vec<_> = checks.iter().filter(|check| check.required).collect();
    if required.is_empty()
        || required
            .iter()
            .any(|check| check.outcome != CheckOutcome::Passed)
    {
        return None;
    }
    let green_at = required.iter().map(|check| check.finished_at).max()?;
    u64::try_from(
        green_at
            .signed_duration_since(run_started_at)
            .num_milliseconds(),
    )
    .ok()
}

fn median(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        Some((values[middle - 1] + values[middle]) / 2)
    } else {
        Some(values[middle])
    }
}

fn mean(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let total = values.iter().map(|value| u128::from(*value)).sum::<u128>();
    u64::try_from(total / values.len() as u128).ok()
}

fn main() -> Result<()> {
    let args = Args::parse();
    let until = parse_time(args.until, "--until", Utc::now())?;
    let since = parse_time(args.since, "--since", until - TimeDelta::days(7))?;
    anyhow::ensure!(since <= until, "--since must not be after --until");
    let data_root = match args.data_root {
        Some(path) => path,
        None => repository::data_directory()?,
    };
    let snapshot = ExecutionService::load(&data_root)?.snapshot();
    let work_items: HashMap<_, _> = snapshot
        .work_items
        .iter()
        .map(|item| (item.id, item))
        .collect();
    let mut session_counts = HashMap::<String, usize>::new();
    for run in &snapshot.runs {
        for attempt in &run.contract.attempts {
            if let Some(session_id) = attempt.provider_session_id.as_ref() {
                *session_counts.entry(session_id.clone()).or_default() += 1;
            }
        }
    }

    let mut attempts = Vec::new();
    for run in &snapshot.runs {
        let Some(work_item) = work_items.get(&run.work_item_id) else {
            continue;
        };
        for attempt in &run.contract.attempts {
            if attempt.started_at < since || attempt.started_at > until {
                continue;
            }
            let checks = run
                .contract
                .checks
                .iter()
                .map(|check| DeterministicCheck {
                    check_id: check.check_id.clone(),
                    capability: check.capability.clone(),
                    required: check.required,
                    outcome: enum_name(&check.outcome),
                    started_at: check.started_at,
                    finished_at: check.finished_at,
                    duration_ms: check.duration_ms,
                    reason: check.reason.clone(),
                })
                .collect();
            let mut evidence: Vec<&Evidence> = attempt.evidence.iter().collect();
            evidence.extend(
                run.contract
                    .checks
                    .iter()
                    .flat_map(|check| check.evidence.iter()),
            );
            evidence.extend(
                run.contract
                    .publications
                    .iter()
                    .flat_map(|publication| publication.evidence.iter()),
            );
            let provider_session_id = attempt.provider_session_id.clone();
            attempts.push(AttemptEvidence {
                task_id: work_item.id,
                run_id: run.id,
                attempt_id: attempt.attempt_id.clone(),
                project_id: run.project_id.clone(),
                repository: run.contract.project.repository.clone(),
                baseline_sha: run.contract.baseline.git_sha.clone(),
                candidate_sha: run
                    .contract
                    .candidates
                    .last()
                    .and_then(|candidate| candidate.git_sha.clone()),
                provider: run.provider.to_string(),
                model: run.contract.agent.model.clone(),
                resumed_provider_session: provider_session_id.as_ref().is_some_and(|session_id| {
                    session_counts.get(session_id).copied().unwrap_or(0) > 1
                }),
                provider_session_id,
                attempt_number: attempt.number,
                started_at: attempt.started_at,
                finished_at: attempt.finished_at,
                execution_ms: elapsed_ms(attempt.started_at, attempt.finished_at),
                outcome: enum_name(&attempt.outcome),
                failure_reason: run.error.clone(),
                escalation_reason: None,
                environment: environment_fingerprint(&data_root, run.id),
                deterministic_checks: checks,
                deterministic_time_to_green_ms: deterministic_time_to_green_ms(
                    run.started_at,
                    &run.contract.checks,
                ),
                ci_run_ids: ci_run_ids_from_evidence(evidence),
                usage: usage_evidence(&data_root, run.id),
            });
        }
    }
    attempts.sort_by_key(|attempt| attempt.started_at);

    let agent_durations: Vec<_> = attempts
        .iter()
        .filter_map(|attempt| attempt.execution_ms)
        .collect();
    let green_durations: Vec<_> = attempts
        .iter()
        .filter_map(|attempt| attempt.deterministic_time_to_green_ms)
        .collect();
    let mut summary = Summary {
        run_count: attempts
            .iter()
            .map(|attempt| attempt.run_id)
            .collect::<BTreeSet<_>>()
            .len(),
        agent_call_count: attempts.len(),
        mean_agent_execution_ms: mean(&agent_durations),
        median_agent_execution_ms: median(&agent_durations),
        mean_deterministic_time_to_green_ms: mean(&green_durations),
        median_deterministic_time_to_green_ms: median(&green_durations),
        resumed_session_count: attempts
            .iter()
            .filter(|attempt| attempt.resumed_provider_session)
            .count(),
        environment_verified_count: attempts
            .iter()
            .filter(|attempt| {
                attempt.environment.as_ref().and_then(|value| value.matched) == Some(true)
            })
            .count(),
        ..Summary::default()
    };
    for attempt in &attempts {
        *summary
            .provider_counts
            .entry(attempt.provider.clone())
            .or_default() += 1;
        *summary
            .model_counts
            .entry(attempt.model.clone().unwrap_or_else(|| "unknown".into()))
            .or_default() += 1;
        *summary
            .outcome_counts
            .entry(attempt.outcome.clone())
            .or_default() += 1;
    }

    let report = EfficiencyReport {
        schema_version: 1,
        kind: "agent-loop-orchestrator/task-efficiency-ledger",
        generated_at: Utc::now(),
        window: Window { since, until },
        summary,
        attempts,
        telemetry_gaps: vec![
            "escalationReason is not yet persisted by the execution model",
            "provider token/cost fields are emitted only when present in provider event JSON",
            "CI run IDs are available only when evidence URIs contain actions/runs/<id>",
            "hosted-CI time-to-green is not inferred from local deterministic checks",
        ],
    };
    let json = serde_json::to_string_pretty(&report)?;
    if let Some(output) = args.output {
        if let Some(parent) = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(&output, format!("{json}\n"))
            .with_context(|| format!("write {}", output.display()))?;
    }
    println!("{json}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use agent_loop_orchestrator::contracts::Evidence;
    use chrono::Utc;
    use serde_json::json;

    use super::{UsageEvidence, ci_run_ids_from_evidence, collect_usage};

    #[test]
    fn collects_max_provider_usage_without_double_counting_events() {
        let mut usage = UsageEvidence::default();
        collect_usage(
            &json!({
                "event": {"usage": {"input_tokens": 100, "output_tokens": 20}},
                "later": {
                    "usage": {"inputTokens": 140, "outputTokens": 30, "totalTokens": 170}
                }
            }),
            &mut usage,
        );
        assert_eq!(usage.input_tokens, Some(140));
        assert_eq!(usage.output_tokens, Some(30));
        assert_eq!(usage.total_tokens, Some(170));
    }

    #[test]
    fn extracts_ci_run_ids_from_evidence_uris() {
        let evidence = Evidence {
            schema_version: 1,
            kind: "ci".into(),
            uri: "https://github.com/o/r/actions/runs/123456/job/7".into(),
            digest: "sha256:abc".into(),
            media_type: None,
            created_at: Utc::now(),
            size_bytes: None,
        };
        assert_eq!(ci_run_ids_from_evidence([&evidence]), vec![123456]);
    }
}
