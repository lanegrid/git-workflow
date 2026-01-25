//! GitHub client abstraction
//!
//! Provides a trait-based interface for GitHub operations,
//! allowing for dependency injection and testing.

use std::process::Command;

use crate::error::{GwError, Result};

use super::parser::parse_pr_json;
use super::types::{MergeMethod, PrInfo, PrState, RawPrData};

/// Output from a command execution
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            success: true,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub fn failure(stderr: impl Into<String>) -> Self {
        Self {
            success: false,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }
}

/// Trait for executing shell commands
///
/// This abstraction allows mocking command execution in tests.
pub trait CommandExecutor: Send + Sync {
    /// Execute a command and return its output
    fn execute(&self, program: &str, args: &[&str]) -> Result<CommandOutput>;

    /// Execute a command in a specific directory
    fn execute_in_dir(&self, program: &str, args: &[&str], dir: &str) -> Result<CommandOutput>;
}

/// Real command executor that runs actual shell commands
#[derive(Debug, Default)]
pub struct RealCommandExecutor;

impl CommandExecutor for RealCommandExecutor {
    fn execute(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| GwError::GitCommandFailed(format!("Failed to execute {program}: {e}")))?;

        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    fn execute_in_dir(&self, program: &str, args: &[&str], dir: &str) -> Result<CommandOutput> {
        let output = Command::new(program)
            .args(args)
            .current_dir(dir)
            .output()
            .map_err(|e| GwError::GitCommandFailed(format!("Failed to execute {program}: {e}")))?;

        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

/// GitHub client for interacting with GitHub via gh CLI
pub struct GitHubClient<E: CommandExecutor = RealCommandExecutor> {
    executor: E,
}

impl Default for GitHubClient<RealCommandExecutor> {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubClient<RealCommandExecutor> {
    /// Create a new GitHubClient with the real command executor
    pub fn new() -> Self {
        Self {
            executor: RealCommandExecutor,
        }
    }
}

impl<E: CommandExecutor> GitHubClient<E> {
    /// Create a GitHubClient with a custom command executor (for testing)
    pub fn with_executor(executor: E) -> Self {
        Self { executor }
    }

    /// Check if `gh` CLI is available
    pub fn is_available(&self) -> bool {
        self.executor
            .execute("gh", &["--version"])
            .map(|o| o.success)
            .unwrap_or(false)
    }

    /// Check if `gh` is authenticated
    pub fn is_authenticated(&self) -> bool {
        self.executor
            .execute("gh", &["auth", "status"])
            .map(|o| o.success)
            .unwrap_or(false)
    }

    /// Get PR information for a branch
    ///
    /// Returns `None` if no PR exists for this branch.
    pub fn get_pr_for_branch(&self, branch: &str) -> Result<Option<PrInfo>> {
        let output = self.executor.execute(
            "gh",
            &[
                "pr",
                "view",
                branch,
                "--json",
                "number,title,url,state,baseRefName,mergeCommit,mergedAt",
            ],
        )?;

        if !output.success {
            return self.handle_pr_view_error(&output.stderr);
        }

        let raw = parse_pr_json(&output.stdout)?;
        let pr_info = self.convert_raw_to_pr_info(raw)?;
        Ok(Some(pr_info))
    }

    /// Delete a remote branch
    pub fn delete_remote_branch(&self, branch: &str) -> Result<()> {
        let output = self
            .executor
            .execute("git", &["push", "origin", "--delete", branch])?;

        if output.success {
            Ok(())
        } else if output.stderr.contains("remote ref does not exist") {
            // Branch already deleted is not an error
            Ok(())
        } else {
            Err(GwError::GitCommandFailed(format!(
                "Failed to delete remote branch: {}",
                output.stderr.trim()
            )))
        }
    }

    /// Add a comment to a PR
    pub fn add_pr_comment(&self, pr_number: u64, comment: &str) -> Result<()> {
        let output = self.executor.execute(
            "gh",
            &["pr", "comment", &pr_number.to_string(), "-b", comment],
        )?;

        if output.success {
            Ok(())
        } else {
            Err(GwError::GitCommandFailed(format!(
                "Failed to add PR comment: {}",
                output.stderr.trim()
            )))
        }
    }

    /// Update PR base branch
    pub fn update_pr_base(&self, pr_number: u64, new_base: &str) -> Result<()> {
        let output = self.executor.execute(
            "gh",
            &["pr", "edit", &pr_number.to_string(), "--base", new_base],
        )?;

        if output.success {
            Ok(())
        } else {
            Err(GwError::GitCommandFailed(format!(
                "Failed to update PR base: {}",
                output.stderr.trim()
            )))
        }
    }

    /// Handle error from `gh pr view`
    fn handle_pr_view_error(&self, stderr: &str) -> Result<Option<PrInfo>> {
        if stderr.contains("no pull requests found") || stderr.contains("Could not resolve") {
            return Ok(None);
        }
        if stderr.contains("auth login") {
            return Err(GwError::Other(
                "GitHub CLI not authenticated. Run: gh auth login".to_string(),
            ));
        }
        Err(GwError::GitCommandFailed(format!(
            "gh pr view failed: {}",
            stderr.trim()
        )))
    }

    /// Convert raw PR data to PrInfo, detecting merge method
    fn convert_raw_to_pr_info(&self, raw: RawPrData) -> Result<PrInfo> {
        let state = match raw.state.as_str() {
            "OPEN" => PrState::Open,
            "MERGED" => {
                let method = self.detect_merge_method(&raw.merge_commit);
                PrState::Merged {
                    method,
                    merge_commit: raw.merge_commit,
                }
            }
            "CLOSED" => PrState::Closed,
            _ => PrState::Closed,
        };

        Ok(PrInfo {
            number: raw.number,
            title: raw.title,
            url: raw.url,
            state,
            base_branch: raw.base_branch,
        })
    }

    /// Detect merge method from merge commit
    ///
    /// Note: GitHub API doesn't directly expose merge method for merged PRs.
    /// We infer it by checking commit parent count:
    /// - 2 parents -> regular merge
    /// - 1 parent -> squash or rebase
    /// - No merge commit -> rebase
    fn detect_merge_method(&self, merge_commit: &Option<String>) -> MergeMethod {
        let Some(sha) = merge_commit else {
            return MergeMethod::Rebase;
        };

        let Ok(output) = self.executor.execute("git", &["cat-file", "-p", sha]) else {
            return MergeMethod::Squash;
        };

        if !output.success {
            return MergeMethod::Squash;
        }

        let parent_count = output
            .stdout
            .lines()
            .filter(|l| l.starts_with("parent "))
            .count();

        match parent_count {
            2 => MergeMethod::Merge,
            1 => MergeMethod::Squash,
            _ => MergeMethod::Squash,
        }
    }
}

// Convenience functions using the default client (backward compatibility)

/// Check if `gh` CLI is available
pub fn is_gh_available() -> bool {
    GitHubClient::new().is_available()
}

/// Check if `gh` is authenticated
pub fn is_gh_authenticated() -> bool {
    GitHubClient::new().is_authenticated()
}

/// Get PR information for a branch
pub fn get_pr_for_branch(branch: &str) -> Result<Option<PrInfo>> {
    GitHubClient::new().get_pr_for_branch(branch)
}

/// Delete a remote branch
pub fn delete_remote_branch(branch: &str) -> Result<()> {
    GitHubClient::new().delete_remote_branch(branch)
}

/// Add a comment to a PR
pub fn add_pr_comment(pr_number: u64, comment: &str) -> Result<()> {
    GitHubClient::new().add_pr_comment(pr_number, comment)
}

/// Update PR base branch
pub fn update_pr_base(pr_number: u64, new_base: &str) -> Result<()> {
    GitHubClient::new().update_pr_base(pr_number, new_base)
}
