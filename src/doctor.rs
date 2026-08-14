use std::{
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Result, bail};
use serde::Serialize;

use crate::{adapters::Provider, config::ProjectConfig};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub provider: Provider,
    pub executable: String,
    pub found_at: Option<PathBuf>,
    pub version: Option<String>,
    pub authenticated: bool,
    pub error: Option<String>,
}

pub fn run(config: Option<&ProjectConfig>, requested: Option<Provider>, json: bool) -> Result<()> {
    let providers: Vec<Provider> = requested
        .map(|provider| vec![provider])
        .unwrap_or_else(|| vec![Provider::Claude, Provider::Codex]);
    let diagnostics: Vec<_> = providers
        .into_iter()
        .map(|provider| diagnose(config, provider))
        .collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&diagnostics)?);
    } else {
        for diagnostic in &diagnostics {
            let state = if diagnostic.authenticated { "ready" } else { "not ready" };
            println!("{}: {state}", diagnostic.provider);
            println!("  executable: {}", diagnostic.executable);
            if let Some(path) = &diagnostic.found_at {
                println!("  path: {}", path.display());
            }
            if let Some(version) = &diagnostic.version {
                println!("  version: {version}");
            }
            if let Some(error) = &diagnostic.error {
                println!("  issue: {error}");
            }
        }
    }

    if diagnostics.iter().any(|diagnostic| !diagnostic.authenticated) {
        bail!("one or more requested providers are not ready");
    }
    Ok(())
}

fn diagnose(config: Option<&ProjectConfig>, provider: Provider) -> Diagnostic {
    let executable = match (config, provider) {
        (Some(config), Provider::Claude) => config.providers.claude.executable.clone(),
        (Some(config), Provider::Codex) => config.providers.codex.executable.clone(),
        (None, Provider::Claude) => "claude".into(),
        (None, Provider::Codex) => "codex".into(),
    };
    let Some(found_at) = find_on_path(&executable) else {
        return Diagnostic {
            provider,
            executable,
            found_at: None,
            version: None,
            authenticated: false,
            error: Some("executable not found on PATH".into()),
        };
    };

    let version = command_text(&found_at, &["--version"]);
    let authentication = match provider {
        Provider::Claude => command_success(&found_at, &["auth", "status"]),
        Provider::Codex => command_success(&found_at, &["login", "status"]),
    };
    Diagnostic {
        provider,
        executable,
        found_at: Some(found_at),
        version: version.ok(),
        authenticated: authentication.is_ok(),
        error: authentication.err(),
    }
}

fn find_on_path(executable: &str) -> Option<PathBuf> {
    let candidate = Path::new(executable);
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_owned());
    }
    env::split_paths(&env::var_os("PATH")?).find_map(|directory| {
        executable_candidates(&directory, executable)
            .into_iter()
            .find(|path| path.is_file())
    })
}

fn executable_candidates(directory: &Path, executable: &str) -> Vec<PathBuf> {
    let mut candidates = vec![directory.join(executable)];
    if cfg!(windows) {
        for extension in env::var_os("PATHEXT")
            .unwrap_or_else(|| OsString::from(".EXE;.CMD;.BAT"))
            .to_string_lossy()
            .split(';')
        {
            candidates.push(directory.join(format!("{executable}{extension}")));
        }
    }
    candidates
}

fn command_text(executable: &OsStr, args: &[&str]) -> std::result::Result<String, String> {
    let output = Command::new(executable)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn command_success(executable: &OsStr, args: &[&str]) -> std::result::Result<(), String> {
    command_text(executable, args).map(|_| ())
}

