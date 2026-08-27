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
            Some(component) if *name == "coding-tooling" && !coding_tooling_cli(component).is_file() => {
                ComponentDiagnostic {
                    name: (*name).into(),
                    path: Some(component.path.clone()),
                    observed_revision: Some(component.observed_revision.clone()),
                    ready: false,
                    error: Some("registered coding-tooling checkout has no src/cli.ts".into()),
                }
            }
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

/// Make machine-registered deterministic tools available to the existing execution adapters.
///
/// The orchestrator still honors an already installed `coding-tooling` executable. When it is
/// absent, this creates a process-local runtime shim pointing at the exact registered source
/// checkout and prepends only that shim directory to PATH. The registry remains the source of
/// discovery; no tool code is copied or vendored into a target repository.
pub fn activate_registered_tools() -> Result<()> {
    if find_on_path(OsStr::new("coding-tooling")).is_some() {
        return Ok(());
    }
    let Some(registry) = load_default()? else {
        return Ok(());
    };
    let Some(component) = registry.components.get("coding-tooling") else {
        return Ok(());
    };
    let cli = coding_tooling_cli(component);
    if !cli.is_file() {
        bail!(
            "registered coding-tooling checkout {} has no src/cli.ts",
            component.path.display()
        );
    }
    if find_on_path(OsStr::new("bun")).is_none() {
        bail!("Bun is required to run the registered coding-tooling source checkout");
    }
    activate_executable_shim("coding-tooling", &cli)
}

fn coding_tooling_cli(component: &EnvironmentComponent) -> PathBuf {
    component.path.join("src").join("cli.ts")
}

#[cfg(unix)]
fn activate_executable_shim(name: &str, target: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;

    let data_root = dirs::data_local_dir().context("determine local data directory")?;
    let bin = data_root
        .join("agent-loop-orchestrator")
        .join("runtime-bin");
    fs::create_dir_all(&bin)?;
    let shim = bin.join(name);
    if shim.exists() || shim.symlink_metadata().is_ok() {
        let current = fs::read_link(&shim).ok();
        if current.as_deref() != Some(target) {
            fs::remove_file(&shim)
                .with_context(|| format!("replace runtime shim {}", shim.display()))?;
        }
    }
    if !shim.exists() {
        symlink(target, &shim).with_context(|| {
            format!(
                "create runtime shim {} -> {}",
                shim.display(),
                target.display()
            )
        })?;
    }

    let existing = env::var_os("PATH").unwrap_or_default();
    let joined = env::join_paths(std::iter::once(bin).chain(env::split_paths(&existing)))
        .context("construct process PATH for registered tools")?;
    // SAFETY: this runs synchronously at process startup before the CLI creates worker threads.
    unsafe { env::set_var("PATH", joined) };
    Ok(())
}

#[cfg(not(unix))]
fn activate_executable_shim(_name: &str, _target: &Path) -> Result<()> {
    Ok(())
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
        let tooling_src = tooling.join("src");
        fs::create_dir(&conventions).unwrap();
        fs::create_dir_all(&tooling_src).unwrap();
        fs::write(tooling_src.join("cli.ts"), "#!/usr/bin/env bun\n").unwrap();
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
        assert!(
            diagnostics
                .iter()
                .any(|item| item.name == "coding-agent-conventions" && item.ready)
        );
        assert!(
            diagnostics
                .iter()
                .any(|item| item.name == "coding-tooling" && item.ready)
        );
        assert!(
            diagnostics
                .iter()
                .any(|item| item.name == "coding-agent-skills" && !item.ready)
        );
    }
}
