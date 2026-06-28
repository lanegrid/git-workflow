//! Repository type detection (main repo vs worktree)

use crate::error::Result;
use crate::git;

/// Type of git repository
#[derive(Debug, Clone)]
pub enum RepoType {
    /// Main repository (home branch is the repo's default branch, e.g. main/master)
    MainRepo { home_branch: String },
    /// Worktree (home branch is directory name)
    Worktree { home_branch: String },
}

impl RepoType {
    /// Detect the current repository type
    pub fn detect() -> Result<Self> {
        if git::is_worktree()? {
            let home_branch = git::current_dir_name()?;
            Ok(RepoType::Worktree { home_branch })
        } else {
            // The main repo's home is the default branch — main OR master.
            let home_branch = git::default_branch_name()?;
            Ok(RepoType::MainRepo { home_branch })
        }
    }

    /// Get the home branch for this repository type
    pub fn home_branch(&self) -> &str {
        match self {
            RepoType::MainRepo { home_branch } => home_branch,
            RepoType::Worktree { home_branch } => home_branch,
        }
    }

    /// Check if a branch is protected (cannot be deleted)
    pub fn is_protected(&self, branch: &str) -> bool {
        // Always protect main and master
        if branch == "main" || branch == "master" {
            return true;
        }
        // Protect the home branch of this repo
        branch == self.home_branch()
    }
}
