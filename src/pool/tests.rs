//! Tests for pool filesystem detection and lock

use std::fs;
use std::time::Duration;

use tempfile::TempDir;

use super::detect::{PoolNextAction, PoolState, WorktreeStatus};
use super::lock::PoolLock;

/// Create pool directories and optional acquire markers for testing
fn setup_pool(
    dir: &TempDir,
    names: &[&str],
    acquired: &[(&str, &str)],
) -> (std::path::PathBuf, std::path::PathBuf) {
    let wt_dir = dir.path().join(".worktrees");
    let acq_dir = dir.path().join("acquired");
    fs::create_dir_all(&wt_dir).unwrap();
    fs::create_dir_all(&acq_dir).unwrap();
    for name in names {
        fs::create_dir_all(wt_dir.join(name)).unwrap();
    }
    for (name, owner) in acquired {
        fs::write(acq_dir.join(name), owner).unwrap();
    }
    (wt_dir, acq_dir)
}

// --- PoolState::scan ---

#[test]
fn test_scan_empty_directory() {
    let dir = TempDir::new().unwrap();
    let wt_dir = dir.path().join(".worktrees");
    let acq_dir = dir.path().join("acquired");
    fs::create_dir_all(&wt_dir).unwrap();
    fs::create_dir_all(&acq_dir).unwrap();

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert!(state.entries.is_empty());
}

#[test]
fn test_scan_nonexistent_directory() {
    let dir = TempDir::new().unwrap();
    let wt_dir = dir.path().join(".worktrees");
    let acq_dir = dir.path().join("acquired");

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert!(state.entries.is_empty());
}

#[test]
fn test_scan_finds_pool_directories_sorted() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(&dir, &["pool-003", "pool-001", "pool-002"], &[]);

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(state.entries.len(), 3);
    assert_eq!(state.entries[0].name, "pool-001");
    assert_eq!(state.entries[1].name, "pool-002");
    assert_eq!(state.entries[2].name, "pool-003");
}

#[test]
fn test_scan_ignores_non_pool_directories() {
    let dir = TempDir::new().unwrap();
    let wt_dir = dir.path().join(".worktrees");
    let acq_dir = dir.path().join("acquired");
    fs::create_dir_all(&wt_dir).unwrap();
    fs::create_dir_all(wt_dir.join("pool-001")).unwrap();
    fs::create_dir_all(wt_dir.join("feature-branch")).unwrap();
    fs::create_dir_all(wt_dir.join("my-worktree")).unwrap();
    fs::create_dir_all(&acq_dir).unwrap();

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(state.entries.len(), 1);
    assert_eq!(state.entries[0].name, "pool-001");
}

#[test]
fn test_scan_detects_acquired_from_marker() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(&dir, &["pool-001", "pool-002"], &[("pool-001", "web-2")]);

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(state.entries[0].status, WorktreeStatus::Acquired);
    assert_eq!(state.entries[0].owner.as_deref(), Some("web-2"));
    assert_eq!(state.entries[1].status, WorktreeStatus::Available);
    assert_eq!(state.entries[1].owner, None);
}

#[test]
fn test_scan_available_when_no_marker() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(&dir, &["pool-001"], &[]);

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(state.entries[0].status, WorktreeStatus::Available);
}

// --- count_by_status ---

#[test]
fn test_count_by_status_mixed() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(
        &dir,
        &["pool-001", "pool-002", "pool-003"],
        &[("pool-001", "web-2"), ("pool-003", "web-3")],
    );

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(state.count_by_status(&WorktreeStatus::Available), 1);
    assert_eq!(state.count_by_status(&WorktreeStatus::Acquired), 2);
}

// --- find_available ---

#[test]
fn test_find_available_returns_first() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(
        &dir,
        &["pool-001", "pool-002", "pool-003"],
        &[("pool-001", "web-2")],
    );

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    let found = state.find_available().unwrap();
    assert_eq!(found.name, "pool-002");
}

#[test]
fn test_find_available_none_when_all_acquired() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(
        &dir,
        &["pool-001", "pool-002"],
        &[("pool-001", "web-2"), ("pool-002", "web-3")],
    );

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert!(state.find_available().is_none());
}

// --- find_by_name_or_path ---

#[test]
fn test_find_by_name() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(&dir, &["pool-001", "pool-002"], &[]);

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    let found = state.find_by_name_or_path("pool-002").unwrap();
    assert_eq!(found.name, "pool-002");
}

#[test]
fn test_find_by_path() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(&dir, &["pool-001"], &[]);

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    let path_str = wt_dir.join("pool-001").to_string_lossy().to_string();
    let found = state.find_by_name_or_path(&path_str).unwrap();
    assert_eq!(found.name, "pool-001");
}

#[test]
fn test_find_nonexistent() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(&dir, &["pool-001"], &[]);

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert!(state.find_by_name_or_path("pool-999").is_none());
}

// --- next_name ---

#[test]
fn test_next_name_empty() {
    let dir = TempDir::new().unwrap();
    let wt_dir = dir.path().join(".worktrees");
    let acq_dir = dir.path().join("acquired");

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(state.next_name(), "pool-001");
}

#[test]
fn test_next_name_sequential() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(&dir, &["pool-001", "pool-002", "pool-003"], &[]);

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(state.next_name(), "pool-004");
}

#[test]
fn test_next_name_with_gap() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(&dir, &["pool-001", "pool-005"], &[]);

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(state.next_name(), "pool-006");
}

// --- PoolNextAction ---

#[test]
fn test_next_action_warm_pool_when_empty() {
    let dir = TempDir::new().unwrap();
    let wt_dir = dir.path().join(".worktrees");
    let acq_dir = dir.path().join("acquired");

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(state.next_action(), PoolNextAction::WarmPool);
}

#[test]
fn test_next_action_ready_when_mixed() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(
        &dir,
        &["pool-001", "pool-002", "pool-003"],
        &[("pool-001", "web-2")],
    );

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(state.next_action(), PoolNextAction::Ready { available: 2 });
}

#[test]
fn test_next_action_exhausted_when_all_acquired() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(
        &dir,
        &["pool-001", "pool-002"],
        &[("pool-001", "web-2"), ("pool-002", "web-3")],
    );

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(
        state.next_action(),
        PoolNextAction::Exhausted { acquired: 2 }
    );
}

#[test]
fn test_next_action_all_idle_when_none_acquired() {
    let dir = TempDir::new().unwrap();
    let (wt_dir, acq_dir) = setup_pool(&dir, &["pool-001", "pool-002"], &[]);

    let state = PoolState::scan(&wt_dir, &acq_dir).unwrap();
    assert_eq!(
        state.next_action(),
        PoolNextAction::AllIdle { available: 2 }
    );
}

// --- WorktreeStatus::Display ---

#[test]
fn test_status_display() {
    assert_eq!(WorktreeStatus::Available.to_string(), "available");
    assert_eq!(WorktreeStatus::Acquired.to_string(), "acquired");
}

// --- PoolLock ---

#[test]
fn test_lock_creates_dir_and_file() {
    let dir = TempDir::new().unwrap();
    let pool_dir = dir.path().join("pool");

    let _lock = PoolLock::acquire(&pool_dir).unwrap();

    assert!(pool_dir.exists());
    assert!(pool_dir.join("pool.lock").exists());
}

#[test]
fn test_lock_released_on_drop() {
    let dir = TempDir::new().unwrap();
    let pool_dir = dir.path().join("pool");

    {
        let _lock = PoolLock::acquire(&pool_dir).unwrap();
    } // lock dropped here

    // Should be able to acquire again
    let _lock = PoolLock::acquire(&pool_dir).unwrap();
}

#[test]
fn test_lock_exclusive_times_out() {
    let dir = TempDir::new().unwrap();
    let pool_dir = dir.path().join("pool");

    let _lock = PoolLock::acquire(&pool_dir).unwrap();

    // Second acquire with short timeout should fail
    let result = PoolLock::acquire_with_timeout(&pool_dir, Duration::from_millis(100));
    assert!(result.is_err());
}
