//! `gw abandon` command - Abandon current changes and return to home branch

use super::helpers;
use crate::error::{GwError, Result};
use crate::git;
use crate::output;
use crate::state::{RepoType, SyncState, WorkingDirState};

/// Execute the `abandon` command
pub fn run(verbose: bool) -> Result<()> {
    // Ensure we're in a git repo
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    // Check for detached HEAD
    if git::is_detached_head() {
        return Err(GwError::Other(
            "Cannot run from detached HEAD. Checkout a branch first.".to_string(),
        ));
    }

    let repo_type = RepoType::detect()?;
    let home_branch = repo_type.home_branch();
    let current = git::current_branch()?;
    let working_dir = WorkingDirState::detect();

    println!();
    output::info(&format!("Current branch: {}", output::bold(&current)));
    output::info(&format!("Home branch: {}", output::bold(home_branch)));

    // Check what will be affected
    let has_changes = !working_dir.is_clean();
    let has_untracked = git::has_untracked_files();
    let sync_state = SyncState::detect(&current).unwrap_or(SyncState::NoUpstream);

    // Detect default remote branch
    let default_remote = git::get_default_remote_branch()?;
    let default_branch = default_remote.strip_prefix("origin/").unwrap_or("main");

    // Already on home branch with clean working directory and no untracked files
    if !has_changes && !has_untracked && current == home_branch {
        output::success("Already on home branch with clean working directory");
        // Still sync with default remote (fast-forward only; stop on divergence)
        git::fetch_prune(verbose)?;
        helpers::pull_with_output(&default_remote, default_branch, verbose)?;
        return Ok(());
    }

    // Warn about what will be lost. Untracked files get their own message
    // below, so only mention tracked changes here to avoid double-reporting the
    // untracked-only case.
    if working_dir.has_tracked_changes() {
        output::warn(&format!(
            "Uncommitted changes will be DISCARDED: {}",
            working_dir.description()
        ));
    }
    if has_untracked {
        output::warn("Untracked files will be DELETED");
    }

    // Warn about unpushed commits (they'll remain on the branch, not lost)
    if let SyncState::HasUnpushedCommits { count } = sync_state {
        output::warn(&format!(
            "Branch '{}' has {} unpushed commit(s). They will remain on the branch.",
            current, count
        ));
    }

    // Discard changes if any (including untracked files)
    if has_changes || has_untracked {
        output::info("Discarding changes...");
        git::discard_all_changes(verbose)?;
        output::success("Changes discarded");
    }

    // Switch to home branch if not already there
    if current != home_branch {
        if !git::branch_exists(home_branch) {
            output::info(&format!("Creating home branch from {}...", default_remote));
            git::checkout_new_branch(home_branch, &default_remote, verbose)?;
        } else {
            git::checkout(home_branch, verbose)?;
        }
        output::success(&format!("Switched to {}", output::bold(home_branch)));
    }

    // Sync with default remote (fast-forward only; stop on divergence)
    git::fetch_prune(verbose)?;
    helpers::pull_with_output(&default_remote, default_branch, verbose)?;

    output::ready("Ready", home_branch);
    output::hints(&["gw new feature/your-feature  # Start fresh"]);

    Ok(())
}
