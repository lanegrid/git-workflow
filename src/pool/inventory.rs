//! Pool inventory types and JSON persistence

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{GwError, Result};

/// Current inventory format version
const INVENTORY_VERSION: u32 = 1;

/// Status of a worktree in the pool
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// A single worktree entry in the pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolEntry {
    pub name: String,
    pub path: String,
    pub branch: String,
    pub status: WorktreeStatus,
    pub created_at: u64,
    pub acquired_at: Option<u64>,
    pub acquired_by: Option<u32>,
}

/// The pool inventory stored as JSON
#[derive(Debug, Serialize, Deserialize)]
pub struct Inventory {
    pub version: u32,
    pub worktrees: Vec<PoolEntry>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

impl Inventory {
    /// Create a new empty inventory
    pub fn new() -> Self {
        Self {
            version: INVENTORY_VERSION,
            worktrees: Vec::new(),
        }
    }

    /// Load inventory from a file, or create a new one if it doesn't exist
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let data = fs::read_to_string(path)?;
        serde_json::from_str(&data)
            .map_err(|e| GwError::Other(format!("Failed to parse inventory: {e}")))
    }

    /// Save inventory to a file
    pub fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| GwError::Other(format!("Failed to serialize inventory: {e}")))?;
        fs::write(path, data)?;
        Ok(())
    }

    /// Count worktrees with a given status
    pub fn count_by_status(&self, status: &WorktreeStatus) -> usize {
        self.worktrees
            .iter()
            .filter(|w| w.status == *status)
            .count()
    }

    /// Find the next available pool name (pool-NNN)
    pub fn next_name(&self) -> String {
        let max = self
            .worktrees
            .iter()
            .filter_map(|w| w.name.strip_prefix("pool-"))
            .filter_map(|n| n.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("pool-{:03}", max + 1)
    }

    /// Find the first available worktree
    pub fn find_available(&self) -> Option<usize> {
        self.worktrees
            .iter()
            .position(|w| w.status == WorktreeStatus::Available)
    }

    /// Find a worktree by name or path
    pub fn find_by_name_or_path(&self, identifier: &str) -> Option<usize> {
        self.worktrees
            .iter()
            .position(|w| w.name == identifier || w.path == identifier)
    }
}
