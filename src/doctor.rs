use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Result, bail};
use serde::Serialize;

use crate::{
    adapters::Provider,
    config::ProjectConfig,
    environment::{self, CORE_COMPONENTS, ComponentDiagnostic},
};

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteDiagnostic {
    repository: String,
    executable: String,
    found_at: Option<PathBuf>,
    version: Option<String>,
    authenticated: bool,
    repository_accessible: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DoctorReport<'a> {
    components: &'a [ComponentDiagnostic],
    providers: &'a [Diagnostic],
    #[serde(skip_serializing_if = "Option::is_none")]
    remote: Option<&'a RemoteDiagnostic>,
}

pub fn run(config: Option<&ProjectConfig>, requested: Option<Provider>, json: bool) -> Result<()> {
    let components = environment::diagnose_required_components(CORE_COMPONENTS)?;
    let providers: Vec<Provider> = requested
        .map(|provider| vec![provider])
        .unwrap_or_else(|| vec![Provider::Claude, Provider::Codex]);
    let diagnostics: Vec<_> = providers
        .into_iter()
        .map(|provider| diagnose(config, provider))
        .collect();
    let remote = config.and_then(diagnose_remote);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&DoctorReport {
                components: &components,
                providers: &diagnostics,
                remote: remote.as_ref(),
            })?
        );
    } else {
        println!("Machine components:");
        for component in &components {
            let state = if component.ready {
                "ready"
            } else {
                "not ready"
            };
            println!("  {}: {state}", component.name);
            if let Some(path) = &component.path {
                println!("    path: {}", path.display());
            }
            if let Some(revision) = &component.observed_revision {
                println!("    registered revision: {revision}");
            }
            if let Some(error) = &component.error {
                println!("    issue: {error}");
            }
        }

        println!("Providers:");
        for diagnostic in &diagnostics {
            let state = if diagnostic.authenticated {
                "ready"
            } else {
                "not ready"
            };
            println!("  {}: {state}", diagnostic.provider);
            println!("    executable: {}", diagnostic.executable);
            if let Some(path) = &diagnostic.found_at {
                println!("    path: {}", path.display());
            }
            if let Some(version) = &diagnostic.version {
                println!("    version: {version}");
            }
            if let Some(error) = &diagnostic.error {
                println!("    issue: {error}");
            }
        }
        if let Some(remote) = &remote {
            let state = if remote.authenticated && remote.repository_accessible {
                "ready"
            } else {
                "not ready"
            };
            println!("Remote GitHub automation: {state}");
            println!("  repository: {}", remote.repository);
            println!("  executable: {}", remote.executable);
            if let Some(path) = &remote.found_at {
                println!("  path: {}", path.display());
            }
            if let Some(version) = &remote.version {
                println!("  version: {version}");
            }
            if let Some(error) = &remote.error {
                println!("  issue: {error}");
            }
        }
    }

    if components.iter().any(|component| !component.ready) {
        bail!(
            "machine environment is not ready; register coding-agent-conventions, coding-agent-skills, and coding-tooling with agent-loop-setup/bin/setup-environment"
        );
    }
    if diagnostics
        .iter()
        .any(|diagnostic| !diagnostic.authenticated)
    {
        bail!("one or more requested providers are not ready");
    }
    if remote
        .as_ref()
        .is_some_and(|remote| !remote.authenticated || !remote.repository_accessible)
    {
        bail!("remote GitHub automation is not ready");
    }
    Ok(())
}

fn diagnose_remote(config: &ProjectConfig) -> Option<RemoteDiagnostic> {
    if !config.remote.enabled {
        return None;
    }
    let repository = config
        .remote
        .repository
        .clone()
        .unwrap_or_else(|| "(missing)".into());
    let executable = config.remote.github_executable.clone();
    let Some(found_at) = environment::find_on_path(OsStr::new(&executable)) else {
        return Some(RemoteDiagnostic {
            repository,
            executable,
            found_at: None,
            version: None,
            authenticated: false,
            repository_accessible: false,
            error: Some("GitHub CLI executable not found on PATH".into()),
        });
    };
    let version = command_text(&found_at, &["--version"]).ok();
    let authentication = command_success(&found_at, &["auth", "status"]);
    let repository_access = command_success(
        &found_at,
        &["repo", "view", &repository, "--json", "nameWithOwner"],
    );
    let error = authentication
        .as_ref()
        .err()
        .cloned()
        .or_else(|| repository_access.as_ref().err().cloned());
    Some(RemoteDiagnostic {
        repository,
        executable,
        found_at: Some(found_at),
        version,
        authenticated: authentication.is_ok(),
        repository_accessible: repository_access.is_ok(),
        error,
    })
}

fn diagnose(config: Option<&ProjectConfig>, provider: Provider) -> Diagnostic {
    let executable = match (config, provider) {
        (Some(config), Provider::Claude) => config.providers.claude.executable.clone(),
        (Some(config), Provider::Codex) => config.providers.codex.executable.clone(),
        (None, Provider::Claude) => "claude".into(),
        (None, Provider::Codex) => "codex".into(),
    };
    let Some(found_at) = environment::find_on_path(OsStr::new(&executable)) else {
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

fn command_text(executable: &Path, args: &[&str]) -> std::result::Result<String, String> {
    let output = Command::new(executable)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn command_success(executable: &Path, args: &[&str]) -> std::result::Result<(), String> {
    command_text(executable, args).map(|_| ())
}
