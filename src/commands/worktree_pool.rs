//! `gw worktree pool` commands — Per-leader worktree pool management
//!
//! Each leader worktree owns its own pool under `.worktrees/`.
//! Pool state is derived from the filesystem, not from an inventory file.
//! Marker files in the per-worktree git dir track acquisition state.

use std::path::{Path, PathBuf};

use crate::error::{GwError, Result};
use crate::git;
use crate::output;
use crate::pool::{PoolEntry, PoolLock, PoolNextAction, PoolState, WorktreeStatus};

/// Directory name under the per-worktree git dir for pool metadata
const POOL_META_DIR: &str = "pool";

/// Directory name under worktree root for pool worktrees
const POOL_WORKTREES_DIR: &str = ".worktrees";

/// Setup hook path relative to repo root
const SETUP_HOOK: &str = ".gw/setup";

/// Subdirectory for acquire markers
const ACQUIRED_DIR: &str = "acquired";

/// Canonicalize a path, stripping the `\\?\` prefix on Windows so that
/// external tools (like git) can consume the path without issues.
fn canonicalize_clean(path: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(target_os = "windows")]
    {
        let s = canonical.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
    }
    canonical
}

/// Get the current worktree root (works from main repo or any worktree)
fn leader_root() -> Result<PathBuf> {
    let root = git::worktree_root()?;
    Ok(canonicalize_clean(&root))
}

/// Get the leader name (the worktree directory name, e.g., "web-2").
/// Sanitizes the name to be valid as part of a git branch name.
fn leader_name() -> Result<String> {
    let root = leader_root()?;
    let raw = root
        .file_name()
        .and_then(|s| s.to_str())
        .map(String::from)
        .ok_or_else(|| GwError::Other("Could not determine leader name".to_string()))?;
    // Strip leading dots (invalid in git branch names)
    let sanitized = raw.trim_start_matches('.');
    if sanitized.is_empty() {
        return Err(GwError::Other(format!(
            "Leader directory name is not valid for branch naming: {raw}"
        )));
    }
    Ok(sanitized.to_string())
}

/// Pool entry name prefix for the current leader (e.g., "web-2-pool-")
fn pool_prefix() -> Result<String> {
    Ok(format!("{}-pool-", leader_name()?))
}

/// Get the main repository root (parent of .git), even from inside a worktree.
fn main_repo_root() -> Result<PathBuf> {
    let common = git::git_common_dir()?;
    let common = canonicalize_clean(&common);
    common
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| GwError::Other("Could not determine main repository root".to_string()))
}

/// Resolve the pool metadata directory (per-worktree: {git_dir}/pool/)
fn pool_dir() -> Result<PathBuf> {
    let git_dir = git::git_dir()?;
    let git_dir = canonicalize_clean(&git_dir);
    Ok(git_dir.join(POOL_META_DIR))
}

/// Resolve the acquired markers directory
fn acquired_dir() -> Result<PathBuf> {
    Ok(pool_dir()?.join(ACQUIRED_DIR))
}

/// Resolve the worktrees directory ({leader_root}/.worktrees/)
fn worktrees_dir() -> Result<PathBuf> {
    let root = leader_root()?;
    Ok(root.join(POOL_WORKTREES_DIR))
}

/// Run the setup hook if it exists
fn run_setup_hook(repo_root: &Path, worktree_path: &str, verbose: bool) -> Result<()> {
    let hook = repo_root.join(SETUP_HOOK);
    if !hook.exists() {
        return Ok(());
    }

    if verbose {
        output::action(&format!("Running setup hook: {}", hook.display()));
    }

    let status = std::process::Command::new(&hook)
        .arg(worktree_path)
        .current_dir(worktree_path)
        .status()?;

    if !status.success() {
        return Err(GwError::Other(format!(
            "Setup hook failed with exit code: {}",
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

/// Run a git command inside `path`, returning trimmed stdout on success.
fn git_capture(path: &Path, args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Get the current branch of a worktree by running git in that directory
fn worktree_current_branch(path: &Path) -> String {
    git_capture(path, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "???".to_string())
}

/// Detect an in-progress git operation (rebase/merge/cherry-pick/...) in a
/// worktree by probing the well-known state files in its git dir.
fn in_progress_operation(path: &Path) -> Option<String> {
    // (git-path, human label)
    const PROBES: &[(&str, &str)] = &[
        ("rebase-merge", "rebase"),
        ("rebase-apply", "rebase"),
        ("MERGE_HEAD", "merge"),
        ("CHERRY_PICK_HEAD", "cherry-pick"),
        ("REVERT_HEAD", "revert"),
        ("BISECT_LOG", "bisect"),
    ];
    for (git_path, label) in PROBES {
        if let Some(resolved) = git_capture(path, &["rev-parse", "--git-path", git_path]) {
            let candidate = PathBuf::from(&resolved);
            let full = if candidate.is_absolute() {
                candidate
            } else {
                path.join(candidate)
            };
            if full.exists() {
                return Some((*label).to_string());
            }
        }
    }
    None
}

/// Inspect a pool worktree and return the reasons it is NOT in a clean,
/// returnable state. An empty vec means the worktree is clean:
///
/// - checked out on its pool home branch (== entry name),
/// - no staged/unstaged/untracked changes,
/// - no in-progress git operation (rebase/merge/cherry-pick/...).
///
/// Sync state is intentionally not checked: being behind `origin` is fine
/// because `acquire` fast-forwards the worktree before handing it out.
fn worktree_issues(entry: &PoolEntry) -> Vec<String> {
    let path = entry.path.as_path();
    let mut issues = Vec::new();

    if !path.exists() {
        issues.push("worktree directory is missing".to_string());
        return issues;
    }

    let branch = worktree_current_branch(path);
    if branch != entry.branch {
        issues.push(format!(
            "on branch '{}', expected pool home branch '{}'",
            branch, entry.branch
        ));
    }

    match git_capture(path, &["status", "--porcelain"]) {
        Some(s) if !s.trim().is_empty() => {
            issues.push("has uncommitted or untracked changes".to_string());
        }
        None => issues.push("could not read working tree status".to_string()),
        _ => {}
    }

    if let Some(op) = in_progress_operation(path) {
        issues.push(format!("a {op} is in progress"));
    }

    issues
}

/// Ensure `.worktrees/` is excluded via `.git/info/exclude`.
/// This is local-only and never pollutes `.gitignore` or the working tree.
fn ensure_excluded() -> Result<()> {
    let common = git::git_common_dir()?;
    let exclude_path = common.join("info").join("exclude");
    let entry = ".worktrees/";

    if exclude_path.exists() {
        let content = std::fs::read_to_string(&exclude_path)?;
        if content.lines().any(|line| line.trim() == entry) {
            return Ok(());
        }
        let prefix = if content.ends_with('\n') { "" } else { "\n" };
        std::fs::write(&exclude_path, format!("{content}{prefix}{entry}\n"))?;
    } else {
        std::fs::create_dir_all(common.join("info"))?;
        std::fs::write(&exclude_path, format!("{entry}\n"))?;
    }

    Ok(())
}

/// Release a single pool worktree back to the pool.
///
/// Just removes the acquire marker. The worktree should have been
/// cleaned up (gw cleanup) before release.
fn release_one(entry: &PoolEntry, acquired_dir: &Path) -> Result<()> {
    let marker = acquired_dir.join(&entry.name);
    if marker.exists() {
        std::fs::remove_file(&marker)?;
    }
    output::success(&format!("{} released", entry.name));
    Ok(())
}

// --- Pool commands ---

/// `gw worktree pool warm <n>`
pub fn warm(count: usize, verbose: bool) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let pool_dir = pool_dir()?;
    let wt_dir = worktrees_dir()?;
    let acquired_dir = acquired_dir()?;
    let repo_root = main_repo_root()?;
    let prefix = pool_prefix()?;

    println!();
    output::info(&format!(
        "Warming worktree pool to {} available",
        output::bold(&count.to_string())
    ));

    // Acquire lock and scan filesystem
    let _lock = PoolLock::acquire(&pool_dir)?;
    let mut state = PoolState::scan(&wt_dir, &acquired_dir, &prefix)?;

    let available = state.count_by_status(&WorktreeStatus::Available);
    let acquired = state.count_by_status(&WorktreeStatus::Acquired);
    let total = state.entries.len();
    if available >= count {
        output::success(&format!(
            "Pool already has {available} available ({acquired} acquired, {total} total), nothing to do"
        ));
        return Ok(());
    }

    let to_create = count - available;

    // Ensure .worktrees/ is excluded locally (via .git/info/exclude)
    ensure_excluded()?;

    // Fetch once
    output::info("Fetching from origin...");
    git::fetch_prune(verbose)?;
    output::success("Fetched");

    let default_remote = git::get_default_remote_branch()?;

    // Create worktrees dir
    std::fs::create_dir_all(&wt_dir)?;

    let mut created = 0;
    for i in 0..to_create {
        let name = state.next_name(&prefix);
        let abs_path = canonicalize_clean(&wt_dir).join(&name);
        let abs_path_str = abs_path.to_string_lossy().to_string();
        // Branch name = directory name (gw convention: dir name = home branch)
        let branch = name.clone();

        output::info(&format!(
            "[{}/{}] Creating {}...",
            i + 1,
            to_create,
            output::bold(&name)
        ));

        // Create the worktree
        if let Err(e) = git::worktree_add(&abs_path_str, &branch, &default_remote, verbose) {
            output::warn(&format!("Failed to create {name}: {e}"));
            continue;
        }

        // Run setup hook
        if let Err(e) = run_setup_hook(&repo_root, &abs_path_str, verbose) {
            output::warn(&format!(
                "Setup hook failed for {name}: {e}. Removing worktree."
            ));
            let _ = git::worktree_remove(&abs_path_str, verbose);
            let _ = git::force_delete_branch(&branch, verbose);
            continue;
        }

        // Track in-memory for next_name() to work correctly
        state.entries.push(PoolEntry {
            name: name.clone(),
            path: abs_path,
            branch,
            status: WorktreeStatus::Available,
            owner: None,
        });
        created += 1;

        output::success(&format!("[{}/{}] Created {}", i + 1, to_create, name));
    }

    // Re-scan for accurate final counts
    let final_state = PoolState::scan(&wt_dir, &acquired_dir, &prefix)?;
    let total = final_state.entries.len();
    let available = final_state.count_by_status(&WorktreeStatus::Available);

    println!();
    output::success(&format!(
        "Pool warmed: {created} created, {available} available, {total} total"
    ));

    Ok(())
}

/// `gw worktree pool acquire`
pub fn acquire(verbose: bool) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let pool_dir = pool_dir()?;
    let wt_dir = worktrees_dir()?;
    let acquired_dir = acquired_dir()?;
    let prefix = pool_prefix()?;

    if !wt_dir.exists() {
        return Err(GwError::PoolNotInitialized);
    }

    let _lock = PoolLock::acquire(&pool_dir)?;

    // Ensure acquired dir exists
    std::fs::create_dir_all(&acquired_dir)?;

    let state = PoolState::scan(&wt_dir, &acquired_dir, &prefix)?;

    if state.entries.is_empty() {
        return Err(GwError::PoolNotInitialized);
    }

    // Inspect each available worktree before handing one out. A dirty available
    // worktree (e.g. left behind by an agent that crashed without a clean
    // release) must not be loaned to the next agent — skip it with a warning.
    // This is the CLI-side last line of defense; release is the first.
    let available: Vec<&PoolEntry> = state
        .entries
        .iter()
        .filter(|e| e.status == WorktreeStatus::Available)
        .collect();
    if available.is_empty() {
        return Err(GwError::PoolExhausted);
    }
    let mut entry = None;
    for candidate in &available {
        let issues = worktree_issues(candidate);
        if issues.is_empty() {
            entry = Some(*candidate);
            break;
        }
        // Warnings go to stderr so stdout stays "path only".
        eprintln!(
            "\x1b[0;33m\u{26a0}\x1b[0m Skipping unclean worktree {}: {}",
            candidate.name,
            issues.join("; ")
        );
    }
    let entry = entry.ok_or(GwError::PoolNoCleanWorktree)?;

    // Create marker file with leader name as owner
    let owner = leader_name()?;
    std::fs::write(acquired_dir.join(&entry.name), &owner)?;

    // Sync worktree to latest (gw home equivalent)
    let wt_path = entry.path.to_string_lossy().to_string();
    git::git_run_in_dir(&wt_path, &["fetch", "--prune"], verbose)?;
    let default_remote = git::get_default_remote_branch()?;
    let default_branch = default_remote.strip_prefix("origin/").unwrap_or("main");
    git::git_run_in_dir(&wt_path, &["pull", "origin", default_branch], verbose)?;

    let path = entry.path.to_string_lossy().to_string();
    let name = entry.name.clone();

    let remaining = state.count_by_status(&WorktreeStatus::Available) - 1;
    eprintln!(
        "\x1b[0;32m\u{2713}\x1b[0m Acquired {} ({} remaining)",
        name, remaining,
    );

    // Print ONLY the path to stdout for `path=$(gw worktree pool acquire)`
    println!("{path}");

    Ok(())
}

/// `gw worktree pool release [name]`
///
/// Removes acquire markers. No git operations — cleanup should have
/// been run inside the worktree before releasing.
pub fn release(name: Option<String>, _verbose: bool) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let pool_dir = pool_dir()?;
    let wt_dir = worktrees_dir()?;
    let acquired_dir = acquired_dir()?;
    let prefix = pool_prefix()?;

    if !wt_dir.exists() {
        return Err(GwError::PoolNotInitialized);
    }

    let _lock = PoolLock::acquire(&pool_dir)?;
    let state = PoolState::scan(&wt_dir, &acquired_dir, &prefix)?;

    if state.entries.is_empty() {
        return Err(GwError::PoolNotInitialized);
    }

    match name {
        Some(ref n) => {
            let entry = state
                .find_by_name_or_path(n)
                .ok_or_else(|| GwError::PoolWorktreeNotFound(n.clone()))?;

            if entry.status != WorktreeStatus::Acquired {
                return Err(GwError::PoolWorktreeNotAcquired(entry.name.clone()));
            }

            // Inspect before returning to the pool: an explicitly-named dirty
            // worktree is a hard error so the caller notices and fixes it.
            let issues = worktree_issues(entry);
            if !issues.is_empty() {
                return Err(GwError::PoolWorktreeDirty {
                    name: entry.name.clone(),
                    reason: issues.join("; "),
                });
            }

            release_one(entry, &acquired_dir)?;
        }
        None => {
            let acquired: Vec<_> = state
                .entries
                .iter()
                .filter(|e| e.status == WorktreeStatus::Acquired)
                .collect();

            if acquired.is_empty() {
                return Err(GwError::PoolNoneAcquired);
            }

            // Release every clean worktree; keep the dirty ones acquired (so
            // their work can be inspected) and report them at the end.
            let mut skipped = Vec::new();
            for entry in &acquired {
                let issues = worktree_issues(entry);
                if issues.is_empty() {
                    release_one(entry, &acquired_dir)?;
                } else {
                    output::warn(&format!("Kept {}: {}", entry.name, issues.join("; ")));
                    skipped.push(entry.name.clone());
                }
            }

            if !skipped.is_empty() {
                return Err(GwError::Other(format!(
                    "Kept {} unclean worktree(s) acquired: {}. Run `gw cleanup` inside each \
                     (or fix it), then `gw worktree pool release <name>`.",
                    skipped.len(),
                    skipped.join(", ")
                )));
            }
        }
    }

    // Re-scan for final counts
    let final_state = PoolState::scan(&wt_dir, &acquired_dir, &prefix)?;
    let available = final_state.count_by_status(&WorktreeStatus::Available);
    let acquired_count = final_state.count_by_status(&WorktreeStatus::Acquired);
    let total = final_state.entries.len();

    println!();
    output::success(&format!(
        "Pool: {} available, {} acquired, {} total",
        available, acquired_count, total
    ));

    Ok(())
}

/// `gw worktree pool status`
pub fn status(verbose: bool) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let wt_dir = worktrees_dir()?;
    let acquired_dir = acquired_dir()?;
    let prefix = pool_prefix()?;

    if !wt_dir.exists() {
        return Err(GwError::PoolNotInitialized);
    }

    // Read-only — no lock needed
    let state = PoolState::scan(&wt_dir, &acquired_dir, &prefix)?;

    if state.entries.is_empty() {
        return Err(GwError::PoolNotInitialized);
    }

    let available = state.count_by_status(&WorktreeStatus::Available);
    let acquired = state.count_by_status(&WorktreeStatus::Acquired);
    let total = state.entries.len();

    println!();
    output::info(&format!(
        "Pool: {} available, {} acquired, {} total",
        output::bold(&available.to_string()),
        output::bold(&acquired.to_string()),
        output::bold(&total.to_string()),
    ));

    if acquired > 0 {
        println!();
        let header = format!("{:<24} {}", "NAME", "BRANCH");
        println!("{header}");
        println!("{}", "-".repeat(48));

        for entry in &state.entries {
            if entry.status != WorktreeStatus::Acquired {
                continue;
            }
            let branch = worktree_current_branch(&entry.path);
            let branch_display = if branch == entry.name {
                "(idle)".to_string()
            } else {
                branch
            };
            println!("{:<24} {}", entry.name, branch_display);
            println!("    {}", entry.path.display());
        }
    }

    if verbose {
        // --verbose: show all entries
        println!();
        output::info("All entries:");
        println!();
        let header = format!("{:<24} {:<12} {:<24}", "NAME", "STATUS", "BRANCH");
        println!("{header}");
        println!("{}", "-".repeat(60));

        for entry in &state.entries {
            let branch = if entry.status == WorktreeStatus::Acquired {
                worktree_current_branch(&entry.path)
            } else {
                entry.branch.clone()
            };
            println!("{:<24} {:<12} {}", entry.name, entry.status, branch);
        }
    }

    // Show next action
    let next = state.next_action();
    println!();
    display_pool_next_action(&next);

    println!();
    Ok(())
}

fn display_pool_next_action(action: &PoolNextAction) {
    match action {
        PoolNextAction::WarmPool => {
            output::action("Next: warm the pool");
            println!("  gw worktree pool warm <count>");
        }
        PoolNextAction::Ready { available } => {
            output::action(&format!("Ready: {} worktree(s) available", available));
            println!("  gw worktree pool acquire");
            println!("  gw worktree pool release [name]");
        }
        PoolNextAction::Exhausted { acquired } => {
            output::action(&format!(
                "All {} worktree(s) acquired. Release or warm more.",
                acquired
            ));
            println!("  gw worktree pool release [name]");
            println!("  gw worktree pool warm <count>");
        }
        PoolNextAction::AllIdle { available } => {
            output::action(&format!(
                "All {} worktree(s) idle. Acquire or drain.",
                available
            ));
            println!("  gw worktree pool acquire");
            println!("  gw worktree pool drain");
        }
    }
}

/// `gw worktree pool drain [--force]`
pub fn drain(force: bool, verbose: bool) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let pool_dir = pool_dir()?;
    let wt_dir = worktrees_dir()?;
    let acquired_dir = acquired_dir()?;
    let prefix = pool_prefix()?;
    // Resolve all paths upfront — the cwd might be inside a pool worktree
    // that we're about to delete.
    let leader = leader_root()?;
    let leader_str = leader.to_string_lossy().to_string();

    if !wt_dir.exists() {
        return Err(GwError::PoolNotInitialized);
    }

    println!();
    output::info("Draining worktree pool...");

    let _lock = PoolLock::acquire(&pool_dir)?;
    let state = PoolState::scan(&wt_dir, &acquired_dir, &prefix)?;

    if state.entries.is_empty() {
        return Err(GwError::PoolNotInitialized);
    }

    // Check for acquired worktrees
    let acquired = state.count_by_status(&WorktreeStatus::Acquired);
    if acquired > 0 && !force {
        return Err(GwError::PoolHasAcquiredWorktrees(acquired));
    }

    let total = state.entries.len();

    for (i, entry) in state.entries.iter().enumerate() {
        output::info(&format!(
            "[{}/{}] Removing {}...",
            i + 1,
            total,
            output::bold(&entry.name)
        ));

        let path_str = entry.path.to_string_lossy().to_string();

        // Remove the worktree (run from leader root so it works even if cwd is deleted)
        if let Err(e) = git::git_run_in_dir(
            &leader_str,
            &["worktree", "remove", "--force", &path_str],
            verbose,
        ) {
            output::warn(&format!("Failed to remove worktree {}: {e}", entry.name));
            let _ = std::fs::remove_dir_all(&entry.path);
        }

        // Delete the pool branch
        if let Err(e) = git::git_run_in_dir(&leader_str, &["branch", "-D", &entry.branch], verbose)
        {
            output::warn(&format!("Failed to delete branch {}: {e}", entry.branch));
        }

        // Remove acquired marker if present
        let marker = acquired_dir.join(&entry.name);
        let _ = std::fs::remove_file(&marker);

        output::success(&format!("[{}/{}] Removed {}", i + 1, total, entry.name));
    }

    // Clean up pool metadata
    if acquired_dir.exists() {
        let _ = std::fs::remove_dir_all(&acquired_dir);
    }
    let _ = std::fs::remove_file(pool_dir.join("pool.lock"));

    // Prune worktree references
    git::git_run_in_dir(&leader_str, &["worktree", "prune"], verbose)?;

    // Remove empty directories
    if wt_dir.exists() {
        let _ = std::fs::remove_dir(&wt_dir);
    }
    drop(_lock);
    let _ = std::fs::remove_dir(&pool_dir);

    println!();
    output::success(&format!("Drained {total} worktree(s) from pool"));

    Ok(())
}
