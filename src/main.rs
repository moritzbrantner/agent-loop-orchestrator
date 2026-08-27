use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(error) = agent_loop_orchestrator::environment::activate_registered_tools() {
        eprintln!("error: {error:#}");
        return ExitCode::FAILURE;
    }
    match agent_loop_orchestrator::cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
