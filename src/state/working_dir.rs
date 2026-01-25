//! Working directory state

use crate::git;

/// State of the working directory
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkingDirState {
    /// No uncommitted changes
    Clean,
    /// Has unstaged changes only
    HasUnstagedChanges,
    /// Has staged changes only
    HasStagedChanges,
    /// Has both staged and unstaged changes
    HasMixedChanges,
}

impl WorkingDirState {
    /// Detect the current working directory state
    pub fn detect() -> Self {
        let has_unstaged = git::has_unstaged_changes();
        let has_staged = git::has_staged_changes();

        match (has_unstaged, has_staged) {
            (false, false) => WorkingDirState::Clean,
            (true, false) => WorkingDirState::HasUnstagedChanges,
            (false, true) => WorkingDirState::HasStagedChanges,
            (true, true) => WorkingDirState::HasMixedChanges,
        }
    }

    /// Check if the working directory is clean
    pub fn is_clean(&self) -> bool {
        matches!(self, WorkingDirState::Clean)
    }

    /// Get a human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            WorkingDirState::Clean => "clean",
            WorkingDirState::HasUnstagedChanges => "has unstaged changes",
            WorkingDirState::HasStagedChanges => "has staged changes",
            WorkingDirState::HasMixedChanges => "has staged and unstaged changes",
        }
    }
}
