//! File-based locking for the worktree pool using flock (via fs2)

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;

use crate::error::{GwError, Result};

/// Default lock timeout
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// Retry interval between lock attempts
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// A held file lock on the pool
pub struct PoolLock {
    _file: File,
    _path: PathBuf,
}

impl PoolLock {
    /// Acquire an exclusive lock on the pool directory with retry.
    /// Creates the pool directory and lock file if they don't exist.
    /// Retries for up to 10 seconds before returning PoolLockTimeout.
    pub fn acquire(pool_dir: &Path) -> Result<Self> {
        Self::acquire_with_timeout(pool_dir, LOCK_TIMEOUT)
    }

    /// Acquire an exclusive lock with a custom timeout.
    pub fn acquire_with_timeout(pool_dir: &Path, timeout: Duration) -> Result<Self> {
        fs::create_dir_all(pool_dir)?;
        let lock_path = pool_dir.join("pool.lock");
        let file = File::create(&lock_path)?;

        let start = Instant::now();
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => {
                    return Ok(Self {
                        _file: file,
                        _path: lock_path,
                    });
                }
                Err(_) if start.elapsed() < timeout => {
                    thread::sleep(LOCK_RETRY_INTERVAL);
                }
                Err(_) => {
                    return Err(GwError::PoolLockTimeout);
                }
            }
        }
    }
}

// Lock is released when PoolLock is dropped (File close releases flock)
