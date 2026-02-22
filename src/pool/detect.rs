//! Filesystem-derived pool state detection
//!
//! Instead of maintaining an inventory.json file, pool state is derived
//! from the filesystem: directory existence determines pool membership,
//! and marker files in `.git/worktree-pool/acquired/` determine status.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Status of a worktree in the pool, derived from filesystem state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeStatus {
    Available,
    Acquired,
}

impl std::fmt::Display for WorktreeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorktreeStatus::Available => write!(f, "available"),
            WorktreeStatus::Acquired => write!(f, "acquired"),
        }
    }
}

/// A single worktree entry derived from filesystem scan
#[derive(Debug, Clone)]
pub struct PoolEntry {
    pub name: String,
    pub path: PathBuf,
    pub branch: String,
    pub status: WorktreeStatus,
    pub owner: Option<String>,
}

/// Recommended next action for pool management
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolNextAction {
    /// No pool exists yet
    WarmPool,
    /// Worktrees available, some acquired
    Ready { available: usize },
    /// All worktrees in use
    Exhausted { acquired: usize },
    /// All worktrees available, no work in progress
    AllIdle { available: usize },
}

/// Snapshot of the pool state, derived from the filesystem
#[derive(Debug)]
pub struct PoolState {
    pub entries: Vec<PoolEntry>,
}

impl PoolState {
    /// Scan the filesystem to build pool state.
    ///
    /// - `worktrees_dir`: path to `.worktrees/`
    /// - `acquired_dir`: path to `.git/worktree-pool/acquired/`
    pub fn scan(worktrees_dir: &Path, acquired_dir: &Path) -> Result<Self> {
        let mut entries = Vec::new();

        if !worktrees_dir.exists() {
            return Ok(Self { entries });
        }

        let mut dirs: Vec<_> = fs::read_dir(worktrees_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && e.file_name()
                        .to_str()
                        .is_some_and(|n| n.starts_with("pool-"))
            })
            .collect();

        // Sort by name for deterministic ordering
        dirs.sort_by_key(|e| e.file_name());

        for dir_entry in dirs {
            let name = dir_entry.file_name().to_string_lossy().to_string();
            let path = dir_entry.path();
            let branch = name.clone();
            let marker = acquired_dir.join(&name);

            let (status, owner) = if marker.exists() {
                let owner = fs::read_to_string(&marker)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                (WorktreeStatus::Acquired, owner)
            } else {
                (WorktreeStatus::Available, None)
            };

            entries.push(PoolEntry {
                name,
                path,
                branch,
                status,
                owner,
            });
        }

        Ok(Self { entries })
    }

    /// Count worktrees with a given status
    pub fn count_by_status(&self, status: &WorktreeStatus) -> usize {
        self.entries.iter().filter(|e| e.status == *status).count()
    }

    /// Find the first available worktree
    pub fn find_available(&self) -> Option<&PoolEntry> {
        self.entries
            .iter()
            .find(|e| e.status == WorktreeStatus::Available)
    }

    /// Find a worktree by name or path
    pub fn find_by_name_or_path(&self, identifier: &str) -> Option<&PoolEntry> {
        self.entries
            .iter()
            .find(|e| e.name == identifier || e.path.to_string_lossy() == identifier)
    }

    /// Find the next pool name (pool-NNN) based on existing entries
    pub fn next_name(&self) -> String {
        let max = self
            .entries
            .iter()
            .filter_map(|e| e.name.strip_prefix("pool-"))
            .filter_map(|n| n.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("pool-{:03}", max + 1)
    }

    /// Determine the recommended next action
    pub fn next_action(&self) -> PoolNextAction {
        if self.entries.is_empty() {
            return PoolNextAction::WarmPool;
        }

        let available = self.count_by_status(&WorktreeStatus::Available);
        let acquired = self.count_by_status(&WorktreeStatus::Acquired);

        if available == 0 {
            return PoolNextAction::Exhausted { acquired };
        }

        if acquired == 0 {
            return PoolNextAction::AllIdle { available };
        }

        PoolNextAction::Ready { available }
    }
}
