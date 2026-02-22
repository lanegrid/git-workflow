//! `gw worktree pool` commands — Worktree pool management

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{GwError, Result};
use crate::git;
use crate::output;
use crate::pool::{Inventory, PoolEntry, PoolLock, WorktreeStatus};

/// Directory name under git_common_dir for pool metadata
const POOL_META_DIR: &str = "worktree-pool";

/// Directory name under repo root for pool worktrees
const POOL_WORKTREES_DIR: &str = ".worktrees";

/// Setup hook path relative to repo root
const SETUP_HOOK: &str = ".gw/setup";

/// Get the main repository root (parent of .git), even from inside a worktree.
/// Unlike `git::repo_root()` which returns the worktree root when called from
/// inside a worktree, this always returns the main repo root.
fn main_repo_root() -> Result<PathBuf> {
    let common = git::git_common_dir()?;
    // git_common_dir may return a relative path like ".git", so canonicalize
    // to get an absolute path before taking the parent.
    let common = std::fs::canonicalize(&common).unwrap_or(common);
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

/// Resolve the inventory file path
fn inventory_path() -> Result<PathBuf> {
    Ok(pool_dir()?.join("inventory.json"))
}

/// Resolve the worktrees directory ({main_repo_root}/.worktrees/)
fn worktrees_dir() -> Result<PathBuf> {
    let root = main_repo_root()?;
    Ok(root.join(POOL_WORKTREES_DIR))
}

/// Get current unix timestamp
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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

/// `gw worktree pool warm <n>`
pub fn warm(count: usize, verbose: bool) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let pool_dir = pool_dir()?;
    let inv_path = inventory_path()?;
    let wt_dir = worktrees_dir()?;
    let repo_root = main_repo_root()?;

    println!();
    output::info(&format!(
        "Warming worktree pool to {} available",
        output::bold(&count.to_string())
    ));

    // Acquire lock and load inventory
    let _lock = PoolLock::acquire(&pool_dir)?;
    let mut inventory = Inventory::load(&inv_path)?;

    let available = inventory.count_by_status(&WorktreeStatus::Available);
    let acquired = inventory.count_by_status(&WorktreeStatus::Acquired);
    let total = inventory.worktrees.len();
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
        let name = inventory.next_name();
        let abs_path = std::fs::canonicalize(&wt_dir)
            .unwrap_or_else(|_| wt_dir.clone())
            .join(&name);
        let abs_path_str = abs_path.to_string_lossy().to_string();
        let branch = format!("pool/{name}");

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

        inventory.worktrees.push(PoolEntry {
            name: name.clone(),
            path: abs_path_str,
            branch,
            status: WorktreeStatus::Available,
            created_at: now_unix(),
            acquired_at: None,
            acquired_by: None,
        });
        created += 1;

        output::success(&format!("[{}/{}] Created {}", i + 1, to_create, name));
    }

    inventory.save(&inv_path)?;

    let total = inventory.worktrees.len();
    let available = inventory.count_by_status(&WorktreeStatus::Available);

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
    let inv_path = inventory_path()?;

    if !inv_path.exists() {
        return Err(GwError::PoolNotInitialized);
    }

    let _lock = PoolLock::acquire(&pool_dir)?;
    let mut inventory = Inventory::load(&inv_path)?;

    let idx = inventory.find_available().ok_or(GwError::PoolExhausted)?;

    let entry = &mut inventory.worktrees[idx];
    entry.status = WorktreeStatus::Acquired;
    entry.acquired_at = Some(now_unix());
    entry.acquired_by = Some(std::process::id());

    let path = entry.path.clone();
    let name = entry.name.clone();

    inventory.save(&inv_path)?;

    // Always print status to stderr so stdout stays clean for scripting
    let remaining = inventory.count_by_status(&WorktreeStatus::Available);
    eprintln!(
        "\x1b[0;32m\u{2713}\x1b[0m Acquired {} (PID {}, {} remaining)",
        name,
        std::process::id(),
        remaining,
    );

    // Print ONLY the path to stdout for `path=$(gw worktree pool acquire)`
    println!("{path}");

    Ok(())
}

/// `gw worktree pool release [name|path]`
pub fn release(identifier: Option<&str>, verbose: bool) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let pool_dir = pool_dir()?;
    let inv_path = inventory_path()?;
    let repo_root = main_repo_root()?;

    if !inv_path.exists() {
        return Err(GwError::PoolNotInitialized);
    }

    // Auto-detect from cwd if no identifier given
    let resolved = match identifier {
        Some(id) => id.to_string(),
        None => {
            let cwd = std::env::current_dir()
                .map_err(GwError::Io)?
                .to_string_lossy()
                .to_string();
            cwd
        }
    };

    println!();
    output::info(&format!("Releasing worktree: {}", output::bold(&resolved)));

    let _lock = PoolLock::acquire(&pool_dir)?;
    let mut inventory = Inventory::load(&inv_path)?;

    let idx = inventory
        .find_by_name_or_path(&resolved)
        .ok_or_else(|| GwError::PoolWorktreeNotFound(resolved.clone()))?;

    let entry = &inventory.worktrees[idx];
    let wt_path = entry.path.clone();
    let name = entry.name.clone();

    let default_remote = git::get_default_remote_branch()?;

    // Fetch latest before resetting so the worktree gets a fresh base
    output::info("Fetching from origin...");
    git::git_run_in_dir(&wt_path, &["fetch", "--prune", "--quiet"], verbose)?;
    output::success("Fetched");

    // Reset worktree to clean state
    output::info("Resetting worktree...");
    git::git_run_in_dir(&wt_path, &["reset", "--hard", &default_remote], verbose)?;
    git::git_run_in_dir(&wt_path, &["clean", "-fd"], verbose)?;
    output::success("Reset to clean state");

    // Re-run setup hook
    if let Err(e) = run_setup_hook(&repo_root, &wt_path, verbose) {
        output::warn(&format!("Setup hook failed during release: {e}"));
    }

    let entry = &mut inventory.worktrees[idx];
    entry.status = WorktreeStatus::Available;
    entry.acquired_at = None;
    entry.acquired_by = None;

    inventory.save(&inv_path)?;

    output::success(&format!("Released {}", output::bold(&name)));

    Ok(())
}

/// `gw worktree pool status`
pub fn status() -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let inv_path = inventory_path()?;

    if !inv_path.exists() {
        return Err(GwError::PoolNotInitialized);
    }

    // Read-only — no lock needed
    let inventory = Inventory::load(&inv_path)?;

    let available = inventory.count_by_status(&WorktreeStatus::Available);
    let acquired = inventory.count_by_status(&WorktreeStatus::Acquired);
    let total = inventory.worktrees.len();

    println!();
    output::info(&format!(
        "Pool: {} available, {} acquired, {} total",
        output::bold(&available.to_string()),
        output::bold(&acquired.to_string()),
        output::bold(&total.to_string()),
    ));
    println!();

    // Table header
    let header = format!("{:<12} {:<12} {:<8} PATH", "NAME", "STATUS", "PID");
    println!("{header}");
    println!("{}", "-".repeat(72));

    for entry in &inventory.worktrees {
        let pid = entry
            .acquired_by
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{:<12} {:<12} {:<8} {}",
            entry.name, entry.status, pid, entry.path
        );
    }

    println!();
    Ok(())
}

/// `gw worktree pool drain [--force]`
pub fn drain(force: bool, verbose: bool) -> Result<()> {
    if !git::is_git_repo() {
        return Err(GwError::NotAGitRepository);
    }

    let pool_dir = pool_dir()?;
    let inv_path = inventory_path()?;
    // Resolve all paths upfront — the cwd might be inside a pool worktree
    // that we're about to delete, so all git/fs calls after removal must
    // not depend on cwd being valid.
    let repo_root = main_repo_root()?;
    let repo_root_str = repo_root.to_string_lossy().to_string();
    let wt_dir = worktrees_dir()?;

    if !inv_path.exists() {
        return Err(GwError::PoolNotInitialized);
    }

    println!();
    output::info("Draining worktree pool...");

    let _lock = PoolLock::acquire(&pool_dir)?;
    let inventory = Inventory::load(&inv_path)?;

    // Check for acquired worktrees
    let acquired = inventory.count_by_status(&WorktreeStatus::Acquired);
    if acquired > 0 && !force {
        return Err(GwError::PoolHasAcquiredWorktrees(acquired));
    }

    let total = inventory.worktrees.len();

    for (i, entry) in inventory.worktrees.iter().enumerate() {
        output::info(&format!(
            "[{}/{}] Removing {}...",
            i + 1,
            total,
            output::bold(&entry.name)
        ));

        // Remove the worktree (run from repo_root so it works even if cwd is deleted)
        if let Err(e) = git::git_run_in_dir(
            &repo_root_str,
            &["worktree", "remove", "--force", &entry.path],
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

        output::success(&format!("[{}/{}] Removed {}", i + 1, total, entry.name));
    }

    // Clean up pool metadata
    let _ = std::fs::remove_file(&inv_path);
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
