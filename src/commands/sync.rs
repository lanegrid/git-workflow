//! `gw sync` command - Bring the current branch up to date with its base.
//!
//! "Base" is whatever this branch is meant to sit on, and `sync` always moves
//! the branch onto the latest version of it:
//!
//! | Situation                                  | What `gw sync` does                                   |
//! |--------------------------------------------|-------------------------------------------------------|
//! | on the home branch                         | fast-forward pull from `origin/main`                  |
//! | plain branch (base is `main`)              | `git rebase origin/main`, force-push if published     |
//! | stacked, base PR still open                | rebase onto `origin/<base>`, force-push               |
//! | stacked, base PR merged                    | `rebase --onto origin/main <old base>`, move the PR   |
//! |                                            | base to `main`, force-push (restack)                  |
//! | stacked before a PR exists, base merged    | `rebase --onto origin/main <recorded base tip>`       |
//!
//! Rebasing a stacked branch always uses `--onto` with the recorded base tip
//! (`gw new --stack` stores it) as the boundary, so only *this* branch's own
//! commits are replayed — never the base's, which after a squash merge would be
//! doubled and conflict-prone. A plain `git rebase` is used only when nothing
//! better is known (base is `main`, or a stack created without `gw new --stack`).
//!
//! # Example
//!
//! ```text
//! $ gw status
//!   Branch: feature/child
//!   PR: #42 (open)
//!   Base: feature/base (merged ✓)
//!
//!   Next: gw sync
//!
//! $ gw sync
//!   Rebasing commits after <base tip> onto origin/main...
//!   Updating PR base to main...
//!   Force pushing...
//!   ✓ Synced
//! ```

use super::helpers;
use crate::error::{GwError, Result};
use crate::git;
use crate::github::{self, PrState};
use crate::output;
use crate::state::{RepoType, SyncState, WorkingDirState};

/// How the branch should be moved onto its base.
struct Plan {
    /// Ref to rebase onto (`origin/main`, `origin/<parent>`, ...).
    new_base: String,
    /// `rebase --onto` boundary: replay only `boundary..HEAD`. `None` means a
    /// plain `git rebase <new_base>`.
    boundary: Option<String>,
    /// The PR whose base must move to the default branch (restack after the
    /// base PR merged).
    retarget_pr: Option<u64>,
    /// After rebasing, the branch is no longer stacked: drop the recorded base.
    unstack: bool,
    /// After rebasing, the branch is still stacked on `new_base`: re-record its
    /// tip as the next `--onto` boundary.
    rerecord_base: bool,
}

/// Execute the `sync` command
pub fn run(verbose: bool) -> Result<()> {
    // 1. Check prerequisites
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let working_dir = WorkingDirState::detect();
    if !working_dir.is_clean() {
        output::error(&format!(
            "You have uncommitted changes ({}).",
            working_dir.description()
        ));
        output::action("git add <files> && git commit -m \"...\"  # commit first");
        output::action("gw pause                                  # or park the work as WIP");
        return Err(GwError::UncommittedChanges);
    }

    // 2. Get current branch info
    let repo_type = RepoType::detect()?;
    let home_branch = repo_type.home_branch();
    let current = git::current_branch()?;

    // On home branch - just sync with origin/main
    if current == home_branch {
        println!();
        output::info(&format!("Branch: {}", output::bold(&current)));

        // Fetch latest
        output::info("Fetching from origin...");
        git::fetch_prune(verbose)?;
        output::success("Fetched (stale remote branches pruned)");

        // Detect default remote branch and sync
        let default_remote = git::get_default_remote_branch()?;
        let default_branch = default_remote.strip_prefix("origin/").unwrap_or("main");
        helpers::pull_with_output(&default_remote, default_branch, verbose)?;

        output::ready("Ready", home_branch);
        return Ok(());
    }

    println!();
    output::info(&format!("Branch: {}", output::bold(&current)));

    // 3. Fetch latest first to get accurate PR/branch state
    output::info("Fetching from origin...");
    git::fetch_prune(verbose)?;

    // 4. Look up this branch's PR. GitHub's base is authoritative once a PR
    // exists; if we can't ask (no gh, not a GitHub remote, network), fall back
    // to what we know locally — the recorded stacked base, else main — and say
    // so.
    let pr = if github::is_gh_available() {
        match github::get_pr_for_branch(&current) {
            Ok(pr) => pr,
            Err(e) => {
                output::warn(&format!("Could not fetch PR info: {e}"));
                output::warn("Assuming no PR; syncing with the locally known base.");
                None
            }
        }
    } else {
        output::warn("GitHub CLI (gh) not available; syncing with the locally known base.");
        None
    };

    // Detect default remote branch
    let default_remote = git::get_default_remote_branch()?;
    let default_branch = default_remote.strip_prefix("origin/").unwrap_or("main");

    // Locally recorded stacked base (`gw new --stack`), if any.
    let recorded_base = git::branch_base(&current).filter(|b| b != default_branch && b != &current);
    let recorded_base_sha = recorded_base
        .as_ref()
        .and_then(|_| git::branch_base_sha(&current))
        // Only usable as a boundary if it is still in this branch's history.
        .filter(|sha| git::is_ancestor(sha, "HEAD"));

    // 5. Decide the plan from the PR (GitHub's base is authoritative once a PR
    // exists) or, before a PR, from the recorded base.
    let plan = match pr {
        Some(pr) => {
            output::info(&format!("PR: #{} ({})", pr.number, pr.title));
            output::info(&format!("Base: {}", pr.base_branch));
            match &pr.state {
                PrState::Merged { .. } => {
                    output::success(&format!("PR #{} is merged. Nothing to sync.", pr.number));
                    output::hints(&["gw cleanup  # Delete the merged branch"]);
                    return Ok(());
                }
                PrState::Closed => {
                    output::warn(&format!(
                        "PR #{} was closed without merging. Nothing to sync.",
                        pr.number
                    ));
                    output::hints(&[&format!("gh pr reopen {}  # Reopen it first", pr.number)]);
                    return Ok(());
                }
                PrState::Open => {}
            }

            if pr.base_branch == default_branch {
                // The PR targets main. If it was stacked and GitHub already
                // retargeted it (the merged base branch was deleted), the
                // branch still carries the old base's commits — restack with
                // the recorded boundary instead of a plain rebase.
                match retargeted_boundary(recorded_base.as_deref(), recorded_base_sha.as_deref()) {
                    Some(boundary) => Plan {
                        new_base: default_remote.clone(),
                        boundary: Some(boundary),
                        retarget_pr: None,
                        unstack: true,
                        rerecord_base: false,
                    },
                    None => Plan {
                        new_base: default_remote.clone(),
                        boundary: None,
                        retarget_pr: None,
                        unstack: false,
                        rerecord_base: false,
                    },
                }
            } else {
                match plan_for_stacked_pr(
                    &pr.base_branch,
                    pr.number,
                    &default_remote,
                    recorded_base_sha.as_deref(),
                )? {
                    Some(plan) => plan,
                    None => return Ok(()),
                }
            }
        }
        None => match &recorded_base {
            Some(base) => {
                match plan_for_recorded_base(base, &default_remote, recorded_base_sha.as_deref())? {
                    Some(plan) => plan,
                    None => return Ok(()),
                }
            }
            None => Plan {
                new_base: default_remote.clone(),
                boundary: None,
                retarget_pr: None,
                unstack: false,
                rerecord_base: false,
            },
        },
    };

    // 6. Carry it out.
    execute(&plan, &current, default_branch, verbose)
}

/// For a PR that GitHub shows targeting the default branch: if `gw new --stack`
/// recorded a base whose PR has since merged, the branch was retargeted by
/// GitHub and still contains the base's commits. Return the `--onto` boundary
/// to replay only this branch's own commits; `None` for an ordinary branch.
fn retargeted_boundary(
    recorded_base: Option<&str>,
    recorded_base_sha: Option<&str>,
) -> Option<String> {
    let base = recorded_base?;
    if !recorded_base_pr_merged(base) {
        return None;
    }
    output::info(&format!(
        "Recorded base '{}' merged and the PR now targets the default branch — restacking",
        base
    ));
    boundary_for_merged_base(base, recorded_base_sha)
}

/// Whether the recorded stacked base's PR has merged. "Can't tell" (no gh, not
/// a GitHub remote, network) counts as not merged — we then keep following the
/// base rather than guess it's gone.
fn recorded_base_pr_merged(base: &str) -> bool {
    if !github::is_gh_available() {
        return false;
    }
    match github::get_pr_for_branch(base) {
        Ok(Some(base_pr)) => base_pr.state.is_merged(),
        Ok(None) => false,
        Err(e) => {
            output::warn(&format!("Could not check PR for base '{}': {}", base, e));
            false
        }
    }
}

/// The `--onto` boundary after a base merged: the recorded base tip if we have
/// it (survives the base branch's deletion), else the remote-tracking ref of
/// the base (cleanup keeps it alive while a child PR still targets it).
fn boundary_for_merged_base(base: &str, recorded_base_sha: Option<&str>) -> Option<String> {
    if let Some(sha) = recorded_base_sha {
        return Some(sha.to_string());
    }
    let remote_ref = format!("origin/{base}");
    if git::ref_exists(&remote_ref) {
        return Some(remote_ref);
    }
    output::warn(&format!(
        "Cannot find where '{}' was forked from (no recorded base tip, origin/{} is gone).",
        base, base
    ));
    output::hints(&[&format!(
        "git rebase --onto origin/main <last commit of {base}>  # replay only your commits"
    )]);
    None
}

/// Plan for a PR stacked on `base` (GitHub base != default branch).
fn plan_for_stacked_pr(
    base: &str,
    pr_number: u64,
    default_remote: &str,
    recorded_base_sha: Option<&str>,
) -> Result<Option<Plan>> {
    let base_pr = github::get_pr_for_branch(base)?;
    match base_pr.as_ref().map(|p| &p.state) {
        Some(PrState::Merged { .. }) => {
            let base_pr = base_pr.as_ref().expect("matched Some");
            output::success(&format!(
                "Base PR #{} ({}) is merged ✓",
                base_pr.number, base
            ));
            Ok(
                boundary_for_merged_base(base, recorded_base_sha).map(|boundary| Plan {
                    new_base: default_remote.to_string(),
                    boundary: Some(boundary),
                    retarget_pr: Some(pr_number),
                    unstack: true,
                    rerecord_base: false,
                }),
            )
        }
        Some(PrState::Closed) => {
            let base_pr = base_pr.as_ref().expect("matched Some");
            output::warn(&format!(
                "Base PR #{} ({}) was closed without merging.",
                base_pr.number, base
            ));
            output::hints(&[&format!(
                "gh pr reopen {}  # or retarget this PR with gh pr edit --base",
                base_pr.number
            )]);
            Ok(None)
        }
        Some(PrState::Open) | None => {
            // Base still in flight: follow it (pick up the parent's new commits).
            if base_pr.is_none() {
                output::warn(&format!(
                    "No PR found for base branch '{}'; following origin/{}.",
                    base, base
                ));
            }
            let base_ref = format!("origin/{base}");
            if !git::ref_exists(&base_ref) {
                output::warn(&format!("origin/{} does not exist. Nothing to sync.", base));
                return Ok(None);
            }
            Ok(Some(Plan {
                new_base: base_ref,
                boundary: recorded_base_sha.map(String::from),
                retarget_pr: None,
                unstack: false,
                rerecord_base: recorded_base_sha.is_some(),
            }))
        }
    }
}

/// Plan for a branch stacked via `gw new --stack` that has no PR yet.
fn plan_for_recorded_base(
    base: &str,
    default_remote: &str,
    recorded_base_sha: Option<&str>,
) -> Result<Option<Plan>> {
    output::info(&format!("Base: {} (stacked, PR not created yet)", base));
    if recorded_base_pr_merged(base) {
        output::success(&format!("Base '{}' merged ✓ — restacking onto main", base));
        return Ok(
            boundary_for_merged_base(base, recorded_base_sha).map(|boundary| Plan {
                new_base: default_remote.to_string(),
                boundary: Some(boundary),
                retarget_pr: None,
                unstack: true,
                rerecord_base: false,
            }),
        );
    }
    // Follow the parent: its remote ref if pushed, else the local branch.
    let remote_ref = format!("origin/{base}");
    let base_ref = if git::ref_exists(&remote_ref) {
        remote_ref
    } else if git::branch_exists(base) {
        base.to_string()
    } else {
        output::warn(&format!(
            "Base branch '{}' no longer exists locally or on origin. Nothing to sync.",
            base
        ));
        return Ok(None);
    };
    Ok(Some(Plan {
        new_base: base_ref,
        boundary: recorded_base_sha.map(String::from),
        retarget_pr: None,
        unstack: false,
        rerecord_base: recorded_base_sha.is_some(),
    }))
}

/// Rebase per `plan`, then publish (force-with-lease) if the branch is pushed.
fn execute(plan: &Plan, current: &str, default_branch: &str, verbose: bool) -> Result<()> {
    let upstream_exists = git::has_remote_tracking(current);

    println!();
    if git::is_ancestor(&plan.new_base, "HEAD") && plan.retarget_pr.is_none() {
        output::success(&format!("Already up to date with {}", plan.new_base));
        // A previous local rebase/amend may still be unpublished.
        if upstream_exists && matches!(SyncState::detect(current), Ok(SyncState::Diverged { .. })) {
            output::info("Local history was rewritten but not pushed — publishing...");
            git::force_push_with_lease(current, verbose)?;
            output::success("Force pushed");
        }
        if plan.unstack {
            git::unset_branch_base(current, verbose)?;
        }
        output::ready("Synced", current);
        return Ok(());
    }

    let behind = git::behind_base_count("HEAD", &plan.new_base);
    output::info("Syncing...");

    // Rebase first, then move the PR base, then push. Moving the base first
    // would leave GitHub showing the new base while the branch still carried
    // the old commits if the rebase then failed.
    let result = match &plan.boundary {
        Some(boundary) => {
            output::info(&format!(
                "  Rebasing commits after {} onto {} ({} new commit(s))...",
                short(boundary),
                plan.new_base,
                behind
            ));
            git::rebase_onto(&plan.new_base, boundary, verbose)
        }
        None => {
            output::info(&format!(
                "  Rebasing onto {} ({} new commit(s))...",
                plan.new_base, behind
            ));
            git::rebase(&plan.new_base, verbose)
        }
    };
    if let Err(e) = result {
        output::error("Rebase failed. You may need to resolve conflicts manually.");
        output::action("git rebase --continue  # After resolving conflicts, then: gw sync");
        output::action("git rebase --abort     # To cancel");
        return Err(e);
    }

    if let Some(pr_number) = plan.retarget_pr {
        output::info(&format!("  Updating PR base to {}...", default_branch));
        github::update_pr_base(pr_number, default_branch)?;
    }

    if upstream_exists {
        output::info("  Force pushing...");
        git::force_push_with_lease(current, verbose)?;
    }

    if plan.unstack {
        // The branch now targets the default branch, so it is no longer
        // stacked -- drop the recorded base so `gw status` stops treating it
        // as such.
        git::unset_branch_base(current, verbose)?;
    } else if plan.rerecord_base {
        // Still stacked: the parent's current tip is the next `--onto`
        // boundary.
        if let Ok(sha) = git::rev_parse(&plan.new_base) {
            git::set_branch_base_sha(current, &sha, verbose)?;
        }
    }

    println!();
    output::ready("Synced", current);
    let mut hints: Vec<String> = Vec::new();
    if let Some(pr_number) = plan.retarget_pr {
        hints.push(format!(
            "PR #{} base is now '{}'",
            pr_number, default_branch
        ));
    }
    if !upstream_exists {
        hints.push(format!(
            "git push -u origin {current}  # Publish when ready"
        ));
    }
    hints.push("gw status  # Check status".to_string());
    let hint_refs: Vec<&str> = hints.iter().map(String::as_str).collect();
    output::hints(&hint_refs);

    Ok(())
}

/// Abbreviate a full SHA for display; leave named refs alone.
fn short(reference: &str) -> &str {
    if reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit()) {
        &reference[..7]
    } else {
        reference
    }
}
