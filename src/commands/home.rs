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

    let repo_type = RepoType::detect()?;
    let home_branch = repo_type.home_branch();
    let current = git::current_branch()?;

    println!();
    output::info(&format!("Home branch: {}", output::bold(home_branch)));

    // Fetch latest
    output::info("Fetching from origin...");
    git::fetch_prune(verbose)?;
    output::success("Fetched (stale remote branches pruned)");

    // Switch to home branch if needed
    if current != home_branch {
        if !git::branch_exists(home_branch) {
            output::info("Creating home branch from origin/main...");
            git::checkout_new_branch(home_branch, "origin/main", verbose)?;
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

    // Sync with origin/main
    output::info("Syncing with origin/main...");
    let before = git::head_commit()?;
    git::pull("origin", "main", verbose)?;
    let after = git::head_commit()?;

    if before != after {
        let count = git::commit_count(&before, &after)?;
        output::success(&format!(
            "Pulled {} commit(s) from origin/main",
            output::bold(&count.to_string())
        ));
    } else {
        output::success("Already up to date with origin/main");
    }

    output::ready("Ready", home_branch);
    output::hints(&["mise run git:new feature/your-feature  # Create new branch"]);

    Ok(())
}
