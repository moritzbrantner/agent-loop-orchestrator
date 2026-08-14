use std::{
    ffi::OsString,
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;

use crate::adapters::{AgentAdapter, Provider};

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutcome {
    pub provider: Provider,
    pub provider_session_id: Option<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub run_directory: PathBuf,
}

enum OutputLine {
    Stdout(String),
    Stderr(String),
    StdoutClosed,
    StderrClosed,
}

pub fn execute(
    adapter: &dyn AgentAdapter,
    spec: &CommandSpec,
    run_directory: &Path,
    timeout: Duration,
) -> Result<RunOutcome> {
    fs::create_dir_all(run_directory)
        .with_context(|| format!("create run directory {}", run_directory.display()))?;

    let mut child = Command::new(&spec.program)
        .args(&spec.args)
        .current_dir(&spec.current_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| {
            format!(
                "start provider executable `{}`; run `agent-loop doctor` to diagnose the installation",
                spec.program.to_string_lossy()
            )
        })?;

    let stdout = child.stdout.take().context("capture provider stdout")?;
    let stderr = child.stderr.take().context("capture provider stderr")?;
    let (sender, receiver) = mpsc::channel();
    spawn_reader(stdout, sender.clone(), true);
    spawn_reader(stderr, sender, false);

    let mut raw = BufWriter::new(File::create(run_directory.join("raw.jsonl"))?);
    let mut events = BufWriter::new(File::create(run_directory.join("events.jsonl"))?);
    let mut errors = BufWriter::new(File::create(run_directory.join("stderr.log"))?);
    let started = Instant::now();
    let mut stdout_closed = false;
    let mut stderr_closed = false;
    let mut timed_out = false;
    let mut provider_session_id = None;

    while !(stdout_closed && stderr_closed) {
        if started.elapsed() >= timeout {
            timed_out = true;
            child.kill().context("terminate timed-out provider process")?;
            break;
        }

        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(OutputLine::Stdout(line)) => {
                writeln!(raw, "{line}")?;
                println!("{line}");
                match serde_json::from_str::<Value>(&line) {
                    Ok(value) => {
                        if provider_session_id.is_none() {
                            provider_session_id = adapter.session_id(&value);
                        }
                        let normalized = serde_json::json!({
                            "schemaVersion": 1,
                            "provider": adapter.provider(),
                            "receivedAt": Utc::now(),
                            "event": value,
                        });
                        serde_json::to_writer(&mut events, &normalized)?;
                        writeln!(events)?;
                    }
                    Err(error) => {
                        let normalized = serde_json::json!({
                            "schemaVersion": 1,
                            "provider": adapter.provider(),
                            "receivedAt": Utc::now(),
                            "parseError": error.to_string(),
                            "raw": line,
                        });
                        serde_json::to_writer(&mut events, &normalized)?;
                        writeln!(events)?;
                    }
                }
            }
            Ok(OutputLine::Stderr(line)) => {
                writeln!(errors, "{line}")?;
                eprintln!("{line}");
            }
            Ok(OutputLine::StdoutClosed) => stdout_closed = true,
            Ok(OutputLine::StderrClosed) => stderr_closed = true,
            Err(RecvTimeoutError::Timeout) => {
                if child.try_wait()?.is_some() && stdout_closed && stderr_closed {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    raw.flush()?;
    events.flush()?;
    errors.flush()?;
    let status = child.wait().context("wait for provider process")?;
    let outcome = RunOutcome {
        provider: adapter.provider(),
        provider_session_id,
        exit_code: status.code(),
        timed_out,
        run_directory: run_directory.to_owned(),
    };
    fs::write(
        run_directory.join("outcome.json"),
        serde_json::to_vec_pretty(&outcome)?,
    )?;

    ensure_success(status, timed_out, run_directory)?;
    Ok(outcome)
}

fn spawn_reader(
    reader: impl std::io::Read + Send + 'static,
    sender: mpsc::Sender<OutputLine>,
    stdout: bool,
) {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(line) => {
                    let message = if stdout {
                        OutputLine::Stdout(line)
                    } else {
                        OutputLine::Stderr(line)
                    };
                    if sender.send(message).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(OutputLine::Stderr(format!("stream read error: {error}")));
                    break;
                }
            }
        }
        let _ = sender.send(if stdout {
            OutputLine::StdoutClosed
        } else {
            OutputLine::StderrClosed
        });
    });
}

fn ensure_success(status: ExitStatus, timed_out: bool, run_directory: &Path) -> Result<()> {
    if timed_out {
        bail!("provider timed out; evidence saved in {}", run_directory.display());
    }
    if !status.success() {
        bail!(
            "provider exited with {}; evidence saved in {}",
            status,
            run_directory.display()
        );
    }
    Ok(())
}
