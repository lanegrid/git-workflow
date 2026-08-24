//! Shared helpers for commands

use crate::error::Result;
use crate::git;
use crate::output;

/// Fast-forward the home branch *ref* to the remote tip before checking it out.
///
/// `checkout home` followed by `git pull` moves the working tree twice: first
/// back to the stale local home (files from just-merged work vanish), then
/// forward to the remote tip (they come back). Anything watching the tree —
/// dev servers, hot reload — sees a mass delete + re-add. Moving the ref
/// first, while home is not checked out, touches no files at all; the single
/// checkout then lands directly on the up-to-date tree, which after a merge
/// barely differs from the feature branch being left.
///
/// Best-effort by design. It acts only when `home_branch` is strictly behind
/// `default_remote` (a pure fast-forward — never when diverged or ahead), and
/// `git branch -f` itself refuses to move a branch checked out in any
/// worktree. Every refused case falls through to the old behavior: check out
/// the stale home, then `pull --ff-only` reports or resolves it as before.
///
/// Call after fetching, and only when the home branch is not the current
/// branch.
pub fn fast_forward_home_ref(home_branch: &str, default_remote: &str, verbose: bool) {
    if !git::ref_exists(default_remote) || !git::is_ancestor(home_branch, default_remote) {
        return;
    }
    let behind = git::behind_base_count(home_branch, default_remote);
    if behind == 0 {
        return;
    }
    match git::force_update_branch(home_branch, default_remote, verbose) {
        Ok(()) => output::success(&format!(
            "Fast-forwarded {} to {} ({} commit(s), ref only)",
            output::bold(home_branch),
            default_remote,
            behind
        )),
        // e.g. held by another worktree — the pull after checkout handles it.
        Err(e) => {
            if verbose {
                output::warn(&format!("Could not fast-forward {home_branch}: {e}"));
            }
        }
    }
}

/// Pull from origin (fast-forward only) and display results
///
/// This function:
/// 1. Displays "Syncing with {remote}..." message
/// 2. Pulls from origin using fast-forward only
/// 3. Shows how many commits were pulled (or "Already up to date")
///
/// Returns Ok(true) if new commits were pulled, Ok(false) if already up to date.
/// Returns an error if the local branch has diverged from the remote.
pub fn pull_with_output(default_remote: &str, default_branch: &str, verbose: bool) -> Result<bool> {
    output::info(&format!("Syncing with {}...", default_remote));

    let before = git::head_commit()?;

    if let Err(e) = git::pull_ff_only("origin", default_branch, verbose) {
        output::error("Cannot fast-forward. Local branch has diverged from remote.");
        output::hints(&[
            &format!("git rebase {}  # Rebase local changes", default_remote),
            "git pull       # Merge (creates merge commit)",
        ]);
        return Err(e);
    }

    let after = git::head_commit()?;

    if before != after {
        let count = git::commit_count(&before, &after)?;
        output::success(&format!(
            "Pulled {} commit(s) from {}",
            output::bold(&count.to_string()),
            default_remote
        ));
        Ok(true)
    } else {
        output::success(&format!("Already up to date with {}", default_remote));
        Ok(false)
    }
}
