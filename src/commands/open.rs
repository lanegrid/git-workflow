//! `gw open` command - Open the PR for the current branch in the browser
//!
//! Resolves the PR for the current branch via `gh` and opens its URL in the
//! browser. The command used to open the URL is configurable so that
//! environment-specific behavior (e.g. selecting a particular Chrome profile)
//! lives in the user's dotfiles, not in this CLI.
//!
//! # Browser command resolution
//!
//! The URL is opened by invoking a single program with the URL as its only
//! argument (`<program> <url>`). The program is resolved in this order:
//!
//! 1. `$GW_OPEN_URL_CMD` - gw-specific override
//! 2. `$OPEN_URL_CMD`     - generic override (shared with other tooling)
//! 3. OS default          - `open` on macOS, `xdg-open` elsewhere
//!
//! Set `GW_OPEN_URL_CMD`/`OPEN_URL_CMD` to a script that takes the URL as `$1`.

use std::process::Command;

use crate::error::{GwError, Result};
use crate::git;
use crate::github;
use crate::output;
use crate::state::RepoType;

/// Execute the `open` command
pub fn run(verbose: bool) -> Result<()> {
    // Ensure we're in a git repo
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let repo_type = RepoType::detect()?;
    let home_branch = repo_type.home_branch();
    let current = git::current_branch()?;

    println!();

    // On the home branch there is no feature PR to open
    if current == home_branch {
        output::warn(&format!("On home branch '{}'. No PR to open.", home_branch));
        output::hints(&["gw new feature/your-feature  # Start a branch first"]);
        return Ok(());
    }

    // Need gh to look up the PR for this branch
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
    open_url(&pr.url, verbose)?;
    output::success(&format!("Opened {}", pr.url));

    Ok(())
}

/// Open a URL in the browser using the resolved open command.
fn open_url(url: &str, verbose: bool) -> Result<()> {
    let gw_cmd = std::env::var("GW_OPEN_URL_CMD").ok();
    let open_cmd = std::env::var("OPEN_URL_CMD").ok();
    let program = resolve_open_program(
        gw_cmd.as_deref(),
        open_cmd.as_deref(),
        default_open_program(),
    );

    if verbose {
        output::action(&format!("{} {}", program, url));
    }

    let status = Command::new(&program)
        .arg(url)
        .status()
        .map_err(|e| GwError::Other(format!("Failed to run open command '{program}': {e}")))?;

    if !status.success() {
        return Err(GwError::Other(format!(
            "Open command '{program}' exited with a non-zero status"
        )));
    }

    Ok(())
}

/// The OS default program for opening a URL.
fn default_open_program() -> &'static str {
    if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    }
}

/// Resolve which program should be used to open a URL.
///
/// Precedence: `GW_OPEN_URL_CMD` > `OPEN_URL_CMD` > OS default. Empty values
/// are ignored so that an exported-but-blank variable falls through.
fn resolve_open_program(gw_cmd: Option<&str>, open_cmd: Option<&str>, os_default: &str) -> String {
    gw_cmd
        .filter(|s| !s.is_empty())
        .or_else(|| open_cmd.filter(|s| !s.is_empty()))
        .unwrap_or(os_default)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gw_cmd_takes_precedence() {
        let program = resolve_open_program(Some("gw-open"), Some("open-url"), "open");
        assert_eq!(program, "gw-open");
    }

    #[test]
    fn falls_back_to_open_url_cmd() {
        let program = resolve_open_program(None, Some("open-url"), "open");
        assert_eq!(program, "open-url");
    }

    #[test]
    fn falls_back_to_os_default() {
        let program = resolve_open_program(None, None, "xdg-open");
        assert_eq!(program, "xdg-open");
    }

    #[test]
    fn empty_values_are_ignored() {
        // Exported-but-blank GW var should fall through to OPEN_URL_CMD
        assert_eq!(
            resolve_open_program(Some(""), Some("open-url"), "open"),
            "open-url"
        );
        // Both blank → OS default
        assert_eq!(resolve_open_program(Some(""), Some(""), "open"), "open");
    }
}
