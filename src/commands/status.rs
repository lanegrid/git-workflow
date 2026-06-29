//! `gw status` command - Show current repository state

use crate::error::{GwError, Result};
use crate::git;
use crate::github::{self, PrInfo, PrState};
use crate::output;
use crate::state::{DetectContext, NextAction, RepoType, SyncState, WorkingDirState};

/// Execute the `status` command
pub fn run() -> Result<()> {
    // Ensure we're in a git repo
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let repo_type = RepoType::detect()?;
    let home_branch = repo_type.home_branch();
    let current = git::current_branch()?;
    let working_dir = WorkingDirState::detect();
    let sync_state = SyncState::detect(&current).unwrap_or(SyncState::NoUpstream);
    // For status display, an unverifiable remote is reported as "not pushed".
    let has_remote = git::remote_branch_exists(&current).unwrap_or(false);

    println!();

    // Repository type
    match &repo_type {
        RepoType::MainRepo { .. } => {
            output::info("Repository: main repo");
        }
        RepoType::Worktree { home_branch } => {
            output::info(&format!(
                "Repository: worktree (home: {})",
                output::bold(home_branch)
            ));
        }
    }

    // Current branch
    if current == home_branch {
        output::success(&format!("Branch: {} (home)", output::bold(&current)));
    } else {
        output::info(&format!(
            "Branch: {} (home: {})",
            output::bold(&current),
            home_branch
        ));
    }

    // Working directory state
    match working_dir {
        WorkingDirState::Clean => {
            output::success("Working directory: clean");
        }
        _ => {
            output::warn(&format!("Working directory: {}", working_dir.description()));
        }
    }

    // Sync state
    match &sync_state {
        SyncState::NoUpstream => {
            output::info("Upstream: no tracking branch");
        }
        SyncState::Synced => {
            output::success("Upstream: synced");
        }
        SyncState::HasUnpushedCommits { count } => {
            output::warn(&format!("Upstream: {} unpushed commit(s)", count));
        }
        SyncState::Behind { count } => {
            output::warn(&format!("Upstream: {} commit(s) behind", count));
        }
        SyncState::Diverged { ahead, behind } => {
            output::warn(&format!(
                "Upstream: diverged ({} ahead, {} behind)",
                ahead, behind
            ));
        }
    }

    // PR info (only for non-home branches). The default branch (main/master) is
    // the trunk a stacked PR ultimately targets — distinct from this worktree's
    // home branch.
    let default_branch = git::default_branch_name()?;
    let (pr_info, base_pr_merged) = if current != home_branch {
        get_and_show_pr_info(&current, &default_branch)
    } else {
        (None, None)
    };

    // Remote branch status
    if current != home_branch {
        if has_remote {
            output::info(&format!("Remote: origin/{} exists", current));
        } else {
            output::info("Remote: not pushed");
        }
    }

    // Locally recorded stacked base (`gw new --stack`). Once a PR exists,
    // GitHub's base is authoritative and shown above, so this only fills the
    // pre-PR gap. Filtered to a real parent (not the default branch / self).
    let recorded_base = if current != home_branch {
        git::branch_base(&current).filter(|b| b != &default_branch && b != &current)
    } else {
        None
    };
    // If the recorded parent already merged before this branch got a PR, the
    // stacked base is stale — the branch should rebase onto main rather than
    // open a `-B <parent>` PR. (Only checked pre-PR; once a PR exists, GitHub's
    // base is authoritative.)
    let mut recorded_base_merged = false;
    if pr_info.is_none() {
        if let Some(base) = &recorded_base {
            output::info(&format!("Base: {} (stacked, PR not created yet)", base));
            recorded_base_merged = check_base_pr_merged(base).is_some();
        }
    }
    // The recorded base tip is the `rebase --onto` boundary that survives the
    // base branch being deleted (used when the base merged before this PR).
    let recorded_base_sha = recorded_base
        .as_ref()
        .and_then(|_| git::branch_base_sha(&current));

    // Stash count
    let stash_count = git::stash_count();
    if stash_count > 0 {
        output::info(&format!("Stashes: {}", stash_count));
    }

    // Next action
    let next_action = NextAction::detect(&DetectContext {
        current_branch: &current,
        home_branch,
        working_dir: &working_dir,
        sync_state: &sync_state,
        pr_info: pr_info.as_ref(),
        has_remote,
        base_pr_merged: base_pr_merged.as_deref(),
        recorded_base: recorded_base.as_deref(),
        recorded_base_merged,
        recorded_base_sha: recorded_base_sha.as_deref(),
    });
    next_action.display(&current);

    Ok(())
}

/// Get and show PR information for a branch
///
/// Returns:
/// - (Some(PrInfo), Some(base_branch)) if PR exists and base PR was merged
/// - (Some(PrInfo), None) if PR exists but base is the default branch or base PR not merged
/// - (None, None) if no PR found
fn get_and_show_pr_info(branch: &str, default_branch: &str) -> (Option<PrInfo>, Option<String>) {
    if !github::is_gh_available() {
        return (None, None);
    }

    match github::get_pr_for_branch(branch) {
        Ok(Some(pr)) => {
            let state_str = match &pr.state {
                PrState::Open => "OPEN",
                PrState::Merged { .. } => "MERGED",
                PrState::Closed => "CLOSED",
            };

            let method_str = match &pr.state {
                PrState::Merged { method, .. } => format!(" ({})", method),
                _ => String::new(),
            };

            match &pr.state {
                PrState::Open => {
                    output::info(&format!("PR: #{} {} [{}]", pr.number, pr.title, state_str));
                }
                PrState::Merged { .. } => {
                    output::success(&format!(
                        "PR: #{} {} [{}{}]",
                        pr.number, pr.title, state_str, method_str
                    ));
                }
                PrState::Closed => {
                    output::warn(&format!("PR: #{} {} [{}]", pr.number, pr.title, state_str));
                }
            }

            // Show base branch info (a base other than the default branch means
            // this is a stacked PR).
            if pr.base_branch != default_branch {
                output::info(&format!(
                    "Base: {} (not {})",
                    pr.base_branch, default_branch
                ));
            }

            // Check if base PR is merged (only for a stacked, still-open PR)
            let base_pr_merged = if pr.base_branch != default_branch && pr.state.is_open() {
                check_base_pr_merged(&pr.base_branch)
            } else {
                None
            };

            (Some(pr), base_pr_merged)
        }
        Ok(None) => {
            output::info("PR: none");
            (None, None)
        }
        Err(e) => {
            output::warn(&format!("Could not fetch PR info: {}", e));
            (None, None)
        }
    }
}

/// Check if the base branch's PR has been merged
fn check_base_pr_merged(base_branch: &str) -> Option<String> {
    match github::get_pr_for_branch(base_branch) {
        Ok(Some(base_pr)) => {
            if base_pr.state.is_merged() {
                output::success(&format!("Base PR: #{} [MERGED] ✓", base_pr.number));
                Some(base_branch.to_string())
            } else {
                let state_str = if base_pr.state.is_open() {
                    "OPEN"
                } else {
                    "CLOSED"
                };
                output::info(&format!("Base PR: #{} [{}]", base_pr.number, state_str));
                None
            }
        }
        Ok(None) => {
            output::info(&format!("Base PR: none (for {})", base_branch));
            None
        }
        Err(_) => None,
    }
}
