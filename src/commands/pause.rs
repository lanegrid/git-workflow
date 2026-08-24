//! `gw pause` command - Pause current work by creating a WIP commit and returning to home

use super::helpers;
use crate::error::{GwError, Result};
use crate::git;
use crate::github;
use crate::output;
use crate::state::{RepoType, WorkingDirState};

/// Execute the `pause` command
pub fn run(message: Option<String>, verbose: bool) -> Result<()> {
    // Ensure we're in a git repo
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let repo_type = RepoType::detect()?;
    let home_branch = repo_type.home_branch();
    let current = git::current_branch()?;
    let working_dir = WorkingDirState::detect();

    println!();

    // If on home branch, just inform
    if current == home_branch {
        output::info(&format!("Already on home branch '{}'", home_branch));
        if !working_dir.is_clean() {
            output::warn(
                "You have uncommitted changes. Consider committing or using 'gw new <branch>'.",
            );
        }
        return Ok(());
    }

    // Create WIP commit if there are changes
    if !working_dir.is_clean() {
        let commit_message = match &message {
            Some(msg) => format!("WIP: {}", msg),
            None => "WIP: paused".to_string(),
        };

        // Everything goes into the WIP commit — untracked files included — so
        // nothing is left behind in the worktree when you switch away.
        output::info("Creating WIP commit (all changes, including untracked files)...");
        git::add_all(verbose)?;
        git::commit(&commit_message, verbose)?;
        output::success(&format!("Created: {}", commit_message));

        // Try to add PR comment if message provided
        if let Some(msg) = &message {
            if github::is_gh_available() {
                if let Ok(Some(pr)) = github::get_pr_for_branch(&current) {
                    let comment = format!("⏸️ Paused work on this PR: {}", msg);
                    match github::add_pr_comment(pr.number, &comment) {
                        Ok(()) => output::success(&format!("Added comment to PR #{}", pr.number)),
                        Err(e) => output::warn(&format!("Could not add PR comment: {}", e)),
                    }
                }
            }
        }
    } else {
        output::info("Working directory is clean. No WIP commit needed.");
    }

    // Switch to home branch
    output::info("Switching to home branch...");
    git::fetch_prune(verbose)?;

    // Detect default remote branch
    let default_remote = git::get_default_remote_branch()?;
    let default_branch = default_remote.strip_prefix("origin/").unwrap_or("main");

    if !git::branch_exists(home_branch) {
        git::checkout_new_branch(home_branch, &default_remote, verbose)?;
    } else {
        // Ref first, checkout second: one working-tree transition, no
        // stale-tree flash for anything watching the files.
        helpers::fast_forward_home_ref(home_branch, &default_remote, verbose);
        git::checkout(home_branch, verbose)?;
    }

    // Sync with default remote (fast-forward only; stop on divergence)
    helpers::pull_with_output(&default_remote, default_branch, verbose)?;
    output::success(&format!("Switched to {}", output::bold(home_branch)));

    output::ready("Paused", home_branch);
    output::hints(&[
        &format!("git checkout {}  # Return to paused branch", current),
        "gw undo              # Undo WIP commit after checkout",
        "gw new <branch>      # Start different work",
    ]);

    Ok(())
}
