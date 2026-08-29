use std::{
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde::Serialize;

use crate::repository;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageReport {
    pub data_root: PathBuf,
    pub cache_root: PathBuf,
    pub total_bytes: u64,
    pub queue_checkouts_bytes: u64,
    pub remote_checkouts_bytes: u64,
    pub runs_bytes: u64,
    pub worktrees_bytes: u64,
    pub shared_build_cache_bytes: u64,
    pub legacy_generated_bytes: u64,
    pub legacy_generated_directories: Vec<PathBuf>,
}

pub fn run_cli(arguments: &[String]) -> Result<()> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return usage();
    };
    match command {
        "report" => {
            if arguments.iter().skip(1).any(|arg| arg != "--json") {
                return usage();
            }
            let report = report()?;
            if arguments.iter().any(|arg| arg == "--json") {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_report(&report);
            }
            Ok(())
        }
        "gc" => {
            let dry_run = arguments.iter().any(|arg| arg == "--dry-run");
            let include_build_cache = arguments.iter().any(|arg| arg == "--include-build-cache");
            if arguments.iter().skip(1).any(|arg| {
                arg != "--dry-run" && arg != "--include-build-cache" && arg != "--json"
            }) {
                return usage();
            }
            let result = gc(dry_run, include_build_cache)?;
            if arguments.iter().any(|arg| arg == "--json") {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!(
                    "Agent Loop storage GC {}: reclaimed {} from {} generated directories{}.",
                    if dry_run { "dry run" } else { "finished" },
                    human_bytes(result.reclaimed_bytes),
                    result.removed_directories.len(),
                    if include_build_cache {
                        " including the shared build cache"
                    } else {
                        ""
                    }
                );
                for path in &result.removed_directories {
                    println!("  {}", path.display());
                }
            }
            Ok(())
        }
        _ => usage(),
    }
}

fn usage() -> Result<()> {
    bail!(
        "usage: agent-loop storage report [--json] | agent-loop storage gc [--dry-run] [--include-build-cache] [--json]"
    )
}

pub fn report() -> Result<StorageReport> {
    let data_root = repository::data_directory()?;
    let cache_root = build_cache_root()?;
    let queue_checkouts = data_root.join("queue-checkouts");
    let remote_checkouts = data_root.join("remote-checkouts");
    let runs = data_root.join("runs");
    let worktrees = data_root.join("worktrees");
    let legacy_generated_directories = generated_directories(&[
        queue_checkouts.clone(),
        remote_checkouts.clone(),
        worktrees.clone(),
    ])?;
    let legacy_generated_bytes = legacy_generated_directories
        .iter()
        .map(|path| directory_size(path).unwrap_or(0))
        .sum();
    let queue_checkouts_bytes = directory_size(&queue_checkouts)?;
    let remote_checkouts_bytes = directory_size(&remote_checkouts)?;
    let runs_bytes = directory_size(&runs)?;
    let worktrees_bytes = directory_size(&worktrees)?;
    let shared_build_cache_bytes = directory_size(&cache_root)?;
    Ok(StorageReport {
        data_root,
        cache_root,
        total_bytes: queue_checkouts_bytes
            + remote_checkouts_bytes
            + runs_bytes
            + worktrees_bytes
            + shared_build_cache_bytes,
        queue_checkouts_bytes,
        remote_checkouts_bytes,
        runs_bytes,
        worktrees_bytes,
        shared_build_cache_bytes,
        legacy_generated_bytes,
        legacy_generated_directories,
    })
}

fn print_report(report: &StorageReport) {
    println!("Agent Loop storage");
    println!("  Queue checkouts:       {}", human_bytes(report.queue_checkouts_bytes));
    println!("  Remote checkouts:      {}", human_bytes(report.remote_checkouts_bytes));
    println!("  Run evidence:          {}", human_bytes(report.runs_bytes));
    println!("  Temporary worktrees:   {}", human_bytes(report.worktrees_bytes));
    println!("  Shared Rust cache:      {}", human_bytes(report.shared_build_cache_bytes));
    println!("  Total:                  {}", human_bytes(report.total_bytes));
    if report.legacy_generated_bytes > 0 {
        println!();
        println!(
            "  Reclaimable legacy checkout-local build data: {}",
            human_bytes(report.legacy_generated_bytes)
        );
        for path in &report.legacy_generated_directories {
            println!("    {}", path.display());
        }
        println!("  Run `agent-loop storage gc` to remove this derived data.");
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageGcResult {
    pub dry_run: bool,
    pub included_build_cache: bool,
    pub reclaimed_bytes: u64,
    pub removed_directories: Vec<PathBuf>,
}

pub fn gc(dry_run: bool, include_build_cache: bool) -> Result<StorageGcResult> {
    let data_root = repository::data_directory()?;
    let _queue_lock = try_queue_lock(&data_root)?;
    let roots = [
        data_root.join("queue-checkouts"),
        data_root.join("remote-checkouts"),
        data_root.join("worktrees"),
    ];
    let mut directories = generated_directories(&roots)?;
    if include_build_cache {
        let cache = build_cache_root()?;
        if cache.exists() {
            directories.push(cache);
        }
    }
    directories.sort();
    directories.dedup();

    let mut reclaimed_bytes = 0;
    let mut removed_directories = Vec::new();
    for path in directories {
        if is_tracked_generated_directory(&path)? {
            continue;
        }
        let bytes = directory_size(&path)?;
        reclaimed_bytes += bytes;
        removed_directories.push(path.clone());
        if !dry_run && path.exists() {
            fs::remove_dir_all(&path)
                .with_context(|| format!("remove generated directory {}", path.display()))?;
        }
    }
    Ok(StorageGcResult {
        dry_run,
        included_build_cache: include_build_cache,
        reclaimed_bytes,
        removed_directories,
    })
}

fn try_queue_lock(data_root: &Path) -> Result<Option<File>> {
    let path = data_root.join("queue-state.lock");
    fs::create_dir_all(data_root)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    match FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(Some(file)),
        Err(error) if error.kind() == ErrorKind::WouldBlock => {
            bail!("Agent Loop queue is active; storage GC refuses to race a running queue")
        }
        Err(error) => Err(error).context("lock Agent Loop queue for storage GC"),
    }
}

fn build_cache_root() -> Result<PathBuf> {
    Ok(dirs::cache_dir()
        .context("determine user cache directory")?
        .join("agent-loop-orchestrator")
        .join("builds"))
}

fn generated_directories(roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for root in roots {
        collect_generated_directories(root, &mut result)?;
    }
    Ok(result)
}

fn collect_generated_directories(path: &Path, result: &mut Vec<PathBuf>) -> Result<()> {
    if !path.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let child = entry.path();
        let name = entry.file_name();
        if name == "target" || name == "node_modules" || name == ".next" {
            result.push(child);
            continue;
        }
        collect_generated_directories(&child, result)?;
    }
    Ok(())
}

fn is_tracked_generated_directory(path: &Path) -> Result<bool> {
    let mut current = path.parent();
    while let Some(candidate) = current {
        if candidate.join(".git").exists() {
            let relative = path.strip_prefix(candidate).unwrap_or(path);
            let output = Command::new("git")
                .args(["ls-files", "--"])
                .arg(relative)
                .current_dir(candidate)
                .output()
                .context("check whether generated directory contains tracked files")?;
            return Ok(output.status.success() && !output.stdout.is_empty());
        }
        current = candidate.parent();
    }
    Ok(false)
}

fn directory_size(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(0);
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    let mut total = 0;
    for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        total += directory_size(&entry?.path())?;
    }
    Ok(total)
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn generated_directory_scan_stops_at_large_cache_roots() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("repo/target/deep")).unwrap();
        fs::create_dir_all(root.path().join("repo/node_modules/pkg")).unwrap();
        fs::create_dir_all(root.path().join("repo/src/nested")).unwrap();
        let found = generated_directories(&[root.path().to_path_buf()]).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|path| path.ends_with("target")));
        assert!(found.iter().any(|path| path.ends_with("node_modules")));
    }

    #[test]
    fn humanizes_large_storage_values() {
        assert_eq!(human_bytes(68 * 1024 * 1024 * 1024), "68.0 GiB");
    }
}
