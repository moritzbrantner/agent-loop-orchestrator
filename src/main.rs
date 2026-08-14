use std::process::ExitCode;

fn main() -> ExitCode {
    match agent_loop_orchestrator::cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

