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
//! State machine:
//!
//! - Wait for CI: poll until the checks reach a verdict (skip with `--no-wait`).
//!   "Pending" just *keeps waiting*; pass/fail are terminal. A branch with no CI
//!   at all ("no checks reported", confirmed across a few polls) isn't waited on
//!   — there's nothing to wait for, so we proceed. Watching the PR is the
//!   irreducible job, so this step also bails the moment the PR itself merges or
//!   closes.
//! - On CI pass (or no CI): open the PR in the browser (with `--open`), then
//!   watch for merge.
//! - On CI fail: report and stop, so you can fix → push → rerun `await`.
//! - Watch for merge: poll the PR state every `--interval` seconds.
//! - On merge: notify, then `gw cleanup <head branch>` (skip with `--no-cleanup`).
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

    // Phase 1: wait for CI. A CI failure stops here (the PR can't merge until
    // it's fixed) unless the user opted into watching anyway. If the PR merges
    // or closes while we wait, CI is moot — react to that directly.
    if !no_wait {
        match wait_for_ci(pr.number, ignore_ci_failure, interval, verbose)? {
            CiWait::Proceed => {}
            CiWait::Terminal(state) => {
                return react_to_terminal(state, pr.number, &head_branch, no_cleanup, verbose);
            }
        }
    }

    // Phase 2: CI is green (or skipped) — surface the PR for review/merge.
    if open_browser {
        match open::open_url(&pr.url, verbose) {
            Ok(()) => output::success(&format!("Opened {}", pr.url)),
            Err(e) => output::warn(&format!("Could not open browser: {}", e)),
        }
    }

    // Phase 3: poll until the PR reaches a terminal state.
    let poll_secs = interval.max(1);
    output::info(&format!(
        "Watching PR #{} for merge (every {}s)...",
        pr.number, poll_secs
    ));
    let terminal = poll_until_terminal(pr.number, poll_secs);

    // Phase 4: react to the terminal state.
    react_to_terminal(terminal, pr.number, &head_branch, no_cleanup, verbose)
}

/// React to a PR's terminal state: clean up on merge, report on close.
fn react_to_terminal(
    terminal: PrState,
    pr_number: u64,
    head_branch: &str,
    no_cleanup: bool,
    verbose: bool,
) -> Result<()> {
    match terminal {
        PrState::Merged { .. } => {
            output::success(&format!("PR #{} merged!", pr_number));
            notify(&format!("PR #{} merged", pr_number), verbose);
            finish_merged(head_branch, no_cleanup, verbose)
        }
        PrState::Closed => {
            output::warn(&format!("PR #{} was closed without merging", pr_number));
            notify(
                &format!("PR #{} closed without merging", pr_number),
                verbose,
            );
            Ok(())
        }
        PrState::Open => unreachable!("react_to_terminal is only called with terminal states"),
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

/// How many consecutive "no checks reported" polls confirm a branch simply has
/// no CI, rather than CI that hasn't registered yet. A few quick confirmations
/// absorb the brief window after a push before GitHub Actions attaches its
/// checks, without making no-CI repos wait long.
const NO_CI_CONFIRMATIONS: u32 = 3;

/// Delay between the quick "is there really no CI?" confirmation polls. Short
/// and independent of `--interval` (the merge-watch cadence) so detecting a
/// no-CI branch takes seconds, not a full merge-poll interval.
const NO_CI_PROBE_DELAY: Duration = Duration::from_secs(2);

/// Where a PR's CI checks are in their lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CiState {
    /// No checks reported. Either CI hasn't registered yet (keep waiting a few
    /// polls) or the branch has no CI at all (proceed without waiting).
    NotStarted,
    /// Checks exist but haven't all finished. Keep waiting.
    Pending,
    /// All checks passed. Terminal.
    Passed,
    /// At least one check failed. Terminal.
    Failed,
}

/// Outcome of the CI wait: either CI settled and we should keep watching for
/// merge, or the PR reached a terminal state while we waited.
enum CiWait {
    /// CI passed (or a failure was ignored) — proceed to watch for merge.
    Proceed,
    /// The PR merged or closed before CI settled — react to it directly.
    Terminal(PrState),
}

/// Wait for CI to reach a verdict, then report pass/fail.
///
/// This is a plain state machine: poll the PR's checks until they settle.
/// `Pending` (checks exist but haven't finished) just *keeps waiting*; only
/// `Passed`/`Failed` end the loop on a verdict. There is deliberately **no**
/// timeout once real checks exist — "wait for CI" means wait for CI.
///
/// `NotStarted` ("no checks reported") is the ambiguous case: CI that hasn't
/// registered yet vs. a branch with no CI at all. We confirm it across a few
/// quick polls ([`NO_CI_CONFIRMATIONS`]); if checks still never appear, there's
/// nothing to wait for, so we proceed (which lets `--open` open the PR and the
/// merge watch begin) instead of hanging forever. The user shouldn't have to
/// reach for `--no-wait` just because a repo has no CI.
///
/// Each poll also checks whether the PR itself went terminal: a merged PR can't
/// be gated on checks. When that happens we return [`CiWait::Terminal`] so the
/// caller reacts (cleanup on merge) instead of waiting. (`--no-wait` still skips
/// this phase entirely.)
///
/// On `Failed`, the PR can't merge until it's fixed, so we stop and report it
/// by default; `ignore_ci_failure` downgrades that to a warning and continues.
fn wait_for_ci(
    pr_number: u64,
    ignore_ci_failure: bool,
    interval: u64,
    verbose: bool,
) -> Result<CiWait> {
    let num = pr_number.to_string();
    let delay = Duration::from_secs(interval.max(1));
    output::info(&format!("Waiting for CI checks on PR #{num}..."));
    let mut no_checks_streak = 0u32;
    loop {
        // Watching the PR is the irreducible job. If it already merged or
        // closed, stop waiting on CI regardless of where the checks stand.
        if let Some(state) = terminal_state(&num) {
            return Ok(CiWait::Terminal(state));
        }
        match query_ci_state(&num, verbose)? {
            CiState::NotStarted => {
                // No checks yet. Confirm a few times before concluding the
                // branch simply has no CI — then proceed rather than hang.
                no_checks_streak += 1;
                if no_checks_streak >= NO_CI_CONFIRMATIONS {
                    output::info(&format!(
                        "No CI checks for PR #{num} — proceeding without waiting"
                    ));
                    return Ok(CiWait::Proceed);
                }
                thread::sleep(NO_CI_PROBE_DELAY);
            }
            // Checks appeared, so CI does exist — reset the no-CI counter and
            // wait for them on the normal cadence.
            CiState::Pending => {
                no_checks_streak = 0;
                thread::sleep(delay);
            }
            CiState::Passed => {
                output::success("CI checks passed");
                return Ok(CiWait::Proceed);
            }
            CiState::Failed if ignore_ci_failure => {
                output::warn(
                    "CI checks did not all pass — continuing anyway (--ignore-ci-failure)",
                );
                return Ok(CiWait::Proceed);
            }
            CiState::Failed => {
                return Err(GwError::Other(format!(
                    "CI checks for PR #{pr_number} did not all pass. Fix the PR, push again, then \
                     rerun `gw await {pr_number} --open` (or pass --ignore-ci-failure to watch \
                     regardless)."
                )));
            }
        }
    }
}

/// Return the PR's terminal state if it has merged or closed, else `None`.
///
/// Open PRs and transient lookup failures both yield `None`, so a caller polling
/// this keeps waiting rather than aborting on a hiccup.
fn terminal_state(selector: &str) -> Option<PrState> {
    match github::get_pr_for_branch(selector) {
        Ok(Some(pr)) => match pr.state {
            PrState::Open => None,
            terminal => Some(terminal),
        },
        _ => None,
    }
}

/// Query the PR's current CI state via `gh pr checks` (no `--watch`).
fn query_ci_state(pr: &str, verbose: bool) -> Result<CiState> {
    let args = ["pr", "checks", pr];
    if verbose {
        output::action(&format!("gh {}", args.join(" ")));
    }
    let output = Command::new("gh")
        .args(args)
        .output()
        .map_err(|e| GwError::Other(format!("Could not query CI checks for PR #{pr}: {e}")))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(classify_ci_state(
        output.status.success(),
        output.status.code(),
        &stderr,
    ))
}

/// Classify a `gh pr checks` (no `--watch`) result into a [`CiState`].
///
/// `gh` exit codes: 0 = all passed, 8 = some pending, 1 = failed *or* no checks
/// reported (disambiguated by stderr). Anything else is treated as still
/// pending so a transient `gh` hiccup retries rather than aborting the wait.
fn classify_ci_state(success: bool, code: Option<i32>, stderr: &str) -> CiState {
    if success {
        return CiState::Passed;
    }
    if code == Some(8) {
        return CiState::Pending;
    }
    if stderr.contains("no checks reported") {
        return CiState::NotStarted;
    }
    if code == Some(1) {
        return CiState::Failed;
    }
    CiState::Pending
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
    fn exit_zero_means_passed() {
        assert_eq!(classify_ci_state(true, Some(0), ""), CiState::Passed);
    }

    #[test]
    fn exit_eight_means_pending() {
        // `gh pr checks` exits 8 while checks are still running.
        assert_eq!(classify_ci_state(false, Some(8), ""), CiState::Pending);
    }

    #[test]
    fn no_checks_reported_means_not_started() {
        // The window right after PR creation: CI hasn't registered any checks.
        // This must keep waiting, not be mistaken for a failure.
        assert_eq!(
            classify_ci_state(
                false,
                Some(1),
                "no checks reported on the 'feature/x' branch"
            ),
            CiState::NotStarted
        );
    }

    #[test]
    fn exit_one_with_checks_means_failed() {
        assert_eq!(
            classify_ci_state(false, Some(1), "1 failing check"),
            CiState::Failed
        );
    }

    #[test]
    fn unknown_failure_retries_as_pending() {
        // A transient gh hiccup (e.g. network) shouldn't abort the wait.
        assert_eq!(
            classify_ci_state(false, None, "could not connect"),
            CiState::Pending
        );
    }
}
