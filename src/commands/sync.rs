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
//! doubled and conflict-prone. Missing stack boundaries stop the operation.
//! Publishing precedes PR retargeting; a shared-clone journal makes interrupted
//! operations resumable without inferring a new boundary from rewritten history.
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
//!   Force pushing...
//!   Updating PR base to main...
//!   ✓ Synced
//! ```

use super::helpers;
use crate::error::{GwError, Result};
use crate::git;
use crate::github::{self, PrState};
use crate::output;
use crate::state::{RepoType, WorkingDirState};

/// How the branch should be moved onto its base.
#[derive(serde::Serialize, serde::Deserialize)]
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
pub fn run(abort: bool, keep_work: bool, finish_work: bool, verbose: bool) -> Result<()> {
    // 1. Check prerequisites
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }
    let _lifecycle_lock = git::lifecycle::LifecycleLock::acquire()?;

    if abort {
        return abort_pending_sync(verbose);
    }
    let current = git::current_branch()?;
    if let Some(pending) = read_pending_sync()? {
        return resume_sync(pending, &current, finish_work, verbose);
    }
    if finish_work {
        return Err(GwError::Other(
            "No interrupted work restoration to finish.".into(),
        ));
    }
    let working_dir = WorkingDirState::detect();
    if !working_dir.is_clean() && !keep_work {
        output::error(&format!(
            "You have uncommitted changes ({}).",
            working_dir.description()
        ));
        output::action(
            "gw sync --keep-work  # save local work, sync, then restore work and staging",
        );
        output::info("Only committed history is published; local work stays local.");
        output::action("git add <files> && git commit -m \"...\"  # or commit when ready");
        return Err(GwError::UncommittedChanges);
    }

    // 2. Get current branch info
    let repo_type = RepoType::detect()?;
    let home_branch = repo_type.home_branch();

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
        if working_dir.is_clean() {
            helpers::pull_with_output(&default_remote, default_branch, verbose)?;
            output::ready("Ready", home_branch);
            return Ok(());
        }
        return execute(
            &Plan {
                new_base: default_remote.clone(),
                boundary: None,
                retarget_pr: None,
                unstack: false,
                rerecord_base: false,
            },
            &current,
            default_branch,
            true,
            verbose,
        );
    }

    println!();
    output::info(&format!("Branch: {}", output::bold(&current)));

    // 3. Fetch latest first to get accurate PR/branch state
    output::info("Fetching from origin...");
    git::fetch_prune(verbose)?;

    // A failed GitHub query is not evidence that no PR exists. Avoid planning
    // against stale local metadata when remote dependency state is unknown.
    let pr = if github::is_gh_available() {
        github::get_pr_for_branch(&current)?
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
                match retargeted_boundary(
                    recorded_base.as_deref(),
                    recorded_base_sha.as_deref(),
                    &default_remote,
                )? {
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
                if recorded_base
                    .as_deref()
                    .is_some_and(|base| base != pr.base_branch)
                {
                    return Err(GwError::Other("GitHub PR base differs from the recorded parent. Refusing to use another parent's fork point.".into()));
                }
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
    execute(&plan, &current, default_branch, false, verbose)
}

/// For a PR that GitHub shows targeting the default branch: if `gw new --stack`
/// recorded a base whose PR has since merged, the branch was retargeted by
/// GitHub and still contains the base's commits. Return the `--onto` boundary
/// to replay only this branch's own commits; `None` for an ordinary branch.
fn retargeted_boundary(
    recorded_base: Option<&str>,
    recorded_base_sha: Option<&str>,
    default_remote: &str,
) -> Result<Option<String>> {
    let Some(base) = recorded_base else {
        return Ok(None);
    };
    if !recorded_base_pr_merged(base, default_remote)? {
        return Err(GwError::Other("PR targets the default branch but its recorded parent is not confirmed integrated. Refusing a plain rebase.".into()));
    }
    output::info(&format!(
        "Recorded base '{}' merged and the PR now targets the default branch — restacking",
        base
    ));
    Ok(Some(required_boundary(base, recorded_base_sha)?))
}

/// A recorded parent may only be dropped with positive integration evidence.
/// Query failures propagate instead of being mistaken for an open parent.
fn recorded_base_pr_merged(base: &str, default_remote: &str) -> Result<bool> {
    if !github::is_gh_available() {
        return Ok(false);
    }
    match github::get_pr_for_branch(base)? {
        Some(pr) if pr.state.is_merged() => {
            ensure_parent_integrated(&pr, default_remote)?;
            Ok(true)
        }
        Some(pr) if pr.state.is_closed() => Err(GwError::Other(format!(
            "Parent '{}' was closed without merging; resolve its disposition before syncing.",
            base
        ))),
        _ => Ok(false),
    }
}

/// A merged PR may have landed on another feature branch. Only a positively
/// confirmed integration into the fetched default branch permits dropping it.
fn ensure_parent_integrated(pr: &github::PrInfo, default_remote: &str) -> Result<()> {
    let default_branch = default_remote
        .strip_prefix("origin/")
        .unwrap_or(default_remote);
    if pr.base_branch != default_branch {
        return Err(GwError::Other(format!(
            "Parent PR #{} merged into '{}', not '{}'. Refusing to drop its changes; integrate the parent stack into the default branch and reconcile its base first.",
            pr.number, pr.base_branch, default_branch
        )));
    }
    let integrated = match &pr.state {
        PrState::Merged {
            merge_commit: Some(sha),
            ..
        } => git::is_ancestor(sha, default_remote),
        _ => false,
    };
    if !integrated {
        return Err(GwError::Other(format!(
            "Cannot confirm parent PR #{} is integrated in {}. Fetch/retry after integration; refusing to discard its commits.",
            pr.number, default_remote
        )));
    }
    Ok(())
}

fn required_boundary(base: &str, recorded_base_sha: Option<&str>) -> Result<String> {
    recorded_base_sha.map(String::from).ok_or_else(|| GwError::Other(format!(
        "No valid recorded fork point for '{}'. Refusing to infer it from a moving branch tip; recover the stack metadata before syncing.", base
    )))
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
            ensure_parent_integrated(base_pr, default_remote)?;
            Ok(Some(Plan {
                new_base: default_remote.to_string(),
                boundary: Some(required_boundary(base, recorded_base_sha)?),
                retarget_pr: Some(pr_number),
                unstack: true,
                rerecord_base: false,
            }))
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
                boundary: Some(required_boundary(base, recorded_base_sha)?),
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
    if recorded_base_pr_merged(base, default_remote)? {
        output::success(&format!("Base '{}' merged ✓ — restacking onto main", base));
        return Ok(Some(Plan {
            new_base: default_remote.to_string(),
            boundary: Some(required_boundary(base, recorded_base_sha)?),
            retarget_pr: None,
            unstack: true,
            rerecord_base: false,
        }));
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
        boundary: Some(required_boundary(base, recorded_base_sha)?),
        retarget_pr: None,
        unstack: false,
        rerecord_base: recorded_base_sha.is_some(),
    }))
}

/// Persist the operation before rewriting history. One journal per shared
/// clone deliberately prevents cleanup/new/another sync until recovery finishes.
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingSync {
    branch: String,
    worktree: std::path::PathBuf,
    original_head: String,
    rebased_head: Option<String>,
    target_sha: String,
    default_branch: String,
    publish: bool,
    expected_remote: Option<String>,
    publication_finished: bool,
    plan: Plan,
    #[serde(default)]
    home: bool,
    #[serde(default)]
    aborting: bool,
    #[serde(default)]
    work: Option<SavedWork>,
}

/// The journal precedes stash creation; the unique message recovers a snapshot
/// even if interrupted before its OID is recorded. A private ref pins it across
/// worktrees and stash-list edits. Restoring is never automatically repeated.
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedWork {
    token: String,
    oid: Option<String>,
    #[serde(default)]
    restoring: bool,
    #[serde(default)]
    restored: bool,
}

fn read_pending_sync() -> Result<Option<PendingSync>> {
    let path = git::lifecycle::sync_journal_path()?;
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| {
            GwError::Other(format!("Cannot read sync journal {}: {e}", path.display()))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn save_pending_sync(pending: &PendingSync) -> Result<()> {
    use std::io::Write;
    let path = git::lifecycle::sync_journal_path()?;
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(pending).map_err(|e| GwError::Other(e.to_string()))?;
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

fn execute(
    plan: &Plan,
    current: &str,
    default_branch: &str,
    home: bool,
    verbose: bool,
) -> Result<()> {
    // Resolve mutable refs once so the rebase and recorded boundary agree even
    // if another tool fetches while this operation runs.
    if git::is_ancestor(&plan.new_base, "HEAD") {
        output::success(&format!("Already up to date with {}", plan.new_base));
    } else {
        output::info(&format!("Rebasing onto {}", plan.new_base));
    }
    let expected_remote = git::remote_branch_tip(current)?;
    let head = git::head_commit()?;
    let publish = !home && (git::has_remote_tracking(current) || expected_remote.is_some());
    // A true no-op must not disturb partial staging or touch local files.
    if git::is_ancestor(&plan.new_base, "HEAD")
        && !plan.unstack
        && !plan.rerecord_base
        && plan.retarget_pr.is_none()
        && (!publish || expected_remote.as_ref() == Some(&head))
    {
        output::ready("Synced", current);
        return Ok(());
    }
    let pending = PendingSync {
        branch: current.to_string(),
        worktree: git::worktree_root()?,
        original_head: head,
        rebased_head: None,
        target_sha: git::rev_parse(&plan.new_base)?,
        default_branch: default_branch.to_string(),
        publish,
        expected_remote,
        publication_finished: false,
        home,
        aborting: false,
        work: if WorkingDirState::detect().is_clean() {
            None
        } else {
            Some(SavedWork {
                token: format!(
                    "gw-sync-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| GwError::Other(e.to_string()))?
                        .as_nanos()
                ),
                oid: None,
                restoring: false,
                restored: false,
            })
        },
        plan: Plan {
            new_base: plan.new_base.clone(),
            boundary: plan.boundary.clone(),
            retarget_pr: plan.retarget_pr,
            unstack: plan.unstack,
            rerecord_base: plan.rerecord_base,
        },
    };
    save_pending_sync(&pending)?;
    resume_sync(pending, current, false, verbose)
}

fn resume_sync(
    mut pending: PendingSync,
    current: &str,
    finish_work: bool,
    verbose: bool,
) -> Result<()> {
    if pending.branch != current || pending.worktree != git::worktree_root()? {
        return Err(GwError::Other(format!(
            "Unfinished sync belongs to '{}' in {}. Finish/abort any rebase there, then rerun gw sync in that worktree.",
            pending.branch,
            pending.worktree.display()
        )));
    }
    let git_dir = git::git_dir()?;
    if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
        return Err(GwError::Other("A rebase is still in progress. Resolve it with git rebase --continue (or --abort), then rerun gw sync.".into()));
    }
    if finish_work && !pending.work.as_ref().is_some_and(|work| work.restoring) {
        return Err(GwError::Other(
            "Work restoration has not started; rerun gw sync without --finish-work.".into(),
        ));
    }
    prepare_work(&mut pending, verbose)?;
    if !pending.work.as_ref().is_some_and(|work| work.restoring)
        && !WorkingDirState::detect().is_clean()
    {
        return Err(GwError::Other("New uncommitted changes appeared during sync. Preserve them before resuming; the original saved work is still protected.".into()));
    }
    if pending.aborting {
        if git::head_commit()? != pending.original_head {
            return Err(GwError::Other(
                "Interrupted rollback: run gw sync --abort to restore the original head first."
                    .into(),
            ));
        }
        restore_work(&mut pending, finish_work, verbose)?;
        std::fs::remove_file(git::lifecycle::sync_journal_path()?)?;
        output::success("Sync aborted; local work restored.");
        return Ok(());
    }
    let head = git::head_commit()?;
    if let Some(expected) = &pending.rebased_head {
        if &head != expected {
            return Err(GwError::Other("HEAD changed after the pending sync's rebase. Restore the recorded rebased head before retrying; refusing to publish unrelated commits.".into()));
        }
    } else {
        if head == pending.original_head {
            if !git::is_ancestor(&pending.target_sha, "HEAD") {
                let result = if pending.home {
                    git::git_run_in_dir(".", &["merge", "--ff-only", &pending.target_sha], verbose)
                } else {
                    match &pending.plan.boundary {
                        Some(boundary) => git::rebase_onto(&pending.target_sha, boundary, verbose),
                        None => git::git_run_in_dir(
                            ".",
                            &["rebase", "--no-autostash", &pending.target_sha],
                            verbose,
                        ),
                    }
                };
                if let Err(e) = result {
                    output::warn(
                        "Sync is recorded for recovery. Resolve/continue or abort the rebase, then rerun gw sync.",
                    );
                    return Err(e);
                }
            }
        } else if !git::is_ancestor(&pending.target_sha, "HEAD") {
            return Err(GwError::Other("Pending rebase has not reached its recorded target. Resolve/abort it before rerunning gw sync.".into()));
        }
        pending.rebased_head = Some(git::head_commit()?);
        save_pending_sync(&pending)?;
    }

    // Publish before retargeting: GitHub's old dependency edge protects the
    // parent until the rebased child has actually reached the remote.
    if pending.publish {
        let remote_tip = git::remote_branch_tip(current)?;
        if remote_tip != pending.rebased_head {
            if remote_tip != pending.expected_remote {
                return Err(GwError::Other("Remote changed during pending sync. Refusing to overwrite it; preserve the journal and reconcile the remote change first.".into()));
            }
            output::info("Force pushing...");
            git::push_with_expected_tip(
                current,
                pending.expected_remote.as_deref().unwrap_or(""),
                verbose,
            )?;
            output::success("Force pushed");
        }
    }
    pending.publication_finished = true;
    save_pending_sync(&pending)?;
    if let Some(pr_number) = pending.plan.retarget_pr {
        github::update_pr_base(pr_number, &pending.default_branch)?;
    }
    if pending.plan.unstack {
        git::unset_branch_base(current, verbose)?;
    } else if pending.plan.rerecord_base {
        git::set_branch_base_sha(current, &pending.target_sha, verbose)?;
    }
    restore_work(&mut pending, finish_work, verbose)?;
    std::fs::remove_file(git::lifecycle::sync_journal_path()?)?;
    output::ready("Synced", current);
    if !pending.publish && !pending.home {
        output::hints(&[&format!(
            "git push -u origin {current}  # Publish when ready"
        )]);
    }
    output::hints(&["gw status  # Check status"]);
    Ok(())
}

/// Status must route recovery before deriving a fresh action from partial state.
pub fn pending_recovery_hint() -> Result<Option<String>> {
    Ok(read_pending_sync()?.map(|pending| {
        if let Some(work) = &pending.work {
            if work.restoring && !work.restored {
                return format!("Sync on '{}' in {} reached work restoration. Saved work: refs/gw/sync-work/{}. Resolve/restore the files, then run gw sync --finish-work there. This is not an active rebase.", pending.branch, pending.worktree.display(), work.token);
            }
        }
        format!(
        "Unfinished sync on '{}' in {}. Finish/abort any active rebase there, then run gw sync to resume (or gw sync --abort before publication).",
        pending.branch, pending.worktree.display()
    )}))
}

fn abort_pending_sync(verbose: bool) -> Result<()> {
    let Some(mut pending) = read_pending_sync()? else {
        output::info("No pending sync to abort.");
        return Ok(());
    };
    if pending.worktree != git::worktree_root()? {
        return Err(GwError::Other(format!(
            "Abort sync in its original worktree: {}",
            pending.worktree.display()
        )));
    }
    let dir = git::git_dir()?;
    if dir.join("rebase-merge").exists() || dir.join("rebase-apply").exists() {
        return Err(GwError::Other(
            "Run git rebase --abort first, then gw sync --abort.".into(),
        ));
    }
    if git::current_branch()? != pending.branch || !WorkingDirState::detect().is_clean() {
        return Err(GwError::Other(
            "Return to the pending branch with a clean working tree before aborting sync.".into(),
        ));
    }
    if pending.publication_finished
        || git::remote_branch_tip(&pending.branch)? != pending.expected_remote
    {
        return Err(GwError::Other("Sync was published or the remote changed. Refusing rollback; resume gw sync after reconciling the remote.".into()));
    }
    let head = git::head_commit()?;
    if head != pending.original_head {
        if pending.rebased_head.as_ref() != Some(&head) {
            return Err(GwError::Other("HEAD differs from the recorded sync checkpoints. Refusing rollback of unrelated work.".into()));
        }
        pending.aborting = true;
        save_pending_sync(&pending)?;
        git::git_run_in_dir(".", &["reset", "--keep", &pending.original_head], verbose)?;
    }
    pending.aborting = true;
    save_pending_sync(&pending)?;
    prepare_work(&mut pending, verbose)?;
    restore_work(&mut pending, false, verbose)?;
    std::fs::remove_file(git::lifecycle::sync_journal_path()?)?;
    output::success("Sync aborted; original head and dependency metadata preserved.");
    Ok(())
}

/// Run snapshot commands with checked status. Do not infer successful reads from
/// empty output (a failed stash query must never mean "no snapshot exists").
fn work_git(args: &[&str]) -> Result<String> {
    let result = std::process::Command::new("git")
        .args(args)
        .current_dir(git::worktree_root()?)
        .output()?;
    if !result.status.success() {
        return Err(GwError::GitCommandFailed(
            String::from_utf8_lossy(&result.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_string())
}

fn prepare_work(pending: &mut PendingSync, verbose: bool) -> Result<()> {
    let Some(work) = &pending.work else {
        return Ok(());
    };
    // Restored snapshots may already have had their private ref removed.
    if work.restored {
        return Ok(());
    }
    let token = work.token.clone();
    let reference = format!("refs/gw/sync-work/{token}");
    if work.oid.is_none() {
        let find_snapshot = || -> Result<Option<String>> {
            Ok(work_git(&["stash", "list", "--format=%H%x09%gs"])?
                .lines()
                .find_map(|line| {
                    let (oid, subject) = line.split_once('\t')?;
                    subject
                        .ends_with(&format!(": {token}"))
                        .then(|| oid.to_string())
                }))
        };
        let oid = match find_snapshot()? {
            Some(oid) => oid,
            None => {
                if git::head_commit()? != pending.original_head
                    || WorkingDirState::detect().is_clean()
                {
                    return Err(GwError::Other(format!(
                        "Cannot locate saved work {token}; refusing to infer or overwrite it. Inspect git stash list and gw status."
                    )));
                }
                output::info(
                    "Saving staged, unstaged, and untracked work locally (ignored files excluded)...",
                );
                git::git_run_in_dir(
                    ".",
                    &["stash", "push", "--include-untracked", "--message", &token],
                    verbose,
                )?;
                find_snapshot()?.ok_or_else(|| GwError::Other("Snapshot creation returned without a recoverable stash entry. Stop and inspect git stash list.".into()))?
            }
        };
        // Keep the stash entry until both the private ref and journal are durable.
        git::git_run_in_dir(".", &["update-ref", &reference, &oid], verbose)?;
        pending.work.as_mut().unwrap().oid = Some(oid);
        save_pending_sync(pending)?;
    }
    let oid = pending.work.as_ref().unwrap().oid.as_ref().unwrap();
    if work_git(&["rev-parse", "--verify", &reference])? != *oid {
        return Err(GwError::Other(
            "Saved work reference changed; refusing to restore a different snapshot.".into(),
        ));
    }
    // Only remove our own entry, by OID and unique message, never stash@{0}.
    let entries = work_git(&["stash", "list", "--format=%gd%x09%H%x09%gs"])?;
    for line in entries.lines() {
        let parts: Vec<_> = line.splitn(3, '\t').collect();
        if parts.len() == 3 && parts[1] == oid && parts[2].ends_with(&format!(": {token}")) {
            git::git_run_in_dir(".", &["stash", "drop", parts[0]], verbose)?;
            break;
        }
    }
    Ok(())
}

fn restore_work(pending: &mut PendingSync, finish_work: bool, verbose: bool) -> Result<()> {
    let Some(work) = &pending.work else {
        return Ok(());
    };
    let reference = format!("refs/gw/sync-work/{}", work.token);
    if !work.restored {
        if work.restoring {
            if !finish_work {
                return Err(GwError::Other(format!(
                    "Sync reached local work restoration. Backup: {reference}. Resolve/restore your files, then run gw sync --finish-work. Do not run git rebase --continue for a restoration conflict."
                )));
            }
            if !work_git(&["ls-files", "--unmerged"])?.is_empty() {
                return Err(GwError::Other("Unresolved files remain. Resolve them and git add <files>, then run gw sync --finish-work.".into()));
            }
        } else {
            pending.work.as_mut().unwrap().restoring = true;
            save_pending_sync(pending)?;
            output::info("Restoring local work and staging...");
            if let Err(error) =
                git::git_run_in_dir(".", &["stash", "apply", "--index", &reference], verbose)
            {
                return Err(GwError::Other(format!(
                    "Committed-history operation finished, but local work restoration failed: {error}\nBackup retained: {reference}\nInspect with git stash show -p --include-untracked {reference}. Resolve/restore the files, then run gw sync --finish-work. This is not a rebase conflict."
                )));
            }
        }
        pending.work.as_mut().unwrap().restored = true;
        save_pending_sync(pending)?;
    }
    // A crash here is harmless: a restored journal skips application on retry.
    let remaining = work_git(&["for-each-ref", "--format=%(objectname)", &reference])?;
    if !remaining.is_empty() {
        git::git_run_in_dir(
            ".",
            &[
                "update-ref",
                "-d",
                &reference,
                pending.work.as_ref().unwrap().oid.as_deref().unwrap(),
            ],
            verbose,
        )?;
    }
    output::success(if finish_work {
        "Manual work restoration acknowledged"
    } else {
        "Local work and staging restored"
    });
    Ok(())
}
