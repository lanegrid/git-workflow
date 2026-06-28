//! Working directory state

use crate::git;

/// State of the working directory
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkingDirState {
    /// No uncommitted changes and no untracked files
    Clean,
    /// Has unstaged changes only
    HasUnstagedChanges,
    /// Has staged changes only
    HasStagedChanges,
    /// Has both staged and unstaged changes
    HasMixedChanges,
    /// Has untracked files only (no staged or unstaged tracked changes)
    HasUntrackedFiles,
}

impl WorkingDirState {
    /// Detect the current working directory state
    ///
    /// Untracked files count as "not clean": leaving them out caused `gw pause`
    /// to skip the WIP commit (and switch home) when the only work was new,
    /// unstaged files — silently leaving them behind. Tracked changes take
    /// precedence for the description; untracked-only maps to `HasUntrackedFiles`.
    pub fn detect() -> Self {
        let has_unstaged = git::has_unstaged_changes();
        let has_staged = git::has_staged_changes();
        let has_untracked = git::has_untracked_files();

        match (has_unstaged, has_staged) {
            (true, true) => WorkingDirState::HasMixedChanges,
            (true, false) => WorkingDirState::HasUnstagedChanges,
            (false, true) => WorkingDirState::HasStagedChanges,
            (false, false) => {
                if has_untracked {
                    WorkingDirState::HasUntrackedFiles
                } else {
                    WorkingDirState::Clean
                }
            }
        }
    }

    /// Check if the working directory is clean (no tracked changes, no untracked files)
    pub fn is_clean(&self) -> bool {
        matches!(self, WorkingDirState::Clean)
    }

    /// Whether there are tracked changes (staged and/or unstaged), ignoring
    /// untracked files.
    pub fn has_tracked_changes(&self) -> bool {
        matches!(
            self,
            WorkingDirState::HasUnstagedChanges
                | WorkingDirState::HasStagedChanges
                | WorkingDirState::HasMixedChanges
        )
    }

    /// Get a human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            WorkingDirState::Clean => "clean",
            WorkingDirState::HasUnstagedChanges => "has unstaged changes",
            WorkingDirState::HasStagedChanges => "has staged changes",
            WorkingDirState::HasMixedChanges => "has staged and unstaged changes",
            WorkingDirState::HasUntrackedFiles => "has untracked files",
        }
    }
}
