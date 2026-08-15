use std::{fs, io, net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use uuid::Uuid;

use crate::{
    adapters::{Provider, RunRequest, adapter},
    config::ProjectConfig,
    doctor, process,
    repository::{self, find_repository_root},
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
    /// Run one provider turn and persist its event stream as local evidence.
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
        /// Continue an existing provider session by its provider-native ID or name.
        #[arg(long)]
        resume: Option<String>,
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
            let adapter = adapter(provider);
            let request = RunRequest {
                repository_root: &root,
                prompt: &prompt,
                resume_session: resume.as_deref(),
                model_override: model.as_deref(),
                effort_override: effort.as_deref(),
            };
            let command = adapter.command(&config, &request)?;
            let run_id = Uuid::new_v4();
            let run_directory = root.join(".agent-loop/runs").join(run_id.to_string());
            let outcome = process::execute(
                adapter.as_ref(),
                &command,
                &run_directory,
                Duration::from_secs(config.agent.max_duration_seconds),
            )?;
            println!("Run completed: {}", outcome.run_directory.display());
            if let Some(session_id) = outcome.provider_session_id {
                println!("Provider session: {session_id}");
            }
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
