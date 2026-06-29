//! Next action detection for workflow automation
//!
//! Determines the recommended next action based on current repository state.

use crate::github::PrInfo;
use crate::output;

use super::{SyncState, WorkingDirState};

/// Recommended next action based on current state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextAction {
    /// On home branch, ready to start new work
    StartNewWork,
    /// On home branch but behind upstream, should sync first
    SyncHomeWithUpstream { behind_count: usize },
    /// Has uncommitted changes, should commit
    CommitChanges,
    /// Has unpushed commits, should push
    PushChanges,
    /// Pushed but no PR, should create PR. `base` is the stacked parent branch
    /// to pass as `-B`, or `None` when the PR targets the default branch.
    CreatePr { base: Option<String> },
    /// PR is open, waiting for review/CI
    WaitingForReview { pr_number: u64 },
    /// PR is merged, should cleanup
    Cleanup,
    /// Branch is behind origin/main, should rebase
    RebaseNeeded,
    /// Branch has diverged from upstream, needs resolution
    ResolveDivergence,
    /// PR was closed without merging
    PrClosed { pr_number: u64 },
    /// Base PR was merged, should sync (update base to main, rebase, push)
    SyncNeeded { base_branch: String },
}

impl NextAction {
    /// Detect the next action based on current state
    ///
    /// # Arguments
    /// * `base_pr_merged` - If Some(branch_name), the base PR for that branch was merged
    /// * `recorded_base` - If Some(branch_name), the locally recorded stacked base
    ///   (`gw new --stack`), already filtered to a real parent (not the default
    ///   branch). Used to suggest `-B <base>` when creating the PR.
    // The detected state genuinely depends on this many independent inputs;
    // bundling them into a struct would only move the noise to the call site.
    #[allow(clippy::too_many_arguments)]
    pub fn detect(
        current_branch: &str,
        home_branch: &str,
        working_dir: &WorkingDirState,
        sync_state: &SyncState,
        pr_info: Option<&PrInfo>,
        has_remote: bool,
        base_pr_merged: Option<&str>,
        recorded_base: Option<&str>,
    ) -> Self {
        // On home branch
        if current_branch == home_branch {
            // Behind upstream → sync first
            if let SyncState::Behind { count } = sync_state {
                return NextAction::SyncHomeWithUpstream {
                    behind_count: *count,
                };
            }
            return NextAction::StartNewWork;
        }

        // PR is merged → cleanup
        if let Some(pr) = pr_info {
            if pr.state.is_merged() {
                return NextAction::Cleanup;
            }
            if pr.state.is_closed() {
                return NextAction::PrClosed {
                    pr_number: pr.number,
                };
            }
        }

        // Base PR was merged → sync needed (takes priority over uncommitted changes)
        if let Some(base_branch) = base_pr_merged {
            return NextAction::SyncNeeded {
                base_branch: base_branch.to_string(),
            };
        }

        // Has uncommitted changes → commit
        if !matches!(working_dir, WorkingDirState::Clean) {
            return NextAction::CommitChanges;
        }

        // Diverged from upstream → resolve
        if matches!(sync_state, SyncState::Diverged { .. }) {
            return NextAction::ResolveDivergence;
        }

        // Behind upstream → rebase
        if matches!(sync_state, SyncState::Behind { .. }) {
            return NextAction::RebaseNeeded;
        }

        // Has unpushed commits or no upstream → push
        if matches!(
            sync_state,
            SyncState::HasUnpushedCommits { .. } | SyncState::NoUpstream
        ) {
            return NextAction::PushChanges;
        }

        // Pushed but no PR → create PR (carry the stacked base if recorded)
        if pr_info.is_none() && has_remote {
            return NextAction::CreatePr {
                base: recorded_base.map(String::from),
            };
        }

        // PR is open → waiting
        if let Some(pr) = pr_info {
            if pr.state.is_open() {
                return NextAction::WaitingForReview {
                    pr_number: pr.number,
                };
            }
        }

        // Default: waiting for something
        NextAction::WaitingForReview { pr_number: 0 }
    }

    /// Display the next action with commands
    pub fn display(&self, branch: &str) {
        println!();
        output::separator();

        match self {
            NextAction::StartNewWork => {
                output::action("Next: start new work");
                println!();
                println!("  gw new feature/your-feature");
            }
            NextAction::SyncHomeWithUpstream { behind_count } => {
                output::action(&format!(
                    "Next: sync with upstream ({} commit(s) behind)",
                    behind_count
                ));
                println!();
                println!("  gw home");
            }
            NextAction::CommitChanges => {
                output::action("Next: commit changes");
                println!();
                println!(
                    "  git add <files> && git commit -m \"feat: ...\"  # stage deliberately, not -A"
                );
            }
            NextAction::PushChanges => {
                output::action("Next: push to remote");
                println!();
                println!("  git push -u origin {}", branch);
            }
            NextAction::CreatePr { base } => {
                output::action("Next: create pull request");
                println!();
                match base {
                    Some(base) => {
                        println!(
                            "  gh pr create -a \"@me\" -B {} -t \"...\"  # stacked on {}",
                            base, base
                        )
                    }
                    None => println!("  gh pr create -a \"@me\" -t \"...\""),
                }
            }
            NextAction::WaitingForReview { pr_number } => {
                if *pr_number > 0 {
                    output::action(&format!("Waiting: PR #{} in review", pr_number));
                    println!();
                    println!(
                        "  gw await {} --open  # Wait for merge, then cleanup",
                        pr_number
                    );
                    println!("  gw open             # Open PR in browser");
                } else {
                    output::action("Waiting: PR in review");
                    println!();
                    println!("  gw open  # Open PR in browser");
                }
            }
            NextAction::Cleanup => {
                output::action("Next: cleanup merged branch");
                println!();
                println!("  gw cleanup");
            }
            NextAction::RebaseNeeded => {
                output::action("Next: rebase on latest main");
                println!();
                println!("  git fetch --prune && git rebase origin/main");
            }
            NextAction::ResolveDivergence => {
                output::action("Next: resolve divergence");
                println!();
                println!("  # Option 1: Rebase (preferred)");
                println!("  git fetch --prune && git rebase origin/main");
                println!();
                println!("  # Option 2: Force push (if you know what you're doing)");
                println!("  git push --force-with-lease");
            }
            NextAction::PrClosed { pr_number } => {
                output::action(&format!("PR #{} was closed without merging", pr_number));
                println!();
                println!("  # Option 1: Reopen the PR");
                println!("  gh pr reopen {}", pr_number);
                println!();
                println!("  # Option 2: Cleanup and start fresh");
                println!("  gw cleanup");
            }
            NextAction::SyncNeeded { base_branch } => {
                output::action(&format!("Next: sync (base '{}' was merged)", base_branch));
                println!();
                println!("  gw sync");
            }
        }

        output::separator();
    }

    /// Get a short description for the action
    pub fn short_description(&self) -> &'static str {
        match self {
            NextAction::StartNewWork => "start new work",
            NextAction::SyncHomeWithUpstream { .. } => "sync with upstream",
            NextAction::CommitChanges => "commit changes",
            NextAction::PushChanges => "push to remote",
            NextAction::CreatePr { .. } => "create PR",
            NextAction::WaitingForReview { .. } => "waiting for review",
            NextAction::Cleanup => "cleanup branch",
            NextAction::RebaseNeeded => "rebase needed",
            NextAction::ResolveDivergence => "resolve divergence",
            NextAction::PrClosed { .. } => "PR closed",
            NextAction::SyncNeeded { .. } => "sync needed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{PrInfo, PrState};

    #[test]
    fn test_on_home_branch_suggests_start_new_work() {
        let action = NextAction::detect(
            "main",
            "main",
            &WorkingDirState::Clean,
            &SyncState::Synced,
            None,
            false,
            None,
            None,
        );
        assert_eq!(action, NextAction::StartNewWork);
    }

    #[test]
    fn test_on_home_branch_behind_suggests_sync() {
        let action = NextAction::detect(
            "main",
            "main",
            &WorkingDirState::Clean,
            &SyncState::Behind { count: 5 },
            None,
            false,
            None,
            None,
        );
        assert_eq!(action, NextAction::SyncHomeWithUpstream { behind_count: 5 });
    }

    #[test]
    fn test_uncommitted_changes_suggests_commit() {
        let action = NextAction::detect(
            "feature/test",
            "main",
            &WorkingDirState::HasUnstagedChanges,
            &SyncState::Synced,
            None,
            true,
            None,
            None,
        );
        assert_eq!(action, NextAction::CommitChanges);
    }

    #[test]
    fn test_unpushed_commits_suggests_push() {
        let action = NextAction::detect(
            "feature/test",
            "main",
            &WorkingDirState::Clean,
            &SyncState::HasUnpushedCommits { count: 2 },
            None,
            true,
            None,
            None,
        );
        assert_eq!(action, NextAction::PushChanges);
    }

    #[test]
    fn test_no_upstream_suggests_push() {
        let action = NextAction::detect(
            "feature/test",
            "main",
            &WorkingDirState::Clean,
            &SyncState::NoUpstream,
            None,
            false,
            None,
            None,
        );
        assert_eq!(action, NextAction::PushChanges);
    }

    #[test]
    fn test_pushed_no_pr_suggests_create_pr() {
        let action = NextAction::detect(
            "feature/test",
            "main",
            &WorkingDirState::Clean,
            &SyncState::Synced,
            None,
            true,
            None,
            None,
        );
        assert_eq!(action, NextAction::CreatePr { base: None });
    }

    #[test]
    fn test_pushed_no_pr_with_recorded_base_suggests_stacked_pr() {
        let action = NextAction::detect(
            "feature/child",
            "main",
            &WorkingDirState::Clean,
            &SyncState::Synced,
            None,
            true,
            None,
            Some("feature/parent"),
        );
        assert_eq!(
            action,
            NextAction::CreatePr {
                base: Some("feature/parent".to_string())
            }
        );
    }

    #[test]
    fn test_open_pr_suggests_waiting() {
        let pr = PrInfo::new(42, "Test PR", "https://...", PrState::Open, "main");
        let action = NextAction::detect(
            "feature/test",
            "main",
            &WorkingDirState::Clean,
            &SyncState::Synced,
            Some(&pr),
            true,
            None,
            None,
        );
        assert_eq!(action, NextAction::WaitingForReview { pr_number: 42 });
    }

    #[test]
    fn test_merged_pr_suggests_cleanup() {
        let pr = PrInfo::new(
            42,
            "Test PR",
            "https://...",
            PrState::Merged {
                method: crate::github::MergeMethod::Squash,
                merge_commit: None,
            },
            "main",
        );
        let action = NextAction::detect(
            "feature/test",
            "main",
            &WorkingDirState::Clean,
            &SyncState::Synced,
            Some(&pr),
            true,
            None,
            None,
        );
        assert_eq!(action, NextAction::Cleanup);
    }

    #[test]
    fn test_closed_pr_suggests_reopen_or_cleanup() {
        let pr = PrInfo::new(42, "Test PR", "https://...", PrState::Closed, "main");
        let action = NextAction::detect(
            "feature/test",
            "main",
            &WorkingDirState::Clean,
            &SyncState::Synced,
            Some(&pr),
            true,
            None,
            None,
        );
        assert_eq!(action, NextAction::PrClosed { pr_number: 42 });
    }

    #[test]
    fn test_behind_suggests_rebase() {
        let action = NextAction::detect(
            "feature/test",
            "main",
            &WorkingDirState::Clean,
            &SyncState::Behind { count: 3 },
            None,
            true,
            None,
            None,
        );
        assert_eq!(action, NextAction::RebaseNeeded);
    }

    #[test]
    fn test_diverged_suggests_resolve() {
        let action = NextAction::detect(
            "feature/test",
            "main",
            &WorkingDirState::Clean,
            &SyncState::Diverged {
                ahead: 2,
                behind: 3,
            },
            None,
            true,
            None,
            None,
        );
        assert_eq!(action, NextAction::ResolveDivergence);
    }

    #[test]
    fn test_uncommitted_changes_takes_priority_over_pr_open() {
        let pr = PrInfo::new(42, "Test PR", "https://...", PrState::Open, "main");
        let action = NextAction::detect(
            "feature/test",
            "main",
            &WorkingDirState::HasStagedChanges,
            &SyncState::Synced,
            Some(&pr),
            true,
            None,
            None,
        );
        assert_eq!(action, NextAction::CommitChanges);
    }

    #[test]
    fn test_merged_pr_takes_priority_over_uncommitted_changes() {
        let pr = PrInfo::new(
            42,
            "Test PR",
            "https://...",
            PrState::Merged {
                method: crate::github::MergeMethod::Squash,
                merge_commit: None,
            },
            "main",
        );
        let action = NextAction::detect(
            "feature/test",
            "main",
            &WorkingDirState::HasUnstagedChanges,
            &SyncState::Synced,
            Some(&pr),
            true,
            None,
            None,
        );
        // Merged PR takes priority - cleanup first
        assert_eq!(action, NextAction::Cleanup);
    }

    #[test]
    fn test_base_pr_merged_suggests_sync() {
        let pr = PrInfo::new(42, "Test PR", "https://...", PrState::Open, "feature/base");
        let action = NextAction::detect(
            "feature/child",
            "main",
            &WorkingDirState::Clean,
            &SyncState::Synced,
            Some(&pr),
            true,
            Some("feature/base"),
            None,
        );
        assert_eq!(
            action,
            NextAction::SyncNeeded {
                base_branch: "feature/base".to_string()
            }
        );
    }

    #[test]
    fn test_base_pr_merged_takes_priority_over_waiting() {
        let pr = PrInfo::new(42, "Test PR", "https://...", PrState::Open, "feature/base");
        let action = NextAction::detect(
            "feature/child",
            "main",
            &WorkingDirState::Clean,
            &SyncState::Synced,
            Some(&pr),
            true,
            Some("feature/base"),
            None,
        );
        // SyncNeeded should take priority over WaitingForReview
        assert_eq!(
            action,
            NextAction::SyncNeeded {
                base_branch: "feature/base".to_string()
            }
        );
    }
}
