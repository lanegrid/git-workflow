//! `gw cleanup` command - Delete merged branch and return to home
//!
//! Uses GitHub PR information to make smart decisions about branch deletion.

use crate::error::{GwError, Result};
use crate::git;
use crate::github::{self, PrState};
use crate::output;
use crate::state::{RepoType, SyncState, WorkingDirState, classify_branch};

/// Execute the `cleanup` command
pub fn run(branch_name: Option<String>, verbose: bool) -> Result<()> {
    // Ensure we're in a git repo
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let repo_type = RepoType::detect()?;
    let home_branch = repo_type.home_branch();
    let current = git::current_branch()?;

    // Determine which branch to delete
    let branch_to_delete = match branch_name {
        Some(name) => name,
        None => {
            if current == home_branch {
                // In a pool worktree on home branch: just release back to pool
                if let Some((pool_name, leader_root)) = super::worktree_pool::detect_pool_worktree()
                {
                    println!();
                    output::info("On home branch in pool worktree, releasing...");
                    super::worktree_pool::pool_release_after_cleanup(
                        &pool_name,
                        &leader_root,
                        verbose,
                    )?;
                    output::ready("Pool worktree released", home_branch);
                    return Ok(());
                }
                return Err(GwError::AlreadyOnHomeBranch(home_branch.to_string()));
            }
            current.clone()
        }
    };

    println!();
    output::info(&format!(
        "Branch to delete: {}",
        output::bold(&branch_to_delete)
    ));
    output::info(&format!("Home branch: {}", output::bold(home_branch)));

    // Safety: check if branch is protected (compile-time typestate enforced)
    let branch = classify_branch(&branch_to_delete, &repo_type);
    let deletable_branch = branch.try_deletable()?;

    // Check if branch exists locally
    let branch_exists = git::branch_exists(&branch_to_delete);
    if !branch_exists {
        output::warn(&format!(
            "Branch '{}' does not exist locally",
            branch_to_delete
        ));
    }

    // Safety check: uncommitted changes
    let working_dir = WorkingDirState::detect();
    if !working_dir.is_clean() {
        output::error(&format!(
            "You have uncommitted changes ({}).",
            working_dir.description()
        ));
        println!();
        output::action("git stash -u -m 'WIP before cleanup'");
        output::action("git status");
        return Err(GwError::UncommittedChanges);
    }

    // Query PR information from GitHub
    let pr_info = query_pr_info(&branch_to_delete);
    let force_delete_allowed = should_allow_force_delete(&pr_info, &branch_to_delete);

    // Safety check: unpushed commits (skip if PR is merged)
    if branch_exists && !force_delete_allowed {
        check_unpushed_commits(&branch_to_delete)?;
    }

    // Fetch and prune
    output::info("Fetching from origin...");
    git::fetch_prune(verbose)?;
    output::success("Fetched");

    // Detect default remote branch
    let default_remote = git::get_default_remote_branch()?;
    let default_branch = default_remote.strip_prefix("origin/").unwrap_or("main");

    // Switch to home branch first (if on the branch to delete)
    if current == branch_to_delete {
        if !git::branch_exists(home_branch) {
            git::checkout_new_branch(home_branch, &default_remote, verbose)?;
            output::success(&format!(
                "Created and switched to {}",
                output::bold(home_branch)
            ));
        } else {
            git::checkout(home_branch, verbose)?;
            output::success(&format!("Switched to {}", output::bold(home_branch)));
        }
    }

    // Sync home branch with default remote
    output::info(&format!("Syncing with {}...", default_remote));
    git::pull("origin", default_branch, verbose)?;
    output::success("Synced");

    // Delete the local branch
    if branch_exists {
        delete_local_branch(
            deletable_branch,
            &branch_to_delete,
            force_delete_allowed,
            verbose,
        );
    }

    // Handle remote branch
    handle_remote_branch(&branch_to_delete, &pr_info, verbose);

    // Check for stashes
    let stash_count = git::stash_count();
    if stash_count > 0 {
        output::warn(&format!(
            "You have {} stash(es). Don't forget about them:",
            stash_count
        ));
        output::action("git stash list");
    }

    // Pool worktree auto-release: if we're inside a pool worktree,
    // clean untracked files, remove the acquire marker, and run setup hook
    if let Some((pool_name, leader_root)) = super::worktree_pool::detect_pool_worktree() {
        super::worktree_pool::pool_release_after_cleanup(&pool_name, &leader_root, verbose)?;
    }

    output::ready("Cleanup complete", home_branch);
    output::hints(&["mise run git:new feature/your-feature  # Create new branch"]);

    Ok(())
}

/// Query PR information from GitHub
fn query_pr_info(branch: &str) -> Option<github::PrInfo> {
    if !github::is_gh_available() {
        output::info("GitHub CLI (gh) not available, skipping PR lookup");
        return None;
    }

    output::info("Checking PR status...");

    match github::get_pr_for_branch(branch) {
        Ok(Some(pr)) => {
            display_pr_info(&pr);
            Some(pr)
        }
        Ok(None) => {
            output::info("No PR found for this branch");
            None
        }
        Err(e) => {
            output::warn(&format!("Could not fetch PR info: {}", e));
            None
        }
    }
}

/// Display PR information
fn display_pr_info(pr: &github::PrInfo) {
    let state_display = match &pr.state {
        PrState::Open => "OPEN".to_string(),
        PrState::Merged { method, .. } => format!("MERGED ({})", method),
        PrState::Closed => "CLOSED".to_string(),
    };

    output::success(&format!(
        "PR #{}: {} [{}]",
        pr.number, pr.title, state_display
    ));
}

/// Determine if force delete should be allowed based on PR state
fn should_allow_force_delete(pr_info: &Option<github::PrInfo>, branch: &str) -> bool {
    match pr_info {
        Some(pr) => match &pr.state {
            PrState::Merged { method, .. } => {
                output::info(&format!("PR was {} merged, safe to force delete", method));
                true
            }
            PrState::Open => {
                output::warn("PR is still OPEN, be careful!");
                false
            }
            PrState::Closed => {
                output::warn("PR was closed without merging");
                false
            }
        },
        None => {
            // No PR info - check if remote branch exists
            if git::remote_branch_exists(branch) {
                output::info("No PR found but remote branch exists");
                false
            } else {
                // Branch was never pushed or already deleted from remote
                true
            }
        }
    }
}

/// Check for unpushed commits
fn check_unpushed_commits(branch: &str) -> Result<()> {
    if git::has_remote_tracking(branch) {
        let sync_state = SyncState::detect(branch)?;
        if sync_state.has_unpushed() {
            let count = sync_state.unpushed_count();
            output::error(&format!(
                "Branch '{}' has {} unpushed commit(s)!",
                branch, count
            ));
            println!();

            // Show unpushed commits
            if let Ok(commits) =
                git::log_commits(&format!("{}@{{upstream}}", branch), branch, false)
            {
                println!("Unpushed commits:");
                for commit in commits.iter().take(5) {
                    println!("  {commit}");
                }
                println!();
            }

            output::action(&format!("git push origin {}  # Push first", branch));
            output::action(&format!(
                "git branch -D {}    # Or force delete (lose commits)",
                branch
            ));
            return Err(GwError::UnpushedCommits(branch.to_string(), count));
        }
    } else {
        // No remote tracking
        if git::remote_branch_exists(branch) {
            output::info("Branch has no tracking but remote exists (PR probably merged)");
        } else {
            output::warn(&format!("Branch '{}' was never pushed to remote", branch));
            output::warn("Commits on this branch will be lost if deleted");
        }
    }
    Ok(())
}

/// Delete local branch, using force delete if allowed
fn delete_local_branch(
    deletable_branch: crate::state::Branch<crate::state::Deletable>,
    branch_name: &str,
    force_allowed: bool,
    verbose: bool,
) {
    match deletable_branch.delete(verbose) {
        Ok(()) => {
            output::success(&format!(
                "Deleted local branch {}",
                output::bold(branch_name)
            ));
        }
        Err(_) => {
            if force_allowed {
                // PR was merged, safe to force delete
                output::info(
                    "Branch not fully merged locally, but PR was merged. Force deleting...",
                );
                if let Err(e) = git::force_delete_branch(branch_name, verbose) {
                    output::warn(&format!("Force delete failed: {}", e));
                } else {
                    output::success(&format!(
                        "Force deleted local branch {}",
                        output::bold(branch_name)
                    ));
                }
            } else {
                output::warn("Branch not fully merged. Use -D to force delete:");
                output::action(&format!("git branch -D {}", branch_name));
            }
        }
    }
}

/// Handle remote branch deletion
fn handle_remote_branch(branch: &str, pr_info: &Option<github::PrInfo>, verbose: bool) {
    let remote_exists = git::remote_branch_exists(branch);

    if !remote_exists {
        // Remote branch already deleted (GitHub auto-delete after merge)
        if let Some(pr) = pr_info {
            if matches!(pr.state, PrState::Merged { .. }) {
                output::success("Remote branch already deleted by GitHub");
            }
        }
        return;
    }

    // Remote branch still exists
    match pr_info {
        Some(pr) if matches!(pr.state, PrState::Merged { .. }) => {
            // PR merged but remote branch exists - delete it
            output::info("PR merged, deleting remote branch...");
            match github::delete_remote_branch(branch) {
                Ok(()) => {
                    output::success(&format!(
                        "Deleted remote branch origin/{}",
                        output::bold(branch)
                    ));
                }
                Err(e) => {
                    output::warn(&format!("Failed to delete remote branch: {}", e));
                    output::action(&format!("git push origin --delete {}", branch));
                }
            }
        }
        Some(pr) if matches!(pr.state, PrState::Open) => {
            output::warn(&format!(
                "Remote branch exists and PR #{} is still open",
                pr.number
            ));
            output::action(&format!("gh pr view {}", pr.number));
        }
        _ => {
            output::warn(&format!("Remote branch still exists: origin/{}", branch));
            if verbose {
                output::action(&format!("git push origin --delete {}", branch));
            }
        }
    }
}
