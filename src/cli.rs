//! CLI argument parsing with clap

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gw")]
#[command(about = "Git workflow CLI - type-safe worktree-aware git operations")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Show verbose output (git commands being run)
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Switch to home branch and sync with origin/main
    Home,

    /// Create new branch from origin/main
    New {
        /// Name of the branch to create (e.g., feature/add-login)
        branch: Option<String>,
    },

    /// Delete merged branch and return to home
    Cleanup {
        /// Branch to delete (defaults to current branch if not on home)
        branch: Option<String>,
    },

    /// Show current repository state
    Status,

    /// Pause current work: WIP commit and return to home branch
    Pause {
        /// Optional message describing why work is paused
        message: Option<String>,
    },

    /// Abandon current changes and return to home branch
    Abandon,

    /// Undo the last commit (soft reset HEAD~1)
    Undo,

    /// Sync current branch after base PR is merged (update base to main, rebase, force push)
    Sync,

    /// Open the PR for the current branch in the browser
    Open,

    /// Watch a specific PR until merged or closed, then clean up its branch
    Await {
        /// PR number to watch (required so the watcher stays bound to one PR
        /// even if you switch branches, e.g. while working a stacked PR)
        pr: u64,

        /// Also open the PR in the browser before watching
        #[arg(long)]
        open: bool,

        /// Skip waiting for CI checks
        #[arg(long = "no-wait")]
        no_wait: bool,

        /// Do not clean up the branch after the PR is merged
        #[arg(long = "no-cleanup")]
        no_cleanup: bool,

        /// Continue watching for merge even if CI checks fail
        /// (default: stop and report the failure)
        #[arg(long = "ignore-ci-failure")]
        ignore_ci_failure: bool,

        /// Seconds between merge-status polls
        #[arg(long, default_value_t = 30)]
        interval: u64,
    },

    /// Manage worktrees
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommands,
    },
}

#[derive(Subcommand)]
pub enum WorktreeCommands {
    /// Manage a pre-warmed worktree pool
    Pool {
        #[command(subcommand)]
        command: PoolCommands,
    },
}

#[derive(Subcommand)]
pub enum PoolCommands {
    /// Pre-warm the pool with ready-to-use worktrees
    Warm {
        /// Target number of available worktrees in the pool
        count: usize,
    },

    /// Acquire a worktree from the pool (prints path to stdout)
    Acquire,

    /// Show pool status
    Status,

    /// Release acquired worktree(s) back to the pool
    Release {
        /// Name of the worktree to release (defaults to all acquired)
        name: Option<String>,
    },

    /// Remove all worktrees and clean up the pool
    Drain {
        /// Force drain even if worktrees are acquired
        #[arg(long)]
        force: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn await_requires_pr_number() {
        // Without a PR number, parsing must fail.
        assert!(Cli::try_parse_from(["gw", "await"]).is_err());
    }

    #[test]
    fn await_defaults() {
        let cli = Cli::try_parse_from(["gw", "await", "42"]).unwrap();
        match cli.command {
            Commands::Await {
                pr,
                open,
                no_wait,
                no_cleanup,
                ignore_ci_failure,
                interval,
            } => {
                assert_eq!(pr, 42);
                assert!(!open);
                assert!(!no_wait);
                assert!(!no_cleanup);
                assert!(!ignore_ci_failure);
                assert_eq!(interval, 30);
            }
            _ => panic!("expected Await command"),
        }
    }

    #[test]
    fn await_flags() {
        let cli = Cli::try_parse_from([
            "gw",
            "await",
            "42",
            "--open",
            "--no-wait",
            "--no-cleanup",
            "--ignore-ci-failure",
            "--interval",
            "5",
        ])
        .unwrap();
        match cli.command {
            Commands::Await {
                pr,
                open,
                no_wait,
                no_cleanup,
                ignore_ci_failure,
                interval,
            } => {
                assert_eq!(pr, 42);
                assert!(open);
                assert!(no_wait);
                assert!(no_cleanup);
                assert!(ignore_ci_failure);
                assert_eq!(interval, 5);
            }
            _ => panic!("expected Await command"),
        }
    }
}
