//! Read-only git operations

use std::path::PathBuf;
use std::process::Command;

use crate::error::{GwError, Result};

/// Execute a git command and return stdout as string
fn git_output(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| GwError::GitCommandFailed(format!("Failed to execute git: {e}")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(GwError::GitCommandFailed(stderr))
    }
}

/// Execute a git command and check if it succeeded (ignoring output)
fn git_check(args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if we're in a git repository
pub fn is_git_repo() -> bool {
    git_check(&["rev-parse", "--git-dir"])
}

/// Get the current branch name
pub fn current_branch() -> Result<String> {
    git_output(&["rev-parse", "--abbrev-ref", "HEAD"])
}

/// Get the worktree root (the top-level working directory of the current worktree)
pub fn worktree_root() -> Result<PathBuf> {
    git_output(&["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

/// Get the git directory (.git or .git/worktrees/xxx)
pub fn git_dir() -> Result<PathBuf> {
    git_output(&["rev-parse", "--git-dir"]).map(PathBuf::from)
}

/// Get the common git directory (always .git of main repo)
pub fn git_common_dir() -> Result<PathBuf> {
    git_output(&["rev-parse", "--git-common-dir"]).map(PathBuf::from)
}

/// Check if we're in a worktree (not the main repo)
pub fn is_worktree() -> Result<bool> {
    let git_dir = git_dir()?;
    let common_dir = git_common_dir()?;
    Ok(git_dir != common_dir)
}

/// Check if a branch exists locally
pub fn branch_exists(branch: &str) -> bool {
    git_check(&[
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/heads/{branch}"),
    ])
}

/// Check if a branch exists on remote origin.
///
/// Returns `Ok(true)` / `Ok(false)` only when the answer is definitive:
/// `git ls-remote --exit-code` exits 0 when the ref exists and 2 when it
/// provably does not. Any other exit (no `origin`, network/auth failure) is
/// returned as an `Err` so callers can distinguish "not there" from "couldn't
/// check" — the latter must never be treated as safe-to-delete.
pub fn remote_branch_exists(branch: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["ls-remote", "--exit-code", "--heads", "origin", branch])
        .output()
        .map_err(|e| GwError::GitCommandFailed(format!("Failed to execute git: {e}")))?;

    match output.status.code() {
        Some(0) => Ok(true),
        Some(2) => Ok(false),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(GwError::GitCommandFailed(format!(
                "Could not query remote branch '{branch}': {stderr}"
            )))
        }
    }
}

/// Read the recorded base branch for a branch (`branch.<name>.gwBase`).
///
/// `gw new --stack` records the parent here so the workflow knows a branch is
/// stacked *before* its PR exists; once a PR exists, GitHub's base is the source
/// of truth instead. Returns `None` when unset (the branch targets the default
/// branch). It is a local config read — no network.
pub fn branch_base(branch: &str) -> Option<String> {
    let value = git_output(&["config", "--get", &format!("branch.{branch}.gwBase")]).ok()?;
    if value.is_empty() { None } else { Some(value) }
}

/// Get the current HEAD commit hash
pub fn head_commit() -> Result<String> {
    git_output(&["rev-parse", "HEAD"])
}

/// Get the short commit hash
pub fn short_commit() -> Result<String> {
    git_output(&["rev-parse", "--short", "HEAD"])
}

/// Get the commit message of HEAD
pub fn head_commit_message() -> Result<String> {
    git_output(&["log", "-1", "--format=%s"])
}

/// Check if working directory has unstaged changes
pub fn has_unstaged_changes() -> bool {
    !git_check(&["diff", "--quiet"])
}

/// Check if working directory has staged changes
pub fn has_staged_changes() -> bool {
    !git_check(&["diff", "--cached", "--quiet"])
}

/// Check if working directory has any uncommitted changes (staged or unstaged)
pub fn has_uncommitted_changes() -> bool {
    has_unstaged_changes() || has_staged_changes()
}

/// Check if working directory has untracked files
pub fn has_untracked_files() -> bool {
    git_output(&["ls-files", "--others", "--exclude-standard"])
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// Get the upstream tracking branch for a local branch, if any
pub fn get_upstream(branch: &str) -> Option<String> {
    git_output(&[
        "rev-parse",
        "--abbrev-ref",
        &format!("{branch}@{{upstream}}"),
    ])
    .ok()
    .filter(|s| !s.is_empty())
}

/// Check if a branch has a remote tracking branch
pub fn has_remote_tracking(branch: &str) -> bool {
    get_upstream(branch).is_some()
}

/// Count commits between two refs (exclusive..inclusive)
pub fn commit_count(from: &str, to: &str) -> Result<usize> {
    let output = git_output(&["rev-list", "--count", &format!("{from}..{to}")])?;
    output
        .parse()
        .map_err(|_| GwError::GitCommandFailed("Failed to parse commit count".to_string()))
}

/// Get number of unpushed commits on a branch (compared to its upstream)
pub fn unpushed_commit_count(branch: &str) -> Result<usize> {
    let upstream = get_upstream(branch)
        .ok_or_else(|| GwError::Other(format!("Branch '{branch}' has no upstream")))?;
    commit_count(&upstream, branch)
}

/// Get number of commits behind upstream
pub fn behind_upstream_count(branch: &str) -> Result<usize> {
    let upstream = get_upstream(branch)
        .ok_or_else(|| GwError::Other(format!("Branch '{branch}' has no upstream")))?;
    commit_count(branch, &upstream)
}

/// Count stashes
pub fn stash_count() -> usize {
    git_output(&["stash", "list"])
        .map(|s| if s.is_empty() { 0 } else { s.lines().count() })
        .unwrap_or(0)
}

/// Get the message of the latest stash (stash@{0})
pub fn get_latest_stash_message() -> Option<String> {
    git_output(&["stash", "list", "-1", "--format=%gs"])
        .ok()
        .filter(|s| !s.is_empty())
}

/// Check if HEAD has a parent commit (i.e., we can undo)
pub fn has_commits_to_undo() -> bool {
    git_check(&["rev-parse", "HEAD~1"])
}

/// Get the current working directory name
pub fn current_dir_name() -> Result<String> {
    std::env::current_dir()
        .map_err(GwError::Io)?
        .file_name()
        .and_then(|s| s.to_str())
        .map(String::from)
        .ok_or_else(|| GwError::Other("Could not determine current directory name".to_string()))
}

/// Get the default remote branch (origin/main or origin/master)
pub fn get_default_remote_branch() -> Result<String> {
    // An unreachable remote is treated as "not found" here; default_branch_name()
    // falls back to local branches when this returns an error.
    if remote_branch_exists("main").unwrap_or(false) {
        Ok("origin/main".to_string())
    } else if remote_branch_exists("master").unwrap_or(false) {
        Ok("origin/master".to_string())
    } else {
        Err(GwError::Other(
            "Neither origin/main nor origin/master exists".to_string(),
        ))
    }
}

/// Get the default branch name (e.g. "main" or "master"), without the remote prefix.
///
/// Prefers the remote's default branch; falls back to a local `main`/`master`
/// for repositories without a configured `origin`. This is the single source of
/// truth for "the trunk branch", replacing scattered hardcoded "main" literals.
pub fn default_branch_name() -> Result<String> {
    if let Ok(remote) = get_default_remote_branch() {
        return Ok(remote
            .strip_prefix("origin/")
            .unwrap_or(&remote)
            .to_string());
    }
    if branch_exists("main") {
        return Ok("main".to_string());
    }
    if branch_exists("master") {
        return Ok("master".to_string());
    }
    Err(GwError::Other(
        "Could not determine the default branch (no origin/main, origin/master, or local main/master)"
            .to_string(),
    ))
}

/// Check if HEAD is detached
pub fn is_detached_head() -> bool {
    current_branch().map(|b| b == "HEAD").unwrap_or(false)
}

/// Get the top-level working directory of the repository
pub fn repo_root() -> Result<PathBuf> {
    git_output(&["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}
