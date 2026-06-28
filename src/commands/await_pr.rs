//! `gw await` command - Watch the current branch's PR to completion, then clean up.
//!
//! `await` is the closing bookend of the workflow. Its irreducible job is to
//! *watch the PR until it reaches a terminal state* (merged or closed); the CI
//! wait and the post-merge cleanup are adjacent steps composed on top, each
//! toggleable by a flag.
//!
//! Phases:
//! 1. Wait for CI       — `gh pr checks --watch` (skip with `--no-wait`)
//! 2. Watch for merge   — poll the PR state every `--interval` seconds
//! 3. On merge          — notify, then `gw cleanup` (skip with `--no-cleanup`)
//!
//! Environment-specific behavior stays in the user's dotfiles, not the CLI:
//! - `--open` opens the URL via `GW_OPEN_URL_CMD`/`OPEN_URL_CMD` (see `gw open`)
//! - the merge notification is delegated to `GW_NOTIFY_CMD` (called as
//!   `$GW_NOTIFY_CMD "<message>"`), e.g. a script wrapping macOS `osascript`.

use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::commands::{cleanup, open};
use crate::error::{GwError, Result};
use crate::git;
use crate::github::{self, PrState};
use crate::output;
use crate::state::RepoType;

/// Execute the `await` command
pub fn run(
    open_browser: bool,
    no_wait: bool,
    no_cleanup: bool,
    interval: u64,
    verbose: bool,
) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let repo_type = RepoType::detect()?;
    let home_branch = repo_type.home_branch();
    let current = git::current_branch()?;

    println!();

    if current == home_branch {
        output::warn(&format!(
            "On home branch '{}'. No PR to await.",
            home_branch
        ));
        output::hints(&["gw new feature/your-feature  # Start a branch first"]);
        return Ok(());
    }

    if !github::is_gh_available() {
        return Err(GwError::Other(
            "GitHub CLI (gh) is not available. Install it from https://cli.github.com/".into(),
        ));
    }

    let pr = match github::get_pr_for_branch(&current)? {
        Some(pr) => pr,
        None => {
            output::warn(&format!("No PR found for branch '{}'.", current));
            output::hints(&["gh pr create -a \"@me\" -t \"...\"  # Create a PR first"]);
            return Ok(());
        }
    };

    output::info(&format!("PR: #{} {}", pr.number, pr.title));

    // Optionally open in the browser up front.
    if open_browser {
        match open::open_url(&pr.url, verbose) {
            Ok(()) => output::success(&format!("Opened {}", pr.url)),
            Err(e) => output::warn(&format!("Could not open browser: {}", e)),
        }
    }

    // If the PR is already terminal, handle it without watching.
    match &pr.state {
        PrState::Merged { .. } => {
            output::success(&format!("PR #{} is already merged", pr.number));
            return finish_merged(&current, no_cleanup, verbose);
        }
        PrState::Closed => {
            output::warn(&format!(
                "PR #{} is already closed without merging",
                pr.number
            ));
            return Ok(());
        }
        PrState::Open => {}
    }

    // Phase 1: wait for CI.
    if !no_wait {
        output::info(&format!("Waiting for CI checks on PR #{}...", pr.number));
        wait_for_ci(pr.number, verbose);
    }

    // Phase 2: poll until the PR reaches a terminal state.
    let poll_secs = interval.max(1);
    output::info(&format!(
        "Watching PR #{} for merge (every {}s)...",
        pr.number, poll_secs
    ));
    let terminal = poll_until_terminal(pr.number, poll_secs);

    // Phase 3: react to the terminal state.
    match terminal {
        PrState::Merged { .. } => {
            output::success(&format!("PR #{} merged!", pr.number));
            notify(&format!("PR #{} merged", pr.number), verbose);
            finish_merged(&current, no_cleanup, verbose)
        }
        PrState::Closed => {
            output::warn(&format!("PR #{} was closed without merging", pr.number));
            notify(
                &format!("PR #{} closed without merging", pr.number),
                verbose,
            );
            Ok(())
        }
        PrState::Open => unreachable!("poll_until_terminal only returns terminal states"),
    }
}

/// Handle a merged PR: clean up the branch unless suppressed.
fn finish_merged(branch: &str, no_cleanup: bool, verbose: bool) -> Result<()> {
    if no_cleanup {
        output::ready("Merged", branch);
        output::hints(&["gw cleanup  # Delete the merged branch when ready"]);
        return Ok(());
    }
    println!();
    output::info("Cleaning up merged branch...");
    cleanup::run(None, verbose)
}

/// Wait for CI checks to finish by delegating to `gh pr checks --watch`.
///
/// A non-zero exit (failed/missing checks) is not fatal: the PR may still be
/// mergeable, so we warn and continue to the merge watch.
fn wait_for_ci(pr_number: u64, verbose: bool) {
    let num = pr_number.to_string();
    let args = ["pr", "checks", &num, "--watch"];
    if verbose {
        output::action(&format!("gh {}", args.join(" ")));
    }
    match Command::new("gh").args(args).status() {
        Ok(status) if status.success() => output::success("CI checks passed"),
        Ok(_) => output::warn("CI checks did not all pass — continuing anyway"),
        Err(e) => output::warn(&format!("Could not watch CI checks: {} — continuing", e)),
    }
}

/// Poll the PR state until it is merged or closed.
///
/// Polls *by PR number* so it keeps working after the remote branch is deleted
/// on merge. Transient lookup failures are retried rather than aborting.
fn poll_until_terminal(pr_number: u64, interval_secs: u64) -> PrState {
    let selector = pr_number.to_string();
    let delay = Duration::from_secs(interval_secs);
    loop {
        match github::get_pr_for_branch(&selector) {
            Ok(Some(pr)) => match pr.state {
                PrState::Open => {}
                terminal => return terminal,
            },
            Ok(None) => output::warn("PR not found while polling — retrying"),
            Err(e) => output::warn(&format!("Could not fetch PR status: {} — retrying", e)),
        }
        thread::sleep(delay);
    }
}

/// Send a desktop notification via `GW_NOTIFY_CMD`, if configured.
///
/// The command is invoked as `$GW_NOTIFY_CMD "<message>"`. This keeps
/// platform-specific notification logic (e.g. macOS `osascript`) in the user's
/// dotfiles rather than the CLI. No-op when the variable is unset or empty.
fn notify(message: &str, verbose: bool) {
    let cmd = match std::env::var("GW_NOTIFY_CMD") {
        Ok(cmd) if !cmd.is_empty() => cmd,
        _ => return,
    };
    if verbose {
        output::action(&format!("{} {}", cmd, message));
    }
    if let Err(e) = Command::new(&cmd).arg(message).status() {
        output::warn(&format!("Notify command '{}' failed: {}", cmd, e));
    }
}
