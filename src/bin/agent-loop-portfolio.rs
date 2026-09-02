use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

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
    /// Optional JSON report path. The report is always printed to stdout as well.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Bound the number of repositories for canary scans.
    #[arg(long)]
    limit: Option<usize>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let report = agent_loop_orchestrator::portfolio::collect_findings(&args.root, args.limit)?;
    if let Some(output) = args.output.as_deref() {
        agent_loop_orchestrator::portfolio::write_findings_report(output, &report)?;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
