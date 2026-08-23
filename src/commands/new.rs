//! `gw new` command - Create a new branch with a structurally unambiguous base.
//!
//! Three invariants make accidental mistakes unrepresentable:
//!
//! 1. **No ambiguous base.** A base is auto-chosen only where exactly one makes
//!    sense: the home branch (→ `origin/main`). From any other branch the base
//!    is ambiguous (sibling vs stack), so `gw new` refuses and demands `--stack`
//!    (base on the current branch) or returning home first.
//! 2. **No implicit merge.** When the working tree is dirty, the start point is
//!    the current HEAD, so creating the branch never performs a working-tree
//!    merge and therefore can never conflict or fail cryptically.
//! 3. **No silent displacement.** Uncommitted work only ever travels onto a
//!    branch whose base the user explicitly established, and it is always
//!    reported.

use crate::error::{GwError, Result};
use crate::git;
use crate::output;
use crate::state::{RepoType, WorkingDirState};

/// Execute the `new` command
pub fn run(branch_name: Option<String>, stack: bool, verbose: bool) -> Result<()> {
    // Ensure we're in a git repo
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    // A detached HEAD has no branch context, so we can't tell home from a
    // feature branch nor stack on anything. Refuse rather than guess.
    if git::is_detached_head() {
        return Err(GwError::Other(
            "Cannot run gw new from detached HEAD. Checkout a branch first.".to_string(),
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

    let current = git::current_branch()?;
    let repo_type = RepoType::detect()?;
    let home_branch = repo_type.home_branch();
    let on_home = current == home_branch;

    // Invariant 1: the base must be unambiguous.
    if stack && on_home {
        // --stack means "stack on the feature branch I'm on"; on home that's
        // meaningless -- plain `gw new` already starts fresh from origin/main.
        output::error(&format!(
            "--stack requires a non-home branch, but you are on '{}'.",
            current
        ));
        output::hints(&[
            "gw new feature/your-feature                            # start fresh from origin/main",
            "git checkout <parent> && gw new feature/child --stack  # stack on a feature branch",
        ]);
        return Err(GwError::Other(
            "--stack requires a non-home current branch".to_string(),
        ));
    }
    if !stack && !on_home {
        // Refuse to silently base on origin/main from a feature branch: that
        // would strip uncommitted work off the branch and pick a base the user
        // never chose. Force the explicit decision instead.
        output::error(&format!(
            "You are on '{}', not the home branch '{}'.",
            current, home_branch
        ));
        output::hints(&[
            &format!("gw new {branch_name} --stack          # stack on {current}"),
            &format!("gw home && gw new {branch_name}        # start fresh from {home_branch}"),
        ]);
        return Err(GwError::Other(
            "gw new outside the home branch needs --stack (or run gw home first)".to_string(),
        ));
    }

    let working_dir = WorkingDirState::detect();
    let dirty = !working_dir.is_clean();

    // Resolve the start point per invariants 1 & 2.
    //
    // - --stack: base on the current branch's HEAD (local; no fetch needed).
    // - home + clean: base on a freshly fetched origin/main.
    // - home + dirty: base on the current HEAD so carrying the working tree
    //   needs no merge; if local main lags origin/main, defer the catch-up to a
    //   clean rebase after committing.
    let mut behind_count = 0usize;
    let (start_point, base_label, pr_base): (String, String, Option<String>) = if stack {
        (current.clone(), current.clone(), Some(current.clone()))
    } else {
        output::info("Fetching from origin...");
        git::fetch_prune(verbose)?;
        output::success("Fetched");
        let default_remote = git::get_default_remote_branch()?;

        if dirty {
            behind_count = git::commit_count(&current, &default_remote).unwrap_or(0);
            (current.clone(), current.clone(), None)
        } else {
            (default_remote.clone(), default_remote, None)
        }
    };

    // Invariant 3: surface that uncommitted work is moving onto the new branch.
    if dirty {
        output::warn(&format!(
            "Working directory has changes ({}); they will move onto {}",
            working_dir.description(),
            output::bold(&branch_name)
        ));
    }

    // Create the branch. The start point is always the current HEAD when dirty,
    // so this never performs a working-tree merge.
    git::checkout_new_branch(&branch_name, &start_point, verbose)?;
    output::success(&format!(
        "Created branch {} from {}",
        output::bold(&branch_name),
        base_label
    ));

    // Record the stacked base locally so `gw status` can suggest the right PR
    // base (`-B <parent>`) before the PR exists. (A stale entry -- e.g. parent
    // merged before this branch's PR is opened -- is left for the parent/child
    // guard work to handle; `gh pr create` errors loudly on a missing base.)
    if let Some(base) = &pr_base {
        git::set_branch_base(&branch_name, base, verbose)?;
        // Record the base tip SHA (= the fork point, since --stack branches off
        // the current HEAD) so a later restack can `rebase --onto` even if the
        // base branch has since been deleted.
        if let Ok(sha) = git::head_commit() {
            git::set_branch_base_sha(&branch_name, &sha, verbose)?;
        }
    }

    if behind_count > 0 {
        output::warn(&format!(
            "local {} is behind origin/{} ({} commit(s)); rebase after committing",
            home_branch, home_branch, behind_count
        ));
    }

    // Show current position
    let commit_short = git::short_commit()?;
    let commit_msg = git::head_commit_message()?;

    output::ready("Ready to work", &branch_name);
    println!("Base: {commit_short} {commit_msg}");

    // Build the next-step hints, inserting a rebase step when local main lagged
    // and a `-B <parent>` PR base for stacked branches.
    let mut hint_lines: Vec<String> = vec![
        "# Make changes, then:".to_string(),
        "git add <files> && git commit -m \"feat: description\"".to_string(),
    ];
    if behind_count > 0 {
        hint_lines.push("gw sync  # local main was behind; catch up".to_string());
    }
    hint_lines.push(format!("git push -u origin {branch_name}"));
    hint_lines.push(match &pr_base {
        Some(base) => format!("gh pr create -a \"@me\" -B {base} -t \"Title\""),
        None => "gh pr create -a \"@me\" -t \"Title\"".to_string(),
    });
    let hint_refs: Vec<&str> = hint_lines.iter().map(String::as_str).collect();
    output::hints(&hint_refs);

    Ok(())
}
