//! State types for gw CLI

pub mod next_action;
pub mod repo;
pub mod sync;
pub mod working_dir;

pub use next_action::NextAction;
pub use repo::RepoType;
pub use sync::SyncState;
pub use working_dir::WorkingDirState;

use std::marker::PhantomData;

use crate::error::{GwError, Result};
use crate::git;

/// Marker type for protected branches (main, master, home)
pub struct Protected;

/// Marker type for deletable branches
pub struct Deletable;

/// A branch with compile-time safety for deletion
#[derive(Debug)]
pub struct Branch<Status> {
    name: String,
    _status: PhantomData<Status>,
}

impl<S> Branch<S> {
    /// Get the branch name
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Branch<Protected> {
    /// Create a protected branch (for main, master, home branches)
    pub fn protected(name: String) -> Self {
        Branch {
            name,
            _status: PhantomData,
        }
    }
}

impl Branch<Deletable> {
    /// Create a deletable branch
    pub fn deletable(name: String) -> Self {
        Branch {
            name,
            _status: PhantomData,
        }
    }

    /// Delete this branch (only possible for Deletable branches)
    pub fn delete(self, verbose: bool) -> Result<()> {
        git::delete_branch(&self.name, verbose)
    }
}

/// Classify a branch as protected or deletable
pub fn classify_branch(branch: &str, repo_type: &RepoType) -> BranchClassification {
    if repo_type.is_protected(branch) {
        BranchClassification::Protected(Branch::protected(branch.to_string()))
    } else {
        BranchClassification::Deletable(Branch::deletable(branch.to_string()))
    }
}

/// Result of classifying a branch
pub enum BranchClassification {
    Protected(Branch<Protected>),
    Deletable(Branch<Deletable>),
}

impl BranchClassification {
    /// Get the branch name regardless of classification
    pub fn name(&self) -> &str {
        match self {
            BranchClassification::Protected(b) => b.name(),
            BranchClassification::Deletable(b) => b.name(),
        }
    }

    /// Try to get as deletable, returning error if protected
    pub fn try_deletable(self) -> Result<Branch<Deletable>> {
        match self {
            BranchClassification::Protected(b) => {
                Err(GwError::ProtectedBranch(b.name().to_string()))
            }
            BranchClassification::Deletable(b) => Ok(b),
        }
    }
}
