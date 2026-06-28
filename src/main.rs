//! git-workflow - Git workflow CLI

use std::process::ExitCode;

use clap::Parser;

use git_workflow::cli::{Cli, Commands, PoolCommands, WorktreeCommands};
use git_workflow::commands;
use git_workflow::output;

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Home => commands::home::run(cli.verbose),
        Commands::New { branch } => commands::new::run(branch, cli.verbose),
        Commands::Cleanup { branch } => commands::cleanup::run(branch, cli.verbose),
        Commands::Status => commands::status::run(),
        Commands::Pause { message } => commands::pause::run(message, cli.verbose),
        Commands::Abandon => commands::abandon::run(cli.verbose),
        Commands::Undo => commands::undo::run(cli.verbose),
        Commands::Sync => commands::sync::run(cli.verbose),
        Commands::Open => commands::open::run(cli.verbose),
        Commands::Worktree { command } => match command {
            WorktreeCommands::Pool { command } => match command {
                PoolCommands::Warm { count } => commands::worktree_pool::warm(count, cli.verbose),
                PoolCommands::Acquire => commands::worktree_pool::acquire(cli.verbose),
                PoolCommands::Release { name } => {
                    commands::worktree_pool::release(name, cli.verbose)
                }
                PoolCommands::Status => commands::worktree_pool::status(cli.verbose),
                PoolCommands::Drain { force } => commands::worktree_pool::drain(force, cli.verbose),
            },
        },
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            output::error(&e.to_string());
            ExitCode::FAILURE
        }
    }
}
