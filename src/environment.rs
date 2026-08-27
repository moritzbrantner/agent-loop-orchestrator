use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const CORE_COMPONENTS: &[&str] = &[
    "coding-agent-conventions",
    "coding-agent-skills",
    "coding-tooling",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentRegistry {
    pub schema_version: u32,
    #[serde(default)]
    pub components: BTreeMap<String, EnvironmentComponent>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentComponent {
    pub path: PathBuf,
    pub observed_revision: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDiagnostic {
    pub name: String,
    pub path: Option<PathBuf>,
    pub observed_revision: Option<String>,
    pub ready: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedCommand {
    pub program: OsString,
    pub prefix_args: Vec<OsString>,
    pub source: String,
}

pub fn default_registry_path() -> Result<PathBuf> {
    let root = if let Some(root) = env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(root)
    } else {
        dirs::home_dir()
            .context("determine home directory for the Moenarch environment registry")?
            .join(".config")
    };
    Ok(root.join("moenarch").join("environment.toml"))
}

pub fn load(path: &Path) -> Result<EnvironmentRegistry> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("read machine environment registry {}", path.display()))?;
    let registry: EnvironmentRegistry = toml::from_str(&contents)
        .with_context(|| format!("parse machine environment registry {}", path.display()))?;
    if registry.schema_version != 1 {
        bail!(
            "unsupported machine environment registry schema version {} in {}",
            registry.schema_version,
            path.display()
        );
    }
    Ok(registry)
}

pub fn load_default() -> Result<Option<EnvironmentRegistry>> {
    let path = default_registry_path()?;
    if !path.exists() {
        return Ok(None);
    }
    load(&path).map(Some)
}

pub fn diagnose_required_components(required: &[&str]) -> Result<Vec<ComponentDiagnostic>> {
    let registry_path = default_registry_path()?;
    if !registry_path.exists() {
        return Ok(required
            .iter()
            .map(|name| ComponentDiagnostic {
                name: (*name).into(),
                path: None,
                observed_revision: None,
                ready: false,
                error: Some(format!(
                    "machine environment registry {} does not exist; run agent-loop-setup/bin/setup-environment first",
                    registry_path.display()
                )),
            })
            .collect());
    }
    let registry = load(&registry_path)?;
    Ok(diagnose_registry(&registry, required))
}

pub fn diagnose_registry(
    registry: &EnvironmentRegistry,
    required: &[&str],
) -> Vec<ComponentDiagnostic> {
    required
        .iter()
        .map(|name| match registry.components.get(*name) {
            None => ComponentDiagnostic {
                name: (*name).into(),
                path: None,
                observed_revision: None,
                ready: false,
                error: Some(format!("component `{name}` is not registered")),
            },
            Some(component) if !component.path.is_dir() => ComponentDiagnostic {
                name: (*name).into(),
                path: Some(component.path.clone()),
                observed_revision: Some(component.observed_revision.clone()),
                ready: false,
                error: Some("registered component path does not exist".into()),
            },
            Some(component) => ComponentDiagnostic {
                name: (*name).into(),
                path: Some(component.path.clone()),
                observed_revision: Some(component.observed_revision.clone()),
                ready: true,
                error: None,
            },
        })
        .collect()
}

pub fn resolve_coding_tooling(configured: &str) -> Result<ResolvedCommand> {
    if configured != "coding-tooling" {
        return Ok(ResolvedCommand {
            program: configured.into(),
            prefix_args: Vec::new(),
            source: "repository configuration".into(),
        });
    }

    if let Some(executable) = find_on_path(OsStr::new("coding-tooling")) {
        return Ok(ResolvedCommand {
            program: executable.into_os_string(),
            prefix_args: Vec::new(),
            source: "PATH".into(),
        });
    }

    let registry_path = default_registry_path()?;
    let registry = load_default()?.with_context(|| {
        format!(
            "coding-tooling is not on PATH and machine environment registry {} is missing; run agent-loop-setup/bin/setup-environment",
            registry_path.display()
        )
    })?;
    let component = registry.components.get("coding-tooling").with_context(|| {
        format!(
            "coding-tooling is not on PATH and is not registered in {}",
            registry_path.display()
        )
    })?;
    registered_coding_tooling_command(component, find_on_path(OsStr::new("bun"))).with_context(|| {
        format!(
            "resolve registered coding-tooling from {}",
            component.path.display()
        )
    })
}

fn registered_coding_tooling_command(
    component: &EnvironmentComponent,
    bun: Option<PathBuf>,
) -> Result<ResolvedCommand> {
    if !component.path.is_dir() {
        bail!("registered coding-tooling path does not exist");
    }
    let cli = component.path.join("src").join("cli.ts");
    if !cli.is_file() {
        bail!("registered coding-tooling checkout has no src/cli.ts");
    }
    let bun = bun.context("Bun is required to run coding-tooling from its registered source checkout")?;
    Ok(ResolvedCommand {
        program: bun.into_os_string(),
        prefix_args: vec![cli.into_os_string()],
        source: "machine environment registry".into(),
    })
}

pub fn find_on_path(executable: &OsStr) -> Option<PathBuf> {
    let candidate = Path::new(executable);
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_owned());
    }
    let executable = executable.to_string_lossy();
    env::split_paths(&env::var_os("PATH")?).find_map(|directory| {
        executable_candidates(&directory, &executable)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn diagnoses_registered_and_missing_core_components() {
        let root = tempdir().unwrap();
        let conventions = root.path().join("coding-agent-conventions");
        let tooling = root.path().join("coding-tooling");
        fs::create_dir(&conventions).unwrap();
        fs::create_dir(&tooling).unwrap();
        let registry = EnvironmentRegistry {
            schema_version: 1,
            components: BTreeMap::from([
                (
                    "coding-agent-conventions".into(),
                    EnvironmentComponent {
                        path: conventions,
                        observed_revision: "aaa".into(),
                    },
                ),
                (
                    "coding-tooling".into(),
                    EnvironmentComponent {
                        path: tooling,
                        observed_revision: "bbb".into(),
                    },
                ),
            ]),
        };

        let diagnostics = diagnose_registry(&registry, CORE_COMPONENTS);
        assert!(diagnostics.iter().any(|item| item.name == "coding-agent-conventions" && item.ready));
        assert!(diagnostics.iter().any(|item| item.name == "coding-tooling" && item.ready));
        assert!(diagnostics.iter().any(|item| item.name == "coding-agent-skills" && !item.ready));
    }

    #[test]
    fn builds_source_checkout_command_for_registered_coding_tooling() {
        let root = tempdir().unwrap();
        let tooling = root.path().join("coding-tooling");
        let src = tooling.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("cli.ts"), "console.log('ok');\n").unwrap();
        let bun = root.path().join("bun");
        fs::write(&bun, "").unwrap();
        let component = EnvironmentComponent {
            path: tooling.clone(),
            observed_revision: "abc".into(),
        };

        let command = registered_coding_tooling_command(&component, Some(bun.clone())).unwrap();
        assert_eq!(command.program, bun.into_os_string());
        assert_eq!(command.prefix_args, vec![tooling.join("src/cli.ts").into_os_string()]);
        assert_eq!(command.source, "machine environment registry");
    }
}
