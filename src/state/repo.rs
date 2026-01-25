//! Repository type detection (main repo vs worktree)

use crate::error::Result;
use crate::git;

/// Type of git repository
#[derive(Debug, Clone)]
pub enum RepoType {
    /// Main repository (home branch is "main")
    MainRepo,
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
            Ok(RepoType::MainRepo)
        }
    }

    /// Get the home branch for this repository type
    pub fn home_branch(&self) -> &str {
        match self {
            RepoType::MainRepo => "main",
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
