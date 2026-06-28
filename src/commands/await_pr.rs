//! `gw await` command - Watch a specific PR to completion, then clean up.
//!
//! `await` is the closing bookend of the workflow. Its irreducible job is to
//! *watch the PR until it reaches a terminal state* (merged or closed); the CI
//! wait and the post-merge cleanup are adjacent steps composed on top, each
//! toggleable by a flag.
//!
//! The PR is identified by **number**, not by the current branch. This keeps a
//! background watcher bound to one PR even if you switch branches (e.g. while
//! working a stacked PR), and lets the post-merge cleanup target the PR's own
//! head branch rather than whatever happens to be checked out.
//!
//! Phases:
//! 1. Wait for CI       — `gh pr checks --watch` (skip with `--no-wait`)
//! 2. Watch for merge   — poll the PR state every `--interval` seconds
//! 3. On merge          — notify, then `gw cleanup <head branch>` (skip with `--no-cleanup`)
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

/// Execute the `await` command for a specific PR number
pub fn run(
    pr_number: u64,
    open_browser: bool,
    no_wait: bool,
    no_cleanup: bool,
    ignore_ci_failure: bool,
    interval: u64,
    verbose: bool,
) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    println!();

    if !github::is_gh_available() {
        return Err(GwError::Other(
            "GitHub CLI (gh) is not available. Install it from https://cli.github.com/".into(),
        ));
    }

    // Resolve the PR by number so the watcher is independent of the current branch.
    let pr = match github::get_pr_for_branch(&pr_number.to_string())? {
        Some(pr) => pr,
        None => {
            output::warn(&format!("No PR #{} found.", pr_number));
            return Ok(());
        }
    };

    // The branch to clean up after merge is the PR's head branch, resolved from
    // GitHub — never the current checkout (which may differ for stacked work).
    let head_branch = pr.head_branch.clone();

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
            return finish_merged(&head_branch, no_cleanup, verbose);
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

    // Phase 1: wait for CI. A CI failure stops here (the PR won't merge until
    // it's fixed) unless the user opted into watching anyway.
    if !no_wait {
        output::info(&format!("Waiting for CI checks on PR #{}...", pr.number));
        wait_for_ci(pr.number, ignore_ci_failure, verbose)?;
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
            finish_merged(&head_branch, no_cleanup, verbose)
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

/// Handle a merged PR: clean up the PR's head branch unless suppressed.
fn finish_merged(head_branch: &str, no_cleanup: bool, verbose: bool) -> Result<()> {
    if head_branch.is_empty() {
        // No head branch resolved (e.g. unexpected GitHub output). Don't risk
        // deleting the wrong branch — leave cleanup to the user.
        output::warn("Could not determine the PR's branch; skipping cleanup.");
        output::hints(&["gw cleanup <branch>  # Delete the merged branch manually"]);
        return Ok(());
    }
    if no_cleanup {
        output::ready("Merged", head_branch);
        let hint = format!(
            "gw cleanup {}  # Delete the merged branch when ready",
            head_branch
        );
        output::hints(&[&hint]);
        return Ok(());
    }
    println!();
    output::info(&format!("Cleaning up merged branch '{}'...", head_branch));
    cleanup::run(Some(head_branch.to_string()), verbose)
}

/// Seconds to keep polling for CI checks to *register* before proceeding
/// without a CI wait (e.g. a repo with no CI configured for this PR).
const CHECKS_REGISTER_TIMEOUT_SECS: u64 = 90;
/// Seconds between polls while waiting for checks to register.
const CHECKS_REGISTER_POLL_SECS: u64 = 3;

/// Whether CI checks have shown up for a PR yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChecksPresence {
    /// At least one check exists (passed, pending, or failed).
    Present,
    /// No checks reported yet — CI may simply not have started.
    None,
}

/// Wait for CI checks to finish, then report whether they passed.
///
/// `gh pr checks --watch` bails immediately with "no checks reported" when CI
/// hasn't registered any checks yet — common in the seconds right after a PR is
/// opened, which is exactly when `await` is meant to be launched. Treating that
/// as a failure meant `await` could stop without ever waiting for CI. So we
/// first poll until checks appear, *then* watch them. If none register within
/// the timeout (e.g. no CI on this repo), we proceed without waiting.
///
/// Once checks exist, a non-zero `--watch` exit means they didn't all pass.
/// Since the PR can't merge until that's fixed, we stop and report the failure
/// by default; `ignore_ci_failure` downgrades it to a warning and continues.
fn wait_for_ci(pr_number: u64, ignore_ci_failure: bool, verbose: bool) -> Result<()> {
    let num = pr_number.to_string();

    if !wait_for_checks_to_register(&num, verbose)? {
        output::warn(&format!(
            "No CI checks registered for PR #{num} — nothing to wait for."
        ));
        return Ok(());
    }

    let args = ["pr", "checks", &num, "--watch"];
    if verbose {
        output::action(&format!("gh {}", args.join(" ")));
    }
    match Command::new("gh").args(args).status() {
        Ok(status) if status.success() => {
            output::success("CI checks passed");
            Ok(())
        }
        Ok(_) if ignore_ci_failure => {
            output::warn("CI checks did not all pass — continuing anyway (--ignore-ci-failure)");
            Ok(())
        }
        Ok(_) => Err(GwError::Other(format!(
            "CI checks for PR #{pr_number} did not all pass. Fix the PR, push again, then rerun \
             `gw await {pr_number} --open` (or pass --ignore-ci-failure to watch regardless)."
        ))),
        Err(e) => Err(GwError::Other(format!(
            "Could not watch CI checks for PR #{pr_number}: {e}"
        ))),
    }
}

/// Poll until CI checks register for the PR, so the subsequent `--watch` has
/// something to watch instead of exiting instantly.
///
/// Returns `true` once checks are present, or `false` if none appear within
/// `CHECKS_REGISTER_TIMEOUT_SECS` (so the caller can proceed without a wait).
fn wait_for_checks_to_register(pr: &str, verbose: bool) -> Result<bool> {
    let mut waited = 0;
    loop {
        if matches!(query_checks_presence(pr, verbose)?, ChecksPresence::Present) {
            return Ok(true);
        }
        if waited >= CHECKS_REGISTER_TIMEOUT_SECS {
            return Ok(false);
        }
        if waited == 0 {
            output::info("Waiting for CI checks to register...");
        }
        thread::sleep(Duration::from_secs(CHECKS_REGISTER_POLL_SECS));
        waited += CHECKS_REGISTER_POLL_SECS;
    }
}

/// Ask `gh` whether any CI checks exist for the PR yet (without watching).
fn query_checks_presence(pr: &str, verbose: bool) -> Result<ChecksPresence> {
    let args = ["pr", "checks", pr];
    if verbose {
        output::action(&format!("gh {}", args.join(" ")));
    }
    let output = Command::new("gh")
        .args(args)
        .output()
        .map_err(|e| GwError::Other(format!("Could not query CI checks for PR #{pr}: {e}")))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(classify_checks_presence(
        output.status.success(),
        output.status.code(),
        &stderr,
    ))
}

/// Classify a `gh pr checks` (no `--watch`) result into whether checks exist.
///
/// Exit 0 (all passed) or 8 (pending) means checks are present. Exit 1 with a
/// "no checks reported" message means none have registered yet. Any other
/// failure is treated as "present" so the real status surfaces under `--watch`
/// rather than being mistaken for a no-CI repo.
fn classify_checks_presence(success: bool, code: Option<i32>, stderr: &str) -> ChecksPresence {
    if success || code == Some(8) {
        return ChecksPresence::Present;
    }
    if stderr.contains("no checks reported") {
        ChecksPresence::None
    } else {
        ChecksPresence::Present
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_passed_means_checks_present() {
        assert_eq!(
            classify_checks_presence(true, Some(0), ""),
            ChecksPresence::Present
        );
    }

    #[test]
    fn pending_exit_code_means_checks_present() {
        // `gh pr checks` exits 8 while checks are still running.
        assert_eq!(
            classify_checks_presence(false, Some(8), ""),
            ChecksPresence::Present
        );
    }

    #[test]
    fn no_checks_reported_means_none_yet() {
        // The race right after PR creation: CI hasn't registered any checks.
        assert_eq!(
            classify_checks_presence(
                false,
                Some(1),
                "no checks reported on the 'feature/x' branch"
            ),
            ChecksPresence::None
        );
    }

    #[test]
    fn other_failure_is_treated_as_present() {
        // A genuine check failure must not be mistaken for "no CI" — let
        // `--watch` surface it as a failure instead.
        assert_eq!(
            classify_checks_presence(false, Some(1), "1 failing check"),
            ChecksPresence::Present
        );
    }
}
