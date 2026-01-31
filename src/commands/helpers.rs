//! Shared helpers for commands

use crate::error::Result;
use crate::git;
use crate::output;

/// Pull from origin and display results
///
/// This function:
/// 1. Displays "Syncing with {remote}..." message
/// 2. Pulls from origin
/// 3. Shows how many commits were pulled (or "Already up to date")
///
/// Returns Ok(true) if new commits were pulled, Ok(false) if already up to date.
pub fn pull_with_output(default_remote: &str, default_branch: &str, verbose: bool) -> Result<bool> {
    output::info(&format!("Syncing with {}...", default_remote));

    let before = git::head_commit()?;
    git::pull("origin", default_branch, verbose)?;
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
