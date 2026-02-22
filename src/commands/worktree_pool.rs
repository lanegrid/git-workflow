//! `gw worktree pool` commands — Worktree pool management
//!
//! Pool state is derived from the filesystem, not from an inventory file.
//! Marker files in `.git/worktree-pool/acquired/` track acquisition state.

use std::path::{Path, PathBuf};

use crate::error::{GwError, Result};
use crate::git;
use crate::output;
use crate::pool::{PoolEntry, PoolLock, PoolNextAction, PoolState, WorktreeStatus};

/// Directory name under git_common_dir for pool metadata
const POOL_META_DIR: &str = "worktree-pool";

/// Directory name under repo root for pool worktrees
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

/// Get the main repository root (parent of .git), even from inside a worktree.
fn main_repo_root() -> Result<PathBuf> {
    let common = git::git_common_dir()?;
    let common = canonicalize_clean(&common);
    common
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| GwError::Other("Could not determine main repository root".to_string()))
}

/// Resolve the pool metadata directory ({git_common_dir}/worktree-pool/)
fn pool_dir() -> Result<PathBuf> {
    let common = git::git_common_dir()?;
    Ok(common.join(POOL_META_DIR))
}

/// Resolve the acquired markers directory
fn acquired_dir() -> Result<PathBuf> {
    Ok(pool_dir()?.join(ACQUIRED_DIR))
}

/// Resolve the worktrees directory ({main_repo_root}/.worktrees/)
fn worktrees_dir() -> Result<PathBuf> {
    let root = main_repo_root()?;
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

/// Get the current branch of a worktree by running git in that directory
fn worktree_current_branch(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&*path_str)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "???".to_string())
}

/// Get the owner name for acquire markers (current worktree directory name)
fn current_owner_name() -> String {
    git::current_dir_name().unwrap_or_else(|_| "unknown".to_string())
}

/// Check if the current directory is inside a pool worktree.
/// Returns the pool worktree name if so.
pub fn detect_pool_worktree() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let wt_dir = worktrees_dir().ok()?;

    if !cwd.starts_with(&wt_dir) {
        return None;
    }

    // Get the pool worktree directory name (first component after .worktrees/)
    cwd.strip_prefix(&wt_dir)
        .ok()
        .and_then(|rel| rel.components().next())
        .and_then(|c| c.as_os_str().to_str())
        .filter(|name| name.starts_with("pool-"))
        .map(String::from)
}

/// Release a pool worktree: clean files, remove marker, run setup hook.
/// Called from `gw cleanup` when inside a pool worktree.
pub fn pool_release_after_cleanup(pool_name: &str, verbose: bool) -> Result<()> {
    let acquired_dir = acquired_dir()?;
    let repo_root = main_repo_root()?;
    let wt_dir = worktrees_dir()?;
    let wt_path = wt_dir.join(pool_name);
    let wt_path_str = wt_path.to_string_lossy().to_string();

    output::info("Releasing pool worktree...");

    // Clean untracked files
    git::git_run_in_dir(&wt_path_str, &["clean", "-fd"], verbose)?;

    // Run setup hook (if it fails, keep marker and warn)
    if let Err(e) = run_setup_hook(&repo_root, &wt_path_str, verbose) {
        output::warn(&format!(
            "Setup hook failed during release: {e}. Worktree remains acquired."
        ));
        return Ok(());
    }

    // Remove acquired marker
    let marker = acquired_dir.join(pool_name);
    if marker.exists() {
        std::fs::remove_file(&marker)?;
    }

    output::success("Pool worktree released and available for reuse");
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

    println!();
    output::info(&format!(
        "Warming worktree pool to {} available",
        output::bold(&count.to_string())
    ));

    // Acquire lock and scan filesystem
    let _lock = PoolLock::acquire(&pool_dir)?;
    let mut state = PoolState::scan(&wt_dir, &acquired_dir)?;

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

    // Fetch once
    output::info("Fetching from origin...");
    git::fetch_prune(verbose)?;
    output::success("Fetched");

    let default_remote = git::get_default_remote_branch()?;

    // Create worktrees dir
    std::fs::create_dir_all(&wt_dir)?;

    let mut created = 0;
    for i in 0..to_create {
        let name = state.next_name();
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
    let final_state = PoolState::scan(&wt_dir, &acquired_dir)?;
    let total = final_state.entries.len();
    let available = final_state.count_by_status(&WorktreeStatus::Available);

    println!();
    output::success(&format!(
        "Pool warmed: {created} created, {available} available, {total} total"
    ));

    Ok(())
}

/// `gw worktree pool acquire`
pub fn acquire(_verbose: bool) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let pool_dir = pool_dir()?;
    let wt_dir = worktrees_dir()?;
    let acquired_dir = acquired_dir()?;

    if !wt_dir.exists() {
        return Err(GwError::PoolNotInitialized);
    }

    let _lock = PoolLock::acquire(&pool_dir)?;

    // Ensure acquired dir exists
    std::fs::create_dir_all(&acquired_dir)?;

    let state = PoolState::scan(&wt_dir, &acquired_dir)?;

    if state.entries.is_empty() {
        return Err(GwError::PoolNotInitialized);
    }

    let entry = state.find_available().ok_or(GwError::PoolExhausted)?;

    // Create marker file with owner name
    let owner = current_owner_name();
    std::fs::write(acquired_dir.join(&entry.name), &owner)?;

    let path = entry.path.to_string_lossy().to_string();
    let name = entry.name.clone();

    let remaining = state.count_by_status(&WorktreeStatus::Available) - 1;
    eprintln!(
        "\x1b[0;32m\u{2713}\x1b[0m Acquired {} (owner: {}, {} remaining)",
        name, owner, remaining,
    );

    // Print ONLY the path to stdout for `path=$(gw worktree pool acquire)`
    println!("{path}");

    Ok(())
}

/// `gw worktree pool status`
pub fn status(verbose: bool) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let wt_dir = worktrees_dir()?;
    let acquired_dir = acquired_dir()?;

    if !wt_dir.exists() {
        return Err(GwError::PoolNotInitialized);
    }

    // Read-only — no lock needed
    let state = PoolState::scan(&wt_dir, &acquired_dir)?;

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
    println!();

    // Table header
    let header = format!("{:<12} {:<12} {:<24} OWNER", "NAME", "STATUS", "BRANCH");
    println!("{header}");
    println!("{}", "-".repeat(72));

    for entry in &state.entries {
        let branch = if verbose || entry.status == WorktreeStatus::Acquired {
            worktree_current_branch(&entry.path)
        } else {
            entry.branch.clone()
        };
        let owner = entry.owner.as_deref().unwrap_or("-");
        println!(
            "{:<12} {:<12} {:<24} {}",
            entry.name, entry.status, branch, owner
        );
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
        }
        PoolNextAction::Exhausted { acquired } => {
            output::action(&format!(
                "All {} worktree(s) acquired. Warm more or wait.",
                acquired
            ));
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
    // Resolve all paths upfront — the cwd might be inside a pool worktree
    // that we're about to delete.
    let repo_root = main_repo_root()?;
    let repo_root_str = repo_root.to_string_lossy().to_string();

    if !wt_dir.exists() {
        return Err(GwError::PoolNotInitialized);
    }

    println!();
    output::info("Draining worktree pool...");

    let _lock = PoolLock::acquire(&pool_dir)?;
    let state = PoolState::scan(&wt_dir, &acquired_dir)?;

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

        // Remove the worktree (run from repo_root so it works even if cwd is deleted)
        if let Err(e) = git::git_run_in_dir(
            &repo_root_str,
            &["worktree", "remove", "--force", &path_str],
            verbose,
        ) {
            output::warn(&format!("Failed to remove worktree {}: {e}", entry.name));
            let _ = std::fs::remove_dir_all(&entry.path);
        }

        // Delete the pool branch
        if let Err(e) =
            git::git_run_in_dir(&repo_root_str, &["branch", "-D", &entry.branch], verbose)
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
    git::git_run_in_dir(&repo_root_str, &["worktree", "prune"], verbose)?;

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
