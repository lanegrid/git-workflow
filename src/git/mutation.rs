//! State-changing git operations

use std::process::Command;

use crate::error::{GwError, Result};
use crate::output;

/// Execute a git command, showing the command if verbose
fn git_run(args: &[&str], verbose: bool) -> Result<()> {
    if verbose {
        output::action(&format!("git {}", args.join(" ")));
    }

    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| GwError::GitCommandFailed(format!("Failed to execute git: {e}")))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(GwError::GitCommandFailed(stderr))
    }
}

/// Execute a git command and return stdout
fn git_output(args: &[&str], verbose: bool) -> Result<String> {
    if verbose {
        output::action(&format!("git {}", args.join(" ")));
    }

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

/// Fetch from origin with prune
pub fn fetch_prune(verbose: bool) -> Result<()> {
    git_run(&["fetch", "--prune", "--quiet"], verbose)
}

/// Checkout an existing branch
pub fn checkout(branch: &str, verbose: bool) -> Result<()> {
    git_run(&["checkout", branch, "--quiet"], verbose).map_err(|e| map_checkout_error(branch, e))
}

/// Create and checkout a new branch from a starting point
pub fn checkout_new_branch(branch: &str, start_point: &str, verbose: bool) -> Result<()> {
    git_run(&["checkout", "-b", branch, start_point, "--quiet"], verbose)
        .map_err(|e| map_checkout_error(branch, e))
}

/// Turn git's raw "already checked out / used by worktree" failure into a
/// typed, actionable error. In a worktree setup the same branch can't be
/// checked out in two places, and git's bare fatal message doesn't tell the
/// user what to do; `BranchCheckedOutElsewhere` carries the conflicting path
/// and lets the CLI suggest next steps.
fn map_checkout_error(branch: &str, err: GwError) -> GwError {
    let GwError::GitCommandFailed(ref msg) = err else {
        return err;
    };
    if msg.contains("already checked out") || msg.contains("already used by worktree") {
        return GwError::BranchCheckedOutElsewhere {
            branch: branch.to_string(),
            path: extract_worktree_path(msg),
        };
    }
    err
}

/// Pull the worktree path out of git's message, e.g.
/// `fatal: 'main' is already checked out at '/path/to/wt'`.
fn extract_worktree_path(msg: &str) -> Option<String> {
    let start = msg.find("at '")? + "at '".len();
    let rest = &msg[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// Pull from a remote branch (fast-forward only, safe)
///
/// Returns an error if the pull cannot be done as a fast-forward,
/// which happens when the local branch has diverged from the remote.
pub fn pull_ff_only(remote: &str, branch: &str, verbose: bool) -> Result<()> {
    git_run(&["pull", remote, branch, "--ff-only", "--quiet"], verbose)
}

/// Delete a local branch (safe delete, requires merge)
pub fn delete_branch(branch: &str, verbose: bool) -> Result<()> {
    git_run(&["branch", "-d", branch], verbose)
}

/// Force delete a local branch
pub fn force_delete_branch(branch: &str, verbose: bool) -> Result<()> {
    git_run(&["branch", "-D", branch], verbose)
}

/// Delete a remote branch
#[allow(dead_code)]
pub fn delete_remote_branch(branch: &str, verbose: bool) -> Result<()> {
    git_run(&["push", "origin", "--delete", branch], verbose)
}

/// Get commits that are in `to` but not in `from`
pub fn log_commits(from: &str, to: &str, verbose: bool) -> Result<Vec<String>> {
    let output = git_output(&["log", &format!("{from}..{to}"), "--oneline"], verbose)?;
    Ok(output.lines().map(String::from).collect())
}

/// Stage all changes (including untracked files)
pub fn add_all(verbose: bool) -> Result<()> {
    git_run(&["add", "-A"], verbose)
}

/// Create a commit with the given message
pub fn commit(message: &str, verbose: bool) -> Result<()> {
    git_run(&["commit", "-m", message], verbose)
}

/// Soft reset to target (keeps changes in working directory as staged)
pub fn reset_soft(target: &str, verbose: bool) -> Result<()> {
    git_run(&["reset", "--soft", target], verbose)
}

/// Discard all uncommitted changes (both staged and unstaged, including untracked files)
pub fn discard_all_changes(verbose: bool) -> Result<()> {
    // Reset staged changes
    git_run(&["reset", "--hard", "HEAD"], verbose)?;
    // Remove untracked files and directories
    git_run(&["clean", "-fd"], verbose)
}

/// Rebase current branch onto a target
pub fn rebase(target: &str, verbose: bool) -> Result<()> {
    git_run(&["rebase", target], verbose)
}

/// Force push with lease (safer than --force)
pub fn force_push_with_lease(branch: &str, verbose: bool) -> Result<()> {
    git_run(&["push", "--force-with-lease", "origin", branch], verbose)
}

/// Add a new worktree at the given path with a new branch from a start point
pub fn worktree_add(path: &str, branch: &str, start_point: &str, verbose: bool) -> Result<()> {
    git_run(
        &["worktree", "add", "-b", branch, path, start_point],
        verbose,
    )
}

/// Remove a worktree (with --force)
pub fn worktree_remove(path: &str, verbose: bool) -> Result<()> {
    git_run(&["worktree", "remove", "--force", path], verbose)
}

/// Prune stale worktree entries
pub fn worktree_prune(verbose: bool) -> Result<()> {
    git_run(&["worktree", "prune"], verbose)
}

/// Execute a git command in a specific directory.
/// Sets the process working directory (not just `git -C`) so it works
/// even if the caller's cwd has been deleted.
pub fn git_run_in_dir(dir: &str, args: &[&str], verbose: bool) -> Result<()> {
    if verbose {
        output::action(&format!("git -C {} {}", dir, args.join(" ")));
    }

    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| GwError::GitCommandFailed(format!("Failed to execute git: {e}")))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(GwError::GitCommandFailed(stderr))
    }
}
