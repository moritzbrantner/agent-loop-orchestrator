use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{adapters::Provider, config::ProjectConfig};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Registry {
    projects: BTreeMap<String, RegistryProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryProject {
    repository_root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredProject {
    pub id: String,
    pub repository_root: PathBuf,
}

pub fn find_repository_root(start: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(start)
        .output()
        .with_context(|| format!("run git in {}", start.display()))?;
    if !output.status.success() {
        bail!("{} is not inside a Git repository", start.display());
    }
    let root = String::from_utf8(output.stdout).context("Git returned a non-UTF-8 path")?;
    Ok(PathBuf::from(root.trim()))
}

pub fn init(start: &Path, provider: Provider, force: bool) -> Result<PathBuf> {
    let repository_root = find_repository_root(start)?;
    let project_id = repository_root
        .file_name()
        .and_then(|value| value.to_str())
        .context("repository directory has no valid UTF-8 name")?
        .to_owned();
    let config_directory = repository_root.join(".agent-loop");
    let config_path = config_directory.join("config.toml");
    if config_path.exists() && !force {
        bail!(
            "{} already exists; use --force to replace it",
            config_path.display()
        );
    }

    fs::create_dir_all(&config_directory)?;
    let config = ProjectConfig::default_for(project_id.clone(), provider);
    fs::write(&config_path, config.to_toml()?)
        .with_context(|| format!("write {}", config_path.display()))?;
    update_gitignore(&repository_root)?;
    register(&project_id, &repository_root)?;
    Ok(config_path)
}

fn update_gitignore(repository_root: &Path) -> Result<()> {
    const ENTRY: &str = ".agent-loop/runs/";
    let path = repository_root.join(".gitignore");
    let mut contents = fs::read_to_string(&path).unwrap_or_default();
    if contents.lines().any(|line| line.trim() == ENTRY) {
        return Ok(());
    }
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str("\n# Local agent-loop run evidence\n");
    contents.push_str(ENTRY);
    contents.push('\n');
    fs::write(&path, contents).with_context(|| format!("update {}", path.display()))
}

fn register(project_id: &str, repository_root: &Path) -> Result<()> {
    let directory = data_directory()?;
    let path = directory.join("projects.json");
    fs::create_dir_all(&directory)?;
    let mut registry = if path.exists() {
        serde_json::from_slice::<Registry>(&fs::read(&path)?)
            .with_context(|| format!("parse {}", path.display()))?
    } else {
        Registry::default()
    };
    registry.projects.insert(
        project_id.to_owned(),
        RegistryProject {
            repository_root: repository_root.to_owned(),
        },
    );
    fs::write(&path, serde_json::to_vec_pretty(&registry)?)
        .with_context(|| format!("write {}", path.display()))
}

pub fn list_registered_projects() -> Result<Vec<RegisteredProject>> {
    let path = data_directory()?.join("projects.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let registry: Registry = serde_json::from_slice(&fs::read(&path)?)
        .with_context(|| format!("parse {}", path.display()))?;
    Ok(registry
        .projects
        .into_iter()
        .map(|(id, project)| RegisteredProject {
            id,
            repository_root: project.repository_root,
        })
        .collect())
}

pub fn data_directory() -> Result<PathBuf> {
    let data_root = dirs::data_local_dir().context("determine user data directory")?;
    Ok(data_root.join("agent-loop-orchestrator"))
}
