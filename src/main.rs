//! git-workflow - Git workflow CLI

use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;

use git_workflow::cli::{Cli, Commands, PoolCommands, WorktreeCommands};
use git_workflow::commands;
use git_workflow::error::GwError;
use git_workflow::output;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => return handle_parse_error(e),
    };

    let result = match cli.command {
        Commands::Home => commands::home::run(cli.verbose),
        Commands::New { branch, stack } => commands::new::run(branch, stack, cli.verbose),
        Commands::Cleanup { branch } => commands::cleanup::run(branch, cli.verbose),
        Commands::Status => commands::status::run(),
        Commands::Pause { message } => commands::pause::run(message, cli.verbose),
        Commands::Abandon => commands::abandon::run(cli.verbose),
        Commands::Undo => commands::undo::run(cli.verbose),
        Commands::Sync => commands::sync::run(cli.verbose),
        Commands::Open => commands::open::run(cli.verbose),
        Commands::Await {
            pr,
            open,
            no_wait,
            no_cleanup,
            ignore_ci_failure,
            interval,
        } => commands::await_pr::run(
            pr,
            open,
            no_wait,
            no_cleanup,
            ignore_ci_failure,
            interval,
            cli.verbose,
        ),
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
            if matches!(e, GwError::BranchCheckedOutElsewhere { .. }) {
                output::hints(&[
                    "git worktree list   # See which worktree holds the branch",
                    "gw status           # Check current state",
                ]);
            }
            ExitCode::FAILURE
        }
    }
}

/// Render clap parse failures, then guide the user to a runnable next command.
///
/// `--help`/`--version` aren't failures: print them as-is and exit 0. Real usage
/// errors (bad subcommand, missing argument) get clap's message plus a `Try:`
/// footer pointing at `gw status` -- the command that names the next step -- so
/// the user never has to reconstruct the syntax themselves.
fn handle_parse_error(e: clap::Error) -> ExitCode {
    let kind = e.kind();
    // clap writes help to stdout and errors to stderr; let it pick the stream.
    let _ = e.print();

    match kind {
        ErrorKind::DisplayHelp
        | ErrorKind::DisplayVersion
        | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => ExitCode::SUCCESS,
        _ => {
            output::error_footer(&[
                "gw status     # show where you are and the next command to run",
                "gw --help     # full command reference",
            ]);
            // Clap uses exit code 2 for usage errors; mirror that.
            ExitCode::from(2)
        }
    }
}
