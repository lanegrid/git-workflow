//! GitHub CLI (gh) integration
//!
//! Uses `gh` CLI to query PR information and perform GitHub operations.
//!
//! # Architecture
//!
//! This module uses a trait-based design for testability:
//!
//! - [`CommandExecutor`] - Trait for executing shell commands
//! - [`GitHubClient`] - Main client that uses a CommandExecutor
//! - [`MockCommandExecutor`] - Mock implementation for testing
//!
//! # Example: Using in tests
//!
//! ```ignore
//! use gw::github::{GitHubClient, MockScenarioBuilder, fixtures};
//!
//! let executor = MockScenarioBuilder::new()
//!     .gh_available()
//!     .with_pr("feature/test", &fixtures::open_pr(42, "feature/test"))
//!     .build();
//!
//! let client = GitHubClient::with_executor(executor);
//! let pr = client.get_pr_for_branch("feature/test").unwrap().unwrap();
//! assert!(pr.state.is_open());
//! ```

mod client;
mod mock;
mod parser;
#[cfg(test)]
mod tests;
mod types;

// Re-export types
pub use types::{MergeMethod, PrInfo, PrState, RawPrData};

// Re-export client
pub use client::{
    CommandExecutor, CommandOutput, GitHubClient, RealCommandExecutor, add_pr_comment,
    delete_remote_branch, get_pr_for_branch, is_gh_authenticated, is_gh_available, update_pr_base,
};

// Re-export mock (for testing in other modules)
pub use mock::{CommandCall, MockCommandExecutor, MockScenarioBuilder, fixtures};
