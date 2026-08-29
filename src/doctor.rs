use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Result, bail};
use serde::Serialize;

use crate::{
    adapters::Provider,
    config::{ProjectConfig, PublicationMode},
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
struct PublicationDiagnostic {
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
    publication: Option<&'a PublicationDiagnostic>,
}

pub fn run(
    config: Option<&ProjectConfig>,
    repository_root: Option<&Path>,
    requested: Option<Provider>,
    json: bool,
) -> Result<()> {
    let components = environment::diagnose_required_components(CORE_COMPONENTS)?;
    let providers: Vec<Provider> = requested
        .map(|provider| vec![provider])
        .unwrap_or_else(|| vec![Provider::Claude, Provider::Codex]);
    let diagnostics: Vec<_> = providers
        .into_iter()
        .map(|provider| diagnose(config, provider))
        .collect();
    let publication = config
        .zip(repository_root)
        .and_then(|(config, root)| diagnose_publication(config, root));

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&DoctorReport {
                components: &components,
                providers: &diagnostics,
                publication: publication.as_ref(),
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
        if let Some(publication) = &publication {
            let state = if publication.authenticated && publication.repository_accessible {
                "ready"
            } else {
                "not ready"
            };
            println!("Pull-request publication: {state}");
            println!("  executable: {}", publication.executable);
            if let Some(path) = &publication.found_at {
                println!("  path: {}", path.display());
            }
            if let Some(version) = &publication.version {
                println!("  version: {version}");
            }
            if let Some(error) = &publication.error {
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
    if publication
        .as_ref()
        .is_some_and(|publication| !publication.authenticated || !publication.repository_accessible)
    {
        bail!("pull-request publication is not ready");
    }
    Ok(())
}

fn diagnose_publication(
    config: &ProjectConfig,
    repository_root: &Path,
) -> Option<PublicationDiagnostic> {
    if config.publication.mode != PublicationMode::PullRequest {
        return None;
    }
    let executable = config.publication.github_executable.clone();
    let Some(found_at) = environment::find_on_path(OsStr::new(&executable)) else {
        return Some(PublicationDiagnostic {
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
    let repository_access = command_success_in(
        &found_at,
        &["repo", "view", "--json", "nameWithOwner"],
        repository_root,
    );
    let error = authentication
        .as_ref()
        .err()
        .cloned()
        .or_else(|| repository_access.as_ref().err().cloned());
    Some(PublicationDiagnostic {
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

fn command_success_in(
    executable: &Path,
    args: &[&str],
    current_dir: &Path,
) -> std::result::Result<(), String> {
    let output = Command::new(executable)
        .args(args)
        .current_dir(current_dir)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(())
}
