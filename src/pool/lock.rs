//! File-based locking for the worktree pool using flock (via fs2)

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::error::{GwError, Result};

/// A held file lock on the pool
pub struct PoolLock {
    _file: File,
    _path: PathBuf,
}

impl PoolLock {
    /// Acquire an exclusive lock on the pool directory.
    /// Creates the pool directory and lock file if they don't exist.
    pub fn acquire(pool_dir: &Path) -> Result<Self> {
        fs::create_dir_all(pool_dir)?;
        let lock_path = pool_dir.join("pool.lock");
        let file = File::create(&lock_path)?;
        file.try_lock_exclusive()
            .map_err(|_| GwError::PoolLockTimeout)?;
        Ok(Self {
            _file: file,
            _path: lock_path,
        })
    }
}

// Lock is released when PoolLock is dropped (File close releases flock)
