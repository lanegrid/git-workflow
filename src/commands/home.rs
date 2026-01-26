//! `gw home` command - Switch to home branch and sync with origin/main

use crate::error::{GwError, Result};
use crate::git;
use crate::output;
use crate::state::RepoType;

/// Execute the `home` command
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

    println!();
    output::info(&format!("Home branch: {}", output::bold(home_branch)));

    // Fetch latest
    output::info("Fetching from origin...");
    git::fetch_prune(verbose)?;
    output::success("Fetched (stale remote branches pruned)");

    // Detect default remote branch (origin/main or origin/master)
    let default_remote = git::get_default_remote_branch()?;
    let default_branch = default_remote.strip_prefix("origin/").unwrap_or("main");

    // Switch to home branch if needed
    if current != home_branch {
        if !git::branch_exists(home_branch) {
            output::info(&format!("Creating home branch from {}...", default_remote));
            git::checkout_new_branch(home_branch, &default_remote, verbose)?;
            output::success(&format!(
                "Created and switched to {}",
                output::bold(home_branch)
            ));
        } else {
            git::checkout(home_branch, verbose)?;
            output::success(&format!("Switched to {}", output::bold(home_branch)));
        }
    } else {
        output::success(&format!("Already on {}", output::bold(home_branch)));
    }

    // Sync with default remote branch
    output::info(&format!("Syncing with {}...", default_remote));
    let before = git::head_commit()?;
    git::pull("origin", default_branch, verbose)?;
    let after = git::head_commit()?;

    if before != after {
        let count = git::commit_count(&before, &after)?;
        output::success(&format!(
            "Pulled {} commit(s) from {}",
            output::bold(&count.to_string()),
            default_remote
        ));
    } else {
        output::success(&format!("Already up to date with {}", default_remote));
    }

    output::ready("Ready", home_branch);
    output::hints(&["mise run git:new feature/your-feature  # Create new branch"]);

    Ok(())
}
