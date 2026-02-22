//! Custom error types for gw CLI

use thiserror::Error;

/// Result type alias for gw operations
pub type Result<T> = std::result::Result<T, GwError>;

/// Custom error type for gw CLI
#[derive(Error, Debug)]
pub enum GwError {
    #[error("Not in a git repository")]
    NotAGitRepository,

    #[error("Git command failed: {0}")]
    GitCommandFailed(String),

    #[error("Branch '{0}' already exists")]
    BranchAlreadyExists(String),

    #[error("Branch '{0}' does not exist")]
    BranchNotFound(String),

    #[error("Cannot delete protected branch: {0}")]
    ProtectedBranch(String),

    #[error("Uncommitted changes detected. Commit or stash them first.")]
    UncommittedChanges,

    #[error("Branch '{0}' has {1} unpushed commit(s)")]
    UnpushedCommits(String, usize),

    #[error("Already on home branch '{0}'. Specify a branch to delete.")]
    AlreadyOnHomeBranch(String),

    #[error("Branch name is required")]
    BranchNameRequired,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Cannot undo: no commits to undo (initial commit or empty history)")]
    NothingToUndo,

    #[error("Worktree pool is not initialized. Run `gw worktree pool warm <n>` first.")]
    PoolNotInitialized,

    #[error("No available worktrees in the pool. Run `gw worktree pool warm <n>` to add more.")]
    PoolExhausted,

    #[error("Worktree not found in pool: {0}")]
    PoolWorktreeNotFound(String),

    #[error("Pool has {0} acquired worktree(s). Release them first or use --force.")]
    PoolHasAcquiredWorktrees(usize),

    #[error("Could not acquire pool lock within timeout")]
    PoolLockTimeout,

    #[error("{0}")]
    Other(String),
}
