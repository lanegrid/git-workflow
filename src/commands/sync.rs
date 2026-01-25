//! `gw sync` command - Sync current branch after base PR merge
//!
//! When the base PR of your current branch has been merged, this command:
//! 1. Updates the PR's base branch to main via `gh pr edit --base main`
//! 2. Rebases the branch on origin/main
//! 3. Force pushes with --force-with-lease
//!
//! # Example
//!
//! ```text
//! $ gw status
//!   Branch: feature/child
//!   PR: #42 (open)
//!   Base: feature/base (merged ✓)
//!
//!   Next: gw sync
//!
//! $ gw sync
//!   Updating base: feature/base → main
//!   Rebasing on origin/main...
//!   Force pushing...
//!   ✓ Synced
//! ```

use crate::error::{GwError, Result};
use crate::git;
use crate::github::{self, PrState};
use crate::output;
use crate::state::{RepoType, WorkingDirState};

/// Execute the `sync` command
pub fn run(verbose: bool) -> Result<()> {
    // 1. Check prerequisites
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let working_dir = WorkingDirState::detect();
    if !working_dir.is_clean() {
        output::error(&format!(
            "You have uncommitted changes ({}).",
            working_dir.description()
        ));
        output::action("git stash -u -m 'WIP before sync'");
        return Err(GwError::UncommittedChanges);
    }

    // 2. Get current branch info
    let repo_type = RepoType::detect()?;
    let home_branch = repo_type.home_branch();
    let current = git::current_branch()?;

    // Don't run on home branch
    if current == home_branch {
        output::warn("Already on home branch. Nothing to sync.");
        output::hints(&["mise run git:new feature/...  # Create a feature branch first"]);
        return Ok(());
    }

    println!();
    output::info(&format!("Branch: {}", output::bold(&current)));

    // 3. Fetch latest first to get accurate PR/branch state
    output::info("Fetching from origin...");
    git::fetch_prune(verbose)?;

    // 4. Check GitHub CLI
    if !github::is_gh_available() {
        return Err(GwError::Other(
            "GitHub CLI (gh) is not available. Install it from https://cli.github.com/".into(),
        ));
    }

    // 5. Get PR info for current branch
    let pr = match github::get_pr_for_branch(&current)? {
        Some(pr) => pr,
        None => {
            output::warn("No PR found for this branch.");
            output::hints(&["gh pr create  # Create a PR first"]);
            return Ok(());
        }
    };

    output::info(&format!("PR: #{} ({})", pr.number, pr.title));
    output::info(&format!("Base: {}", pr.base_branch));

    // 6. Check if base is already main
    if pr.base_branch == "main" {
        output::success("Base is already 'main'. Nothing to sync.");
        output::hints(&["git rebase origin/main  # If you need to update"]);
        return Ok(());
    }

    // 7. Check if base branch's PR is merged
    let base_pr = match github::get_pr_for_branch(&pr.base_branch)? {
        Some(base_pr) => base_pr,
        None => {
            output::warn(&format!(
                "No PR found for base branch '{}'. Cannot determine if it's merged.",
                pr.base_branch
            ));
            return Ok(());
        }
    };

    if !base_pr.state.is_merged() {
        let state_str = match &base_pr.state {
            PrState::Open => "still open",
            PrState::Closed => "closed (not merged)",
            PrState::Merged { .. } => "merged",
        };
        output::warn(&format!(
            "Base PR #{} ({}) is {}.",
            base_pr.number, pr.base_branch, state_str
        ));
        output::hints(&["Wait for the base PR to be merged first"]);
        return Ok(());
    }

    // Base PR is merged - proceed with sync
    output::success(&format!(
        "Base PR #{} ({}) is merged ✓",
        base_pr.number, pr.base_branch
    ));

    println!();
    output::info("Syncing...");

    // 8. Update PR base to main
    output::info("  Updating PR base to main...");
    github::update_pr_base(pr.number, "main")?;

    // 9. Rebase on origin/main
    output::info("  Rebasing on origin/main...");
    if let Err(e) = git::rebase("origin/main", verbose) {
        output::error("Rebase failed. You may need to resolve conflicts manually.");
        output::action("git rebase --continue  # After resolving conflicts");
        output::action("git rebase --abort     # To cancel");
        return Err(e);
    }

    // 10. Force push
    output::info("  Force pushing...");
    git::force_push_with_lease(&current, verbose)?;

    println!();
    output::ready("Synced", &current);
    output::hints(&[
        &format!("PR #{} base is now 'main'", pr.number),
        "mise run git:status  # Check status",
    ]);

    Ok(())
}
