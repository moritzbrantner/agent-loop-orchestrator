use std::{fs, io, net::SocketAddr, path::PathBuf, str::FromStr};

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use uuid::Uuid;

use crate::{
    adapters::Provider,
    config::ProjectConfig,
    doctor,
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
                prompt,
                declared_scope: vec![".".into()],
                baseline_ref: config.execution.target_branch.clone(),
                target_branch: config.execution.target_branch,
            })?;
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
                let work_item = service.create_work_item(CreateWorkItem {
                    project,
                    title,
                    prompt: read_prompt(prompt, prompt_file)?,
                    declared_scope,
                    baseline_ref,
                    target_branch,
                })?;
                println!("{}", serde_json::to_string_pretty(&work_item)?);
            }
            WorkItemCommands::List => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&execution_service()?.snapshot().work_items)?
                );
            }
        },
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
            let run = service.run_work_item(&work_item_id, provider, None, |_| {})?;
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
            doctor::run(config.as_ref(), provider, json)?;
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
