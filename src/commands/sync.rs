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

use super::helpers;
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

    // On home branch - just sync with origin/main
    if current == home_branch {
        println!();
        output::info(&format!("Branch: {}", output::bold(&current)));

        // Fetch latest
        output::info("Fetching from origin...");
        git::fetch_prune(verbose)?;
        output::success("Fetched (stale remote branches pruned)");

        // Detect default remote branch and sync
        let default_remote = git::get_default_remote_branch()?;
        let default_branch = default_remote.strip_prefix("origin/").unwrap_or("main");
        helpers::pull_with_output(&default_remote, default_branch, verbose)?;

        output::ready("Ready", home_branch);
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

    // Detect default remote branch
    let default_remote = git::get_default_remote_branch()?;
    let default_branch = default_remote.strip_prefix("origin/").unwrap_or("main");

    // 6. Check if base is already the default branch
    if pr.base_branch == default_branch {
        output::success(&format!(
            "Base is already '{}'. Nothing to sync.",
            default_branch
        ));
        output::hints(&[&format!(
            "git rebase {}  # If you need to update",
            default_remote
        )]);
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

    // 8. Replay only THIS branch's commits onto the default branch.
    //
    // A plain `git rebase origin/main` would re-apply the base PR's commits
    // too — doubled and conflict-prone once the base was squash-merged.
    // `--onto origin/main <old-base>` replays only <old-base>..HEAD. We use the
    // remote-tracking ref of the old base, which still points at the pre-merge
    // tip (cleanup keeps the base branch alive while this PR targets it).
    //
    // Order matters: rebase first, then move the PR base, then push. Moving the
    // base first would leave GitHub showing the new base while the branch still
    // carried the old commits if the rebase then failed.
    let old_base = format!("origin/{}", pr.base_branch);
    output::info(&format!(
        "  Rebasing commits after {} onto {}...",
        old_base, default_remote
    ));
    if let Err(e) = git::rebase_onto(&default_remote, &old_base, verbose) {
        output::error("Rebase failed. You may need to resolve conflicts manually.");
        output::action("git rebase --continue  # After resolving conflicts");
        output::action("git rebase --abort     # To cancel");
        return Err(e);
    }

    // 9. Update PR base to the default branch.
    output::info(&format!("  Updating PR base to {}...", default_branch));
    github::update_pr_base(pr.number, default_branch)?;

    // 10. Force push
    output::info("  Force pushing...");
    git::force_push_with_lease(&current, verbose)?;

    // The branch now targets the default branch, so it is no longer stacked --
    // drop any locally recorded base so `gw status` stops treating it as such.
    git::unset_branch_base(&current, verbose)?;

    println!();
    output::ready("Synced", &current);
    output::hints(&[
        &format!("PR #{} base is now '{}'", pr.number, default_branch),
        "gw status  # Check status",
    ]);

    Ok(())
}
