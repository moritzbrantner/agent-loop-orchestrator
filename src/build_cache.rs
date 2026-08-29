use std::{
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCacheActivation {
    pub cargo_target_dir: PathBuf,
    pub managed_target_dir: bool,
}

/// Configure a shared, low-footprint Cargo build cache for child processes.
///
/// Agent Loop invokes both provider CLIs and deterministic tooling from several
/// isolated worktrees/checkouts. Without an inherited `CARGO_TARGET_DIR`, every
/// checkout grows an independent Rust `target/` tree. We establish the build
/// environment once, before Agent Loop starts worker threads, so all descendants
/// reuse one cache for the repository.
pub fn activate_for_current_repository() -> Result<Option<BuildCacheActivation>> {
    let cwd = env::current_dir().context("determine current directory for build cache")?;
    activate_for_repository(&cwd)
}

pub fn activate_for_repository(start: &Path) -> Result<Option<BuildCacheActivation>> {
    let Some(identity) = repository_identity(start)? else {
        return Ok(None);
    };

    let existing_target = env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
    let managed_target_dir = existing_target.is_none();
    let cargo_target_dir = match existing_target {
        Some(path) => path,
        None => {
            let cache_root = dirs::cache_dir()
                .context("determine user cache directory")?
                .join("agent-loop-orchestrator")
                .join("builds")
                .join(cache_key(&identity));
            let target = cache_root.join("cargo-target");
            fs::create_dir_all(&target)
                .with_context(|| format!("create shared Cargo target {}", target.display()))?;
            target
        }
    };

    // SAFETY: Agent Loop activates the build environment in `main` before it
    // starts the queue heartbeat or any provider/tooling worker threads.
    unsafe {
        if managed_target_dir {
            env::set_var("CARGO_TARGET_DIR", &cargo_target_dir);
        }
        set_if_missing("CARGO_INCREMENTAL", "0");
        set_if_missing("CARGO_PROFILE_DEV_DEBUG", "0");
        set_if_missing("CARGO_PROFILE_TEST_DEBUG", "0");
    }

    Ok(Some(BuildCacheActivation {
        cargo_target_dir,
        managed_target_dir,
    }))
}

unsafe fn set_if_missing(name: &str, value: &str) {
    if env::var_os(name).is_none() {
        // SAFETY: caller guarantees this happens before worker threads start.
        unsafe { env::set_var(name, value) };
    }
}

fn repository_identity(start: &Path) -> Result<Option<String>> {
    let root = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(start)
        .output()
        .with_context(|| format!("run git in {}", start.display()))?;
    if !root.status.success() {
        return Ok(None);
    }
    let root = String::from_utf8(root.stdout)?.trim().to_owned();

    let remote = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(&root)
        .output()
        .context("read origin URL for build cache identity")?;
    if remote.status.success() {
        let value = String::from_utf8(remote.stdout)?.trim().to_owned();
        if !value.is_empty() {
            return Ok(Some(format!("remote:{value}")));
        }
    }

    Ok(Some(format!("root:{root}")))
}

fn cache_key(identity: &str) -> String {
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_cache_keys_are_stable_and_separate() {
        assert_eq!(cache_key("remote:a"), cache_key("remote:a"));
        assert_ne!(cache_key("remote:a"), cache_key("remote:b"));
        assert_eq!(cache_key("remote:a").len(), 64);
    }
}
