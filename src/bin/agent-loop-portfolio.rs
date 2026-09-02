use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use agent_loop_orchestrator::portfolio::PortfolioFindingsOptions;

#[derive(Debug, Parser)]
#[command(
    name = "agent-loop-portfolio",
    version,
    about = "Aggregate deterministic coding-tooling findings across local repositories"
)]
struct Args {
    /// Directory containing Git repositories as direct children, or one Git repository.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Optional full JSON report path. The JSON report is always printed to stdout as well.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Optional human-readable Markdown summary path.
    #[arg(long)]
    markdown_output: Option<PathBuf>,
    /// Maximum findings shown per repository in the Markdown summary.
    #[arg(long, default_value_t = 20)]
    max_findings_per_repository: usize,
    /// Bound the number of repositories for canary scans.
    #[arg(long)]
    limit: Option<usize>,
    /// Ask coding-tooling for only findings that are not in the repository baseline.
    #[arg(long)]
    new_only: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let report = agent_loop_orchestrator::portfolio::collect_findings_with_options(
        &args.root,
        PortfolioFindingsOptions {
            limit: args.limit,
            new_only: args.new_only,
        },
    )?;
    if let Some(output) = args.output.as_deref() {
        agent_loop_orchestrator::portfolio::write_findings_report(output, &report)?;
    }
    if let Some(output) = args.markdown_output.as_deref() {
        agent_loop_orchestrator::portfolio::write_markdown_report(
            output,
            &report,
            args.max_findings_per_repository,
        )?;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
