use std::{fs, io, net::SocketAddr, path::PathBuf, str::FromStr};

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    adapters::Provider,
    config::ProjectConfig,
    control,
    execution::{CreateWorkItem, DecisionRequest, ExecutionOverrides, ExecutionService},
    repository::{self, RegisteredProject, find_repository_root},
};

#[derive(Debug, Parser)]
#[command(name = "agent-loop", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Add agent-loop configuration to a Git repository and register it locally.
    Init {
        #[arg(default_value = ".")]
        repository: PathBuf,
        #[arg(long, default_value = "codex")]
        provider: String,
        #[arg(long)]
        force: bool,
    },
    /// Create and start a whole-repository work item, stopping for a decision.
    Run {
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long, conflicts_with = "prompt_file")]
        prompt: Option<String>,
        #[arg(long, value_name = "FILE", conflicts_with = "prompt")]
        prompt_file: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        /// Continue an existing provider session inside a fresh isolated attempt.
        #[arg(long)]
        resume: Option<String>,
    },
    /// Create and inspect durable local work items.
    WorkItem {
        #[command(subcommand)]
        command: WorkItemCommands,
    },
    /// Stable machine-readable integration boundary for thin skills and automation.
    Control {
        #[command(subcommand)]
        command: ControlCommands,
    },
    /// Start one open work item through its configured provider.
    Start {
        work_item_id: Uuid,
        #[arg(long)]
        provider: Option<String>,
    },
    /// Show a work item or run from durable local state.
    Show { id: Uuid },
    /// Approve and locally integrate an exact checked candidate.
    Approve {
        run_id: Uuid,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Reject a candidate without integrating it.
    Reject {
        run_id: Uuid,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Check provider installations and authentication without starting an agent.
    Doctor {
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = ".")]
        repository: PathBuf,
    },
    /// Serve the authenticated React dashboard.
    Serve {
        /// LAN address and port to listen on.
        #[arg(long, default_value = "0.0.0.0:3000")]
        bind: SocketAddr,
    },
    /// Print shell completion definitions.
    Completions {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
}

#[derive(Debug, Subcommand)]
enum WorkItemCommands {
    /// Create a work item and bind it to the selected baseline commit.
    Create {
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        #[arg(long)]
        title: String,
        #[arg(long, conflicts_with = "prompt_file")]
        prompt: Option<String>,
        #[arg(long, value_name = "FILE", conflicts_with = "prompt")]
        prompt_file: Option<PathBuf>,
        #[arg(long = "scope", default_value = ".")]
        declared_scope: Vec<String>,
        #[arg(long)]
        baseline: Option<String>,
        #[arg(long)]
        target_branch: Option<String>,
    },
    /// List durable local work items.
    List,
}

#[derive(Debug, Subcommand)]
enum ControlCommands {
    /// Create or list bounded local work items with explicit intent metadata.
    WorkItem {
        #[command(subcommand)]
        command: ControlWorkItemCommands,
    },
    /// Start one dependency-ready work item.
    Start {
        work_item_id: Uuid,
        #[arg(long)]
        provider: Option<String>,
    },
    /// Continue the provider session from a prior run in a fresh bounded work item.
    Resume {
        run_id: Uuid,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
    },
    /// Inspect one work item or run with readiness and canonical run state.
    Status { id: Uuid },
    /// Approve and locally integrate an exact awaiting-decision candidate.
    Approve {
        run_id: Uuid,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Reject an exact awaiting-decision candidate without integration.
    Reject {
        run_id: Uuid,
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ControlWorkItemCommands {
    /// Create a bounded work item from explicit objective, acceptance, dependencies, and scope.
    Create {
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        #[arg(long)]
        title: String,
        #[arg(long, conflicts_with = "objective_file")]
        objective: Option<String>,
        #[arg(long, value_name = "FILE", conflicts_with = "objective")]
        objective_file: Option<PathBuf>,
        #[arg(long = "acceptance", value_name = "ID=CAPABILITY")]
        acceptance: Vec<String>,
        #[arg(long = "dependency", value_name = "WORK_ITEM_ID")]
        dependencies: Vec<Uuid>,
        #[arg(long = "scope", default_value = ".")]
        declared_scope: Vec<String>,
        #[arg(long)]
        baseline: Option<String>,
        #[arg(long)]
        target_branch: Option<String>,
    },
    /// List bounded work items with deterministic readiness and blockers.
    List,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CompletionShell {
    Bash,
    Fish,
    Zsh,
}

impl From<CompletionShell> for Shell {
    fn from(value: CompletionShell) -> Self {
        match value {
            CompletionShell::Bash => Shell::Bash,
            CompletionShell::Fish => Shell::Fish,
            CompletionShell::Zsh => Shell::Zsh,
        }
    }
}

pub fn run() -> Result<()> {
    match Cli::parse().command {
        Commands::Init {
            repository,
            provider,
            force,
        } => {
            let provider = Provider::from_str(&provider)?;
            let path = repository::init(&repository, provider, force)?;
            println!("Initialized {}", path.display());
            println!("Next: agent-loop doctor && agent-loop run --prompt \"your task\"");
        }
        Commands::Run {
            repository,
            provider,
            prompt,
            prompt_file,
            model,
            effort,
            resume,
        } => {
            let root = find_repository_root(&repository)?;
            let config = ProjectConfig::load(&root)?;
            let provider = provider
                .as_deref()
                .map(Provider::from_str)
                .transpose()?
                .unwrap_or(config.agent.provider);
            let prompt = read_prompt(prompt, prompt_file)?;
            let project = registered_project_for_root(&root, &config)?;
            let mut service = execution_service()?;
            let work_item = service.create_work_item(CreateWorkItem {
                project,
                title: prompt.lines().next().unwrap_or("Local work item").into(),
                prompt: prompt.clone(),
                declared_scope: vec![".".into()],
                baseline_ref: config.execution.target_branch.clone(),
                target_branch: config.execution.target_branch,
            })?;
            control::record_work_item_intent(
                &repository::data_directory()?,
                work_item.id,
                prompt,
                Vec::new(),
                Vec::new(),
            )?;
            let run = service.run_work_item_with_overrides(
                &work_item.id,
                provider,
                ExecutionOverrides {
                    model,
                    effort,
                    resume_session: resume,
                },
                None,
                |_| {},
            )?;
            println!("Work item: {}", work_item.id);
            println!("Run: {} ({:?})", run.id, run.status);
            if run.status == crate::execution::LocalRunStatus::AwaitingDecision {
                println!("Next: agent-loop approve {}", run.id);
            }
        }
        Commands::WorkItem { command } => match command {
            WorkItemCommands::Create {
                repository,
                title,
                prompt,
                prompt_file,
                declared_scope,
                baseline,
                target_branch,
            } => {
                let root = find_repository_root(&repository)?;
                let config = ProjectConfig::load(&root)?;
                let project = registered_project_for_root(&root, &config)?;
                let target_branch = target_branch.unwrap_or(config.execution.target_branch);
                let baseline_ref = baseline.unwrap_or_else(|| target_branch.clone());
                let mut service = execution_service()?;
                let objective = read_prompt(prompt, prompt_file)?;
                let work_item = service.create_work_item(CreateWorkItem {
                    project,
                    title,
                    prompt: objective.clone(),
                    declared_scope,
                    baseline_ref,
                    target_branch,
                })?;
                control::record_work_item_intent(
                    &repository::data_directory()?,
                    work_item.id,
                    objective,
                    Vec::new(),
                    Vec::new(),
                )?;
                println!("{}", serde_json::to_string_pretty(&work_item)?);
            }
            WorkItemCommands::List => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&execution_service()?.snapshot().work_items)?
                );
            }
        },
        Commands::Control { command } => emit_control(run_control(command)),
        Commands::Start {
            work_item_id,
            provider,
        } => {
            let mut service = execution_service()?;
            let item = service
                .snapshot()
                .work_items
                .into_iter()
                .find(|item| item.id == work_item_id)
                .with_context(|| format!("work item {work_item_id} was not found"))?;
            let config = ProjectConfig::load(&item.repository_root)?;
            let provider = provider
                .as_deref()
                .map(Provider::from_str)
                .transpose()?
                .unwrap_or(config.agent.provider);
            let intent = control::intent_for(&data_root, &item)?;
            let run = service.run_work_item_with_overrides(
                &work_item_id,
                provider,
                ExecutionOverrides {
                    objective: Some(intent.objective),
                    acceptance: Some(intent.acceptance),
                    dependencies: Some(intent.dependencies),
                    ..ExecutionOverrides::default()
                },
                None,
                |_| {},
            )?;
            println!("{}", serde_json::to_string_pretty(&run)?);
        }
        Commands::Show { id } => {
            let snapshot = execution_service()?.snapshot();
            if let Some(run) = snapshot.runs.iter().find(|run| run.id == id) {
                println!("{}", serde_json::to_string_pretty(run)?);
            } else if let Some(item) = snapshot.work_items.iter().find(|item| item.id == id) {
                println!("{}", serde_json::to_string_pretty(item)?);
            } else {
                bail!("no work item or run {id} was found");
            }
        }
        Commands::Approve { run_id, reason } => {
            let mut service = execution_service()?;
            let run = service.decide(
                run_id,
                DecisionRequest::Approve {
                    actor: "local-cli".into(),
                    reason,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&run)?);
        }
        Commands::Reject { run_id, reason } => {
            let mut service = execution_service()?;
            let run = service.decide(
                run_id,
                DecisionRequest::Reject {
                    actor: "local-cli".into(),
                    reason,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&run)?);
        }
        Commands::Doctor {
            provider,
            json,
            repository,
        } => {
            let provider = provider.as_deref().map(Provider::from_str).transpose()?;
            let config = find_repository_root(&repository)
                .ok()
                .and_then(|root| ProjectConfig::load(&root).ok());
            crate::doctor::run(config.as_ref(), provider, json)?;
        }
        Commands::Serve { bind } => {
            tokio::runtime::Runtime::new()?
                .block_on(crate::server::serve(crate::server::ServeOptions { bind }))?;
        }
        Commands::Completions { shell } => {
            generate(
                Shell::from(shell),
                &mut Cli::command(),
                "agent-loop",
                &mut io::stdout(),
            );
        }
    }
    Ok(())
}

fn run_control(command: ControlCommands) -> Result<(&'static str, Value)> {
    let data_root = repository::data_directory()?;
    match command {
        ControlCommands::WorkItem { command } => match command {
            ControlWorkItemCommands::Create {
                repository,
                title,
                objective,
                objective_file,
                acceptance,
                dependencies,
                declared_scope,
                baseline,
                target_branch,
            } => {
                let root = find_repository_root(&repository)?;
                let config = ProjectConfig::load(&root).with_context(|| {
                    format!("project configuration unavailable in {}", root.display())
                })?;
                let project = registered_project_for_root(&root, &config)?;
                let target_branch = target_branch.unwrap_or(config.execution.target_branch);
                let baseline_ref = baseline.unwrap_or_else(|| target_branch.clone());
                let objective = read_prompt(objective, objective_file)?;
                let acceptance = acceptance
                    .iter()
                    .map(|value| control::parse_acceptance(value))
                    .collect::<Result<Vec<_>>>()?;
                let dependency_ids = dependencies
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                let mut service = execution_service()?;
                let snapshot = service.snapshot();
                for dependency in &dependencies {
                    if !snapshot
                        .work_items
                        .iter()
                        .any(|item| item.id == *dependency)
                    {
                        bail!("dependency work item {dependency} was not found");
                    }
                }
                let work_item = service.create_work_item(CreateWorkItem {
                    project,
                    title,
                    prompt: objective.clone(),
                    declared_scope,
                    baseline_ref,
                    target_branch,
                })?;
                control::record_work_item_intent(
                    &data_root,
                    work_item.id,
                    objective,
                    acceptance,
                    dependency_ids,
                )?;
                let snapshot = service.snapshot();
                let item = snapshot
                    .work_items
                    .iter()
                    .find(|item| item.id == work_item.id)
                    .context("created work item was not persisted")?;
                Ok((
                    "work_item",
                    json!(control::work_item_view(&data_root, &snapshot, item)?),
                ))
            }
            ControlWorkItemCommands::List => {
                let snapshot = execution_service()?.snapshot();
                Ok((
                    "work_items",
                    json!(control::work_item_views(&data_root, &snapshot)?),
                ))
            }
        },
        ControlCommands::Start {
            work_item_id,
            provider,
        } => {
            let mut service = execution_service()?;
            let snapshot = service.snapshot();
            let item = snapshot
                .work_items
                .iter()
                .find(|item| item.id == work_item_id)
                .cloned()
                .with_context(|| format!("work item {work_item_id} was not found"))?;
            control::ensure_ready(&data_root, &snapshot, &item)?;
            let config = ProjectConfig::load(&item.repository_root)
                .context("project configuration unavailable")?;
            let provider = provider
                .as_deref()
                .map(Provider::from_str)
                .transpose()?
                .unwrap_or(config.agent.provider);
            let run = service.run_work_item(&work_item_id, provider, None, |_| {})?;
            let snapshot = service.snapshot();
            Ok((
                "run",
                json!(control::run_view(&data_root, &snapshot, &run)?),
            ))
        }
        ControlCommands::Resume {
            run_id,
            model,
            effort,
        } => {
            let mut service = execution_service()?;
            let snapshot = service.snapshot();
            let prior_run = snapshot
                .runs
                .iter()
                .find(|run| run.id == run_id)
                .cloned()
                .with_context(|| format!("run {run_id} was not found"))?;
            if prior_run.status == crate::execution::LocalRunStatus::AwaitingDecision {
                bail!(
                    "run {run_id} is awaiting a human decision; approve or reject it before resuming"
                )
            }
            let prior_item = snapshot
                .work_items
                .iter()
                .find(|item| item.id == prior_run.work_item_id)
                .cloned()
                .context("run work item was not found")?;
            let session = prior_run
                .contract
                .attempts
                .iter()
                .rev()
                .find_map(|attempt| attempt.provider_session_id.clone())
                .with_context(|| format!("run {run_id} has no resumable provider session"))?;
            let intent = control::intent_for(&data_root, &prior_item)?;
            let config = ProjectConfig::load(&prior_item.repository_root)
                .context("project configuration unavailable")?;
            let project = registered_project_for_root(&prior_item.repository_root, &config)?;
            let work_item = service.create_work_item(CreateWorkItem {
                project,
                title: prior_item.title.clone(),
                prompt: intent.objective.clone(),
                declared_scope: prior_item.declared_scope.clone(),
                baseline_ref: prior_item.target_branch.clone(),
                target_branch: prior_item.target_branch.clone(),
            })?;
            control::record_work_item_intent(
                &data_root,
                work_item.id,
                intent.objective.clone(),
                intent.acceptance.clone(),
                intent.dependencies.clone(),
            )?;
            let snapshot = service.snapshot();
            let created = snapshot
                .work_items
                .iter()
                .find(|item| item.id == work_item.id)
                .cloned()
                .context("resumed work item was not persisted")?;
            control::ensure_ready(&data_root, &snapshot, &created)?;
            let run = service.run_work_item_with_overrides(
                &created.id,
                prior_run.provider,
                ExecutionOverrides {
                    model,
                    effort,
                    resume_session: Some(session),
                    objective: Some(intent.objective),
                    acceptance: Some(intent.acceptance),
                    dependencies: Some(intent.dependencies),
                },
                None,
                |_| {},
            )?;
            let snapshot = service.snapshot();
            Ok((
                "run",
                json!(control::run_view(&data_root, &snapshot, &run)?),
            ))
        }
        ControlCommands::Status { id } => {
            let snapshot = execution_service()?.snapshot();
            if let Some(run) = snapshot.runs.iter().find(|run| run.id == id) {
                Ok(("run", json!(control::run_view(&data_root, &snapshot, run)?)))
            } else if let Some(item) = snapshot.work_items.iter().find(|item| item.id == id) {
                Ok((
                    "work_item",
                    json!(control::work_item_view(&data_root, &snapshot, item)?),
                ))
            } else {
                bail!("no work item or run {id} was found")
            }
        }
        ControlCommands::Approve { run_id, reason } => {
            let mut service = execution_service()?;
            let run = service.decide(
                run_id,
                DecisionRequest::Approve {
                    actor: "agent-loop-control".into(),
                    reason,
                },
            )?;
            let snapshot = service.snapshot();
            Ok((
                "run",
                json!(control::run_view(&data_root, &snapshot, &run)?),
            ))
        }
        ControlCommands::Reject { run_id, reason } => {
            let mut service = execution_service()?;
            let run = service.decide(
                run_id,
                DecisionRequest::Reject {
                    actor: "agent-loop-control".into(),
                    reason,
                },
            )?;
            let snapshot = service.snapshot();
            Ok((
                "run",
                json!(control::run_view(&data_root, &snapshot, &run)?),
            ))
        }
    }
}

fn emit_control(result: Result<(&'static str, Value)>) {
    match result {
        Ok((kind, data)) => println!(
            "{}",
            serde_json::to_string(&json!({
                "schemaVersion": 1,
                "ok": true,
                "kind": kind,
                "data": data,
            }))
            .expect("serialize control response")
        ),
        Err(error) => println!(
            "{}",
            serde_json::to_string(&json!({
                "schemaVersion": 1,
                "ok": false,
                "error": {
                    "code": control_error_code(&error),
                    "message": format!("{error:#}"),
                }
            }))
            .expect("serialize control error")
        ),
    }
}

fn control_error_code(error: &anyhow::Error) -> &'static str {
    let message = format!("{error:#}").to_lowercase();
    if message.contains("dependency-blocked") {
        "dependency_blocked"
    } else if message.contains("awaiting a human decision")
        || message.contains("awaiting a decision")
    {
        "awaiting_decision"
    } else if message.contains("was not found") || message.contains("no work item or run") {
        "not_found"
    } else if message.contains("project configuration unavailable")
        || message.contains("config.toml")
    {
        "missing_project_config"
    } else if message.contains("provider")
        && (message.contains("unavailable") || message.contains("not ready"))
    {
        "provider_unavailable"
    } else if message.contains("coding-tooling")
        || message.contains("deterministic checks did not pass")
    {
        "tooling_unavailable_or_failed"
    } else if message.contains("not open")
        || message.contains("acceptance must")
        || message.contains("scope")
    {
        "invalid_task"
    } else if message.contains("another local run is active") {
        "conflict"
    } else {
        "control_error"
    }
}

fn execution_service() -> Result<ExecutionService> {
    ExecutionService::load(repository::data_directory()?)
}

fn registered_project_for_root(
    root: &std::path::Path,
    config: &ProjectConfig,
) -> Result<RegisteredProject> {
    repository::list_registered_projects()?
        .into_iter()
        .find(|project| project.id == config.project.id && project.repository_root == root)
        .with_context(|| {
            format!(
                "{} is not the registered root for project `{}`; run `agent-loop init` first",
                root.display(),
                config.project.id
            )
        })
}

fn read_prompt(prompt: Option<String>, prompt_file: Option<PathBuf>) -> Result<String> {
    let prompt = match (prompt, prompt_file) {
        (Some(prompt), None) => prompt,
        (None, Some(path)) => fs::read_to_string(&path)
            .with_context(|| format!("read prompt file {}", path.display()))?,
        (None, None) => bail!("provide --prompt or --prompt-file"),
        (Some(_), Some(_)) => unreachable!("clap enforces mutual exclusion"),
    };
    if prompt.trim().is_empty() {
        bail!("prompt cannot be empty");
    }
    Ok(prompt)
}
