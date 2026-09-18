//! Shared-clone coordination for stack mutations (including linked worktrees).

use std::fs::{File, OpenOptions};
use std::path::PathBuf;

use fs2::FileExt;

use crate::error::{GwError, Result};
use crate::git;

/// Held only during mutation, never during the PR polling loop. Closing the
/// descriptor releases the lock even if the process terminates unexpectedly.
pub struct LifecycleLock {
    _file: File,
}

impl LifecycleLock {
    pub fn acquire() -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(git::git_common_dir()?.join("gw-lifecycle.lock"))?;
        file.try_lock_exclusive().map_err(|e| {
            GwError::Other(format!(
                "Another gw stack mutation is running, or its lock is unavailable ({e}). Retry after it finishes."
            ))
        })?;
        Ok(Self { _file: file })
    }
}

pub fn sync_journal_path() -> Result<PathBuf> {
    Ok(git::git_common_dir()?.join("gw-sync.json"))
}

/// A failed/interrupted sync must finish before another lifecycle mutation.
pub fn require_no_pending_sync() -> Result<()> {
    let path = sync_journal_path()?;
    if path.try_exists()? {
        return Err(GwError::Other(format!(
            "An unfinished gw sync protects this stack. Rerun gw sync in its original worktree first (journal: {}).",
            path.display()
        )));
    }
    Ok(())
}
