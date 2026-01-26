//! `gw new` command - Create new branch from origin/main

use crate::error::{GwError, Result};
use crate::git;
use crate::output;

/// Execute the `new` command
pub fn run(branch_name: Option<String>, verbose: bool) -> Result<()> {
    // Ensure we're in a git repo
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let branch_name = branch_name.ok_or(GwError::BranchNameRequired)?;

    println!();
    output::info(&format!("Creating branch: {}", output::bold(&branch_name)));

    // Check if branch already exists
    if git::branch_exists(&branch_name) {
        output::error(&format!("Branch '{}' already exists locally", branch_name));
        println!();
        output::action(&format!(
            "git checkout {}  # Switch to existing branch",
            branch_name
        ));
        output::action(&format!(
            "git branch -d {}  # Delete and recreate",
            branch_name
        ));
        return Err(GwError::BranchAlreadyExists(branch_name));
    }

    // Fetch latest
    output::info("Fetching from origin...");
    git::fetch_prune(verbose)?;
    output::success("Fetched");

    // Detect default remote branch (origin/main or origin/master)
    let default_remote = git::get_default_remote_branch()?;

    // Create branch from default remote
    git::checkout_new_branch(&branch_name, &default_remote, verbose)?;
    output::success(&format!(
        "Created branch {} from {}",
        output::bold(&branch_name),
        default_remote
    ));

    // Show current position
    let commit_short = git::short_commit()?;
    let commit_msg = git::head_commit_message()?;

    output::ready("Ready to work", &branch_name);
    println!("Base: {commit_short} {commit_msg}");

    output::hints(&[
        "# Make changes, then:",
        "git add <files> && git commit -m \"feat: description\"",
        &format!("git push -u origin {branch_name}"),
        "gh pr create -a \"@me\" -t \"Title\"",
    ]);

    Ok(())
}
