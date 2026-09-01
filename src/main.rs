use std::{
    process::ExitCode,
    sync::mpsc::{self, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

struct QueueHeartbeat {
    stop: Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl QueueHeartbeat {
    fn start() -> Self {
        eprintln!("Agent Loop queue started.");
        eprintln!("  Local repository verification is authoritative.");
        eprintln!(
            "  Hosted GitHub checks are advisory; review and merge safety gates still apply."
        );
        eprintln!("  Long local pipelines or agent runs may take several minutes.");

        let (stop, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            loop {
                match receiver.recv_timeout(Duration::from_secs(10)) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => eprintln!(
                        "  … still working (local verification or agent execution in progress)"
                    ),
                }
            }
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for QueueHeartbeat {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        eprintln!("Agent Loop queue finished.");
    }
}

fn is_queue_run() -> bool {
    let arguments = std::env::args().skip(1).take(2).collect::<Vec<_>>();
    arguments
        .first()
        .is_some_and(|argument| argument == "queue")
        && arguments.get(1).is_some_and(|argument| argument == "run")
}

fn main() -> ExitCode {
    if let Err(error) = agent_loop_orchestrator::environment::activate_registered_tools() {
        eprintln!("error: {error:#}");
        return ExitCode::FAILURE;
    }

    let queue_heartbeat = is_queue_run().then(QueueHeartbeat::start);
    let result = agent_loop_orchestrator::cli::run();
    drop(queue_heartbeat);

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
