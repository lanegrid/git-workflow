//! Sync state with remote

use crate::error::Result;
use crate::git;

/// Sync state with upstream
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    /// No upstream tracking branch
    NoUpstream,
    /// Synced with upstream
    Synced,
    /// Has commits not pushed to upstream
    HasUnpushedCommits { count: usize },
    /// Behind upstream
    Behind { count: usize },
    /// Diverged (both ahead and behind)
    Diverged { ahead: usize, behind: usize },
}

impl SyncState {
    /// Detect the sync state for a branch
    pub fn detect(branch: &str) -> Result<Self> {
        if !git::has_remote_tracking(branch) {
            return Ok(SyncState::NoUpstream);
        }

        let ahead = git::unpushed_commit_count(branch).unwrap_or(0);
        let behind = git::behind_upstream_count(branch).unwrap_or(0);

        Ok(match (ahead, behind) {
            (0, 0) => SyncState::Synced,
            (a, 0) if a > 0 => SyncState::HasUnpushedCommits { count: a },
            (0, b) if b > 0 => SyncState::Behind { count: b },
            (a, b) => SyncState::Diverged {
                ahead: a,
                behind: b,
            },
        })
    }

    /// Check if there are unpushed commits
    pub fn has_unpushed(&self) -> bool {
        matches!(
            self,
            SyncState::HasUnpushedCommits { .. } | SyncState::Diverged { .. }
        )
    }

    /// Get the number of unpushed commits, if any
    pub fn unpushed_count(&self) -> usize {
        match self {
            SyncState::HasUnpushedCommits { count } => *count,
            SyncState::Diverged { ahead, .. } => *ahead,
            _ => 0,
        }
    }

    /// Get a human-readable description
    pub fn description(&self) -> String {
        match self {
            SyncState::NoUpstream => "no upstream".to_string(),
            SyncState::Synced => "synced".to_string(),
            SyncState::HasUnpushedCommits { count } => format!("{count} unpushed commit(s)"),
            SyncState::Behind { count } => format!("{count} commit(s) behind"),
            SyncState::Diverged { ahead, behind } => {
                format!("{ahead} ahead, {behind} behind")
            }
        }
    }
}
