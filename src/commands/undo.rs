//! `gw undo` command - Undo the last commit (soft reset HEAD~1)

use crate::error::{GwError, Result};
use crate::git;
use crate::output;
use crate::state::SyncState;

/// Execute the `undo` command
pub fn run(verbose: bool) -> Result<()> {
    // Ensure we're in a git repo
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    // Check if we can undo
    if !git::has_commits_to_undo() {
        return Err(GwError::NothingToUndo);
    }

    let current = git::current_branch()?;

    // Show what we're undoing
    let commit_short = git::short_commit()?;
    let commit_msg = git::head_commit_message()?;

    println!();
    output::info(&format!(
        "Undoing commit: {} {}",
        output::bold(&commit_short),
        commit_msg
    ));

    // Check if commit was pushed (warn but continue)
    let sync_state = SyncState::detect(&current).unwrap_or(SyncState::NoUpstream);
    if matches!(sync_state, SyncState::Synced | SyncState::Behind { .. }) {
        output::warn("This commit may have been pushed. Local undo won't affect remote.");
        output::warn("You may need to force push after undoing.");
    }

    // Soft reset
    git::reset_soft("HEAD~1", verbose)?;
    output::success("Commit undone (changes are now staged)");

    output::ready("Undone", &current);
    output::hints(&[
        "git status         # See staged changes",
        "git diff --cached  # Review staged changes",
        "git commit -m \"...\"  # Re-commit with new message",
    ]);

    Ok(())
}
