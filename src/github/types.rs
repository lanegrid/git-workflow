//! GitHub data types

/// PR state from GitHub
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrState {
    /// PR is open
    Open,
    /// PR was merged
    Merged {
        /// How the PR was merged
        method: MergeMethod,
        /// The merge commit SHA (for regular merge) or squash commit SHA
        merge_commit: Option<String>,
    },
    /// PR was closed without merging
    Closed,
}

impl PrState {
    /// Check if the PR is merged
    pub fn is_merged(&self) -> bool {
        matches!(self, PrState::Merged { .. })
    }

    /// Check if the PR is open
    pub fn is_open(&self) -> bool {
        matches!(self, PrState::Open)
    }

    /// Check if the PR is closed (without merging)
    pub fn is_closed(&self) -> bool {
        matches!(self, PrState::Closed)
    }
}

/// How a PR was merged
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    /// Regular merge commit
    Merge,
    /// Squash and merge
    Squash,
    /// Rebase and merge
    Rebase,
}

impl std::fmt::Display for MergeMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeMethod::Merge => write!(f, "merge"),
            MergeMethod::Squash => write!(f, "squash"),
            MergeMethod::Rebase => write!(f, "rebase"),
        }
    }
}

/// Information about a PR associated with a branch
#[derive(Debug, Clone)]
pub struct PrInfo {
    /// PR number
    pub number: u64,
    /// PR title
    pub title: String,
    /// PR URL
    pub url: String,
    /// PR state
    pub state: PrState,
    /// Base branch (e.g., "main")
    pub base_branch: String,
    /// Head branch (the PR's own branch, e.g., "feature/x")
    pub head_branch: String,
}

impl PrInfo {
    /// Create a new PrInfo for testing (head branch defaults to empty)
    pub fn new(number: u64, title: &str, url: &str, state: PrState, base_branch: &str) -> Self {
        Self {
            number,
            title: title.to_string(),
            url: url.to_string(),
            state,
            base_branch: base_branch.to_string(),
            head_branch: String::new(),
        }
    }
}

/// Raw PR data from JSON (before merge method detection)
#[derive(Debug, Clone)]
pub struct RawPrData {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub base_branch: String,
    pub head_branch: String,
    pub merge_commit: Option<String>,
}
