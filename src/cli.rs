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
