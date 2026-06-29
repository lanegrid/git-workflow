//! `gw new` command - Create new branch from origin/main (or the current branch with --stack)

use crate::error::{GwError, Result};
use crate::git;
use crate::output;
use crate::state::{RepoType, WorkingDirState};

/// Execute the `new` command
///
/// By default the branch is created from a freshly fetched `origin/main`. With
/// `stack = true` it is created from the *current* branch's HEAD instead, so the
/// new branch stacks on top of an in-flight PR. Stacking from the current HEAD
/// carries any uncommitted changes over without a rebase, so it can't conflict
/// the way basing on `origin/main` can.
pub fn run(branch_name: Option<String>, stack: bool, verbose: bool) -> Result<()> {
    // Ensure we're in a git repo
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    // --stack reads the current branch as the base; a detached HEAD has no
    // branch to stack on, so refuse early with an actionable message.
    if stack && git::is_detached_head() {
        return Err(GwError::Other(
            "Cannot use --stack from detached HEAD. Checkout a branch first.".to_string(),
        ));
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

    // Resolve the base before mutating anything so we can fail fast (e.g.
    // --stack on the home branch) without leaving a half-created branch.
    let base = if stack {
        let current = git::current_branch()?;
        let repo_type = RepoType::detect()?;
        let home_branch = repo_type.home_branch();

        // --stack means "stack on top of the feature branch I'm on". On the
        // home branch that's meaningless -- plain `gw new` already starts fresh
        // from the default branch -- so refuse rather than create a branch that
        // tracks `main` under a stacked PR hint.
        if current == home_branch {
            output::error(&format!(
                "--stack requires a non-home branch, but you are on '{}'.",
                current
            ));
            output::hints(&[
                "gw new feature/your-feature                    # start fresh from the default branch",
                "git checkout <parent> && gw new feature/child --stack  # stack on a feature branch",
            ]);
            return Err(GwError::Other(
                "--stack requires a non-home current branch".to_string(),
            ));
        }

        current
    } else {
        // Fetch so the default branch base is current. (--stack bases on the
        // local current branch, so it needs no fetch.)
        output::info("Fetching from origin...");
        git::fetch_prune(verbose)?;
        output::success("Fetched");

        git::get_default_remote_branch()?
    };

    output::info(&format!("Base branch: {}", output::bold(&base)));

    // Surface that uncommitted work is moving onto the new branch instead of
    // doing it silently. `git checkout -b` carries the working tree along; for
    // --stack the start point is the current HEAD so this never conflicts.
    let working_dir = WorkingDirState::detect();
    if !working_dir.is_clean() {
        output::warn(&format!(
            "Working directory has changes ({}); they will move onto {}",
            working_dir.description(),
            output::bold(&branch_name)
        ));
    }

    // Create branch from the resolved base
    git::checkout_new_branch(&branch_name, &base, verbose)?;
    output::success(&format!(
        "Created branch {} from {}",
        output::bold(&branch_name),
        base
    ));

    // Show current position
    let commit_short = git::short_commit()?;
    let commit_msg = git::head_commit_message()?;

    output::ready("Ready to work", &branch_name);
    println!("Base: {commit_short} {commit_msg}");

    // Stacked branches need an explicit PR base: a branch cut from the current
    // branch doesn't make GitHub default the PR base to that parent.
    let pr_create = if stack {
        format!("gh pr create -a \"@me\" -B {base} -t \"Title\"")
    } else {
        "gh pr create -a \"@me\" -t \"Title\"".to_string()
    };
    output::hints(&[
        "# Make changes, then:",
        "git add <files> && git commit -m \"feat: description\"",
        &format!("git push -u origin {branch_name}"),
        &pr_create,
    ]);

    Ok(())
}
