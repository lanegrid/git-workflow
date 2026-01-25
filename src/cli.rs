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
}
