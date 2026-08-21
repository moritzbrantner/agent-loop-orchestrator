use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    path::Path,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    contracts::AcceptanceCriterion,
    execution::{ExecutionSnapshot, LocalRun, WorkItem, WorkItemStatus},
};

const CONTROL_METADATA_FILE: &str = "control-metadata.json";
const CONTROL_METADATA_LOCK_FILE: &str = "control-metadata.lock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemIntent {
    pub work_item_id: Uuid,
    pub objective: String,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub dependencies: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    Blocked,
    NotOpen,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyBlocker {
    pub work_item_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemView {
    pub work_item: WorkItem,
    pub objective: String,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub dependencies: Vec<String>,
    pub readiness: Readiness,
    pub blockers: Vec<DependencyBlocker>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub run: LocalRun,
    pub work_item: WorkItemView,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedControlMetadata {
    #[serde(default)]
    work_items: BTreeMap<String, WorkItemIntent>,
}

pub fn parse_acceptance(value: &str) -> Result<AcceptanceCriterion> {
    let (id, capability) = value
        .split_once('=')
        .context("acceptance must use ID=CAPABILITY")?;
    let id = id.trim();
    let capability = capability.trim();
    if id.is_empty() || capability.is_empty() {
        bail!("acceptance must use non-empty ID=CAPABILITY");
    }
    Ok(AcceptanceCriterion {
        id: id.into(),
        capability: capability.into(),
        component: None,
        required: true,
    })
}

pub fn record_work_item_intent(
    data_root: &Path,
    work_item_id: Uuid,
    objective: String,
    acceptance: Vec<AcceptanceCriterion>,
    dependencies: Vec<String>,
) -> Result<()> {
    if objective.trim().is_empty() {
        bail!("objective cannot be empty");
    }
    fs::create_dir_all(data_root)?;
    let lock = lock_exclusive(data_root)?;
    let mut metadata = load_unlocked(data_root)?;
    metadata.work_items.insert(
        work_item_id.to_string(),
        WorkItemIntent {
            work_item_id,
            objective,
            acceptance,
            dependencies,
            created_at: Utc::now(),
        },
    );
    write_unlocked(data_root, &metadata)?;
    FileExt::unlock(&lock)?;
    Ok(())
}

pub fn intent_for(data_root: &Path, work_item: &WorkItem) -> Result<WorkItemIntent> {
    Ok(load(data_root)?
        .work_items
        .get(&work_item.id.to_string())
        .cloned()
        .unwrap_or_else(|| WorkItemIntent {
            work_item_id: work_item.id,
            objective: work_item.prompt.clone(),
            acceptance: Vec::new(),
            dependencies: Vec::new(),
            created_at: work_item.created_at,
        }))
}

pub fn work_item_views(
    data_root: &Path,
    snapshot: &ExecutionSnapshot,
) -> Result<Vec<WorkItemView>> {
    snapshot
        .work_items
        .iter()
        .map(|item| work_item_view(data_root, snapshot, item))
        .collect()
}

pub fn work_item_view(
    data_root: &Path,
    snapshot: &ExecutionSnapshot,
    work_item: &WorkItem,
) -> Result<WorkItemView> {
    let intent = intent_for(data_root, work_item)?;
    let blockers = dependency_blockers(snapshot, &intent.dependencies);
    let readiness = if work_item.status != WorkItemStatus::Open {
        Readiness::NotOpen
    } else if blockers.is_empty() {
        Readiness::Ready
    } else {
        Readiness::Blocked
    };
    Ok(WorkItemView {
        work_item: work_item.clone(),
        objective: intent.objective,
        acceptance: intent.acceptance,
        dependencies: intent.dependencies,
        readiness,
        blockers,
    })
}

pub fn run_view(data_root: &Path, snapshot: &ExecutionSnapshot, run: &LocalRun) -> Result<RunView> {
    let work_item = snapshot
        .work_items
        .iter()
        .find(|item| item.id == run.work_item_id)
        .context("run work item was not found")?;
    Ok(RunView {
        run: run.clone(),
        work_item: work_item_view(data_root, snapshot, work_item)?,
    })
}

pub fn ensure_ready(
    data_root: &Path,
    snapshot: &ExecutionSnapshot,
    work_item: &WorkItem,
) -> Result<()> {
    let view = work_item_view(data_root, snapshot, work_item)?;
    match view.readiness {
        Readiness::Ready => Ok(()),
        Readiness::Blocked => {
            let blockers = view
                .blockers
                .iter()
                .map(|blocker| format!("{} ({})", blocker.work_item_id, blocker.status))
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "work item {} is dependency-blocked by {blockers}",
                work_item.id
            )
        }
        Readiness::NotOpen => bail!("work item {} is not open", work_item.id),
    }
}

fn dependency_blockers(
    snapshot: &ExecutionSnapshot,
    dependencies: &[String],
) -> Vec<DependencyBlocker> {
    dependencies
        .iter()
        .filter_map(|dependency| {
            let item = snapshot
                .work_items
                .iter()
                .find(|item| item.id.to_string() == *dependency);
            match item {
                Some(item) if item.status == WorkItemStatus::Approved => None,
                Some(item) => Some(DependencyBlocker {
                    work_item_id: dependency.clone(),
                    status: work_item_status_name(&item.status).into(),
                }),
                None => Some(DependencyBlocker {
                    work_item_id: dependency.clone(),
                    status: "missing".into(),
                }),
            }
        })
        .collect()
}

fn work_item_status_name(status: &WorkItemStatus) -> &'static str {
    match status {
        WorkItemStatus::Open => "open",
        WorkItemStatus::Running => "running",
        WorkItemStatus::AwaitingDecision => "awaiting_decision",
        WorkItemStatus::Approved => "approved",
        WorkItemStatus::Rejected => "rejected",
        WorkItemStatus::Failed => "failed",
        WorkItemStatus::Cancelled => "cancelled",
    }
}

fn load(data_root: &Path) -> Result<PersistedControlMetadata> {
    fs::create_dir_all(data_root)?;
    let lock = lock_shared(data_root)?;
    let metadata = load_unlocked(data_root);
    FileExt::unlock(&lock)?;
    metadata
}

fn load_unlocked(data_root: &Path) -> Result<PersistedControlMetadata> {
    let path = data_root.join(CONTROL_METADATA_FILE);
    if !path.exists() {
        return Ok(PersistedControlMetadata::default());
    }
    serde_json::from_slice(&fs::read(&path)?).with_context(|| format!("parse {}", path.display()))
}

fn write_unlocked(data_root: &Path, metadata: &PersistedControlMetadata) -> Result<()> {
    let path = data_root.join(CONTROL_METADATA_FILE);
    let temporary = data_root.join(format!("{CONTROL_METADATA_FILE}.{}.tmp", Uuid::new_v4()));
    let result = (|| {
        fs::write(&temporary, serde_json::to_vec_pretty(metadata)?)
            .with_context(|| format!("write {}", temporary.display()))?;
        fs::rename(&temporary, &path).with_context(|| format!("replace {}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn lock_shared(data_root: &Path) -> Result<File> {
    let file = open_lock(data_root)?;
    FileExt::lock_shared(&file)?;
    Ok(file)
}

fn lock_exclusive(data_root: &Path) -> Result<File> {
    let file = open_lock(data_root)?;
    FileExt::lock_exclusive(&file)?;
    Ok(file)
}

fn open_lock(data_root: &Path) -> Result<File> {
    let path = data_root.join(CONTROL_METADATA_LOCK_FILE);
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))
}


#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn concurrent_metadata_updates_preserve_every_work_item() {
        let data = Arc::new(tempdir().unwrap());
        let work_items = (0..24).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
        let handles = work_items
            .iter()
            .copied()
            .map(|work_item_id| {
                let data = Arc::clone(&data);
                thread::spawn(move || {
                    record_work_item_intent(
                        data.path(),
                        work_item_id,
                        format!("objective-{work_item_id}"),
                        vec![AcceptanceCriterion {
                            id: format!("check-{work_item_id}"),
                            capability: "test".into(),
                            component: None,
                            required: true,
                        }],
                        Vec::new(),
                    )
                    .unwrap();
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap();
        }

        let metadata = load(data.path()).unwrap();
        assert_eq!(metadata.work_items.len(), work_items.len());
        for work_item_id in work_items {
            assert!(metadata.work_items.contains_key(&work_item_id.to_string()));
        }
    }
}
