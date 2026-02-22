//! Tests for pool inventory and lock

use tempfile::TempDir;

use super::inventory::{Inventory, PoolEntry, WorktreeStatus};
use super::lock::PoolLock;

/// Create a test pool entry with the given name and status
fn make_entry(name: &str, status: WorktreeStatus) -> PoolEntry {
    PoolEntry {
        name: name.to_string(),
        path: format!("/tmp/test/.worktrees/{name}"),
        branch: format!("pool/{name}"),
        status,
        created_at: 1700000000,
        acquired_at: None,
        acquired_by: None,
    }
}

// --- Inventory: new / default ---

#[test]
fn test_new_inventory_is_empty() {
    let inv = Inventory::new();
    assert_eq!(inv.version, 1);
    assert!(inv.worktrees.is_empty());
}

#[test]
fn test_default_equals_new() {
    let a = Inventory::new();
    let b = Inventory::default();
    assert_eq!(a.version, b.version);
    assert_eq!(a.worktrees.len(), b.worktrees.len());
}

// --- Inventory: save / load round-trip ---

#[test]
fn test_save_and_load_round_trip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("inventory.json");

    let mut inv = Inventory::new();
    inv.worktrees
        .push(make_entry("pool-001", WorktreeStatus::Available));
    inv.worktrees
        .push(make_entry("pool-002", WorktreeStatus::Acquired));
    inv.save(&path).unwrap();

    let loaded = Inventory::load(&path).unwrap();
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.worktrees.len(), 2);
    assert_eq!(loaded.worktrees[0].name, "pool-001");
    assert_eq!(loaded.worktrees[0].status, WorktreeStatus::Available);
    assert_eq!(loaded.worktrees[1].name, "pool-002");
    assert_eq!(loaded.worktrees[1].status, WorktreeStatus::Acquired);
}

#[test]
fn test_load_nonexistent_returns_empty() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("does-not-exist.json");

    let inv = Inventory::load(&path).unwrap();
    assert!(inv.worktrees.is_empty());
}

#[test]
fn test_load_invalid_json_returns_error() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "not valid json").unwrap();

    let result = Inventory::load(&path);
    assert!(result.is_err());
}

// --- Inventory: acquired_at / acquired_by serialization ---

#[test]
fn test_acquired_fields_round_trip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("inventory.json");

    let mut inv = Inventory::new();
    let mut entry = make_entry("pool-001", WorktreeStatus::Acquired);
    entry.acquired_at = Some(1700000099);
    entry.acquired_by = Some(12345);
    inv.worktrees.push(entry);
    inv.save(&path).unwrap();

    let loaded = Inventory::load(&path).unwrap();
    assert_eq!(loaded.worktrees[0].acquired_at, Some(1700000099));
    assert_eq!(loaded.worktrees[0].acquired_by, Some(12345));
}

#[test]
fn test_null_acquired_fields_round_trip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("inventory.json");

    let mut inv = Inventory::new();
    inv.worktrees
        .push(make_entry("pool-001", WorktreeStatus::Available));
    inv.save(&path).unwrap();

    let loaded = Inventory::load(&path).unwrap();
    assert_eq!(loaded.worktrees[0].acquired_at, None);
    assert_eq!(loaded.worktrees[0].acquired_by, None);
}

// --- Inventory: count_by_status ---

#[test]
fn test_count_by_status_empty() {
    let inv = Inventory::new();
    assert_eq!(inv.count_by_status(&WorktreeStatus::Available), 0);
    assert_eq!(inv.count_by_status(&WorktreeStatus::Acquired), 0);
}

#[test]
fn test_count_by_status_mixed() {
    let mut inv = Inventory::new();
    inv.worktrees
        .push(make_entry("pool-001", WorktreeStatus::Available));
    inv.worktrees
        .push(make_entry("pool-002", WorktreeStatus::Acquired));
    inv.worktrees
        .push(make_entry("pool-003", WorktreeStatus::Available));

    assert_eq!(inv.count_by_status(&WorktreeStatus::Available), 2);
    assert_eq!(inv.count_by_status(&WorktreeStatus::Acquired), 1);
}

// --- Inventory: next_name ---

#[test]
fn test_next_name_empty() {
    let inv = Inventory::new();
    assert_eq!(inv.next_name(), "pool-001");
}

#[test]
fn test_next_name_sequential() {
    let mut inv = Inventory::new();
    inv.worktrees
        .push(make_entry("pool-001", WorktreeStatus::Available));
    inv.worktrees
        .push(make_entry("pool-002", WorktreeStatus::Available));
    assert_eq!(inv.next_name(), "pool-003");
}

#[test]
fn test_next_name_with_gap() {
    let mut inv = Inventory::new();
    inv.worktrees
        .push(make_entry("pool-001", WorktreeStatus::Available));
    inv.worktrees
        .push(make_entry("pool-005", WorktreeStatus::Available));
    // Should use max+1, not fill gaps
    assert_eq!(inv.next_name(), "pool-006");
}

// --- Inventory: find_available ---

#[test]
fn test_find_available_none() {
    let mut inv = Inventory::new();
    inv.worktrees
        .push(make_entry("pool-001", WorktreeStatus::Acquired));
    assert_eq!(inv.find_available(), None);
}

#[test]
fn test_find_available_returns_first() {
    let mut inv = Inventory::new();
    inv.worktrees
        .push(make_entry("pool-001", WorktreeStatus::Acquired));
    inv.worktrees
        .push(make_entry("pool-002", WorktreeStatus::Available));
    inv.worktrees
        .push(make_entry("pool-003", WorktreeStatus::Available));
    assert_eq!(inv.find_available(), Some(1));
}

#[test]
fn test_find_available_empty_inventory() {
    let inv = Inventory::new();
    assert_eq!(inv.find_available(), None);
}

// --- Inventory: find_by_name_or_path ---

#[test]
fn test_find_by_name() {
    let mut inv = Inventory::new();
    inv.worktrees
        .push(make_entry("pool-001", WorktreeStatus::Available));
    inv.worktrees
        .push(make_entry("pool-002", WorktreeStatus::Acquired));

    assert_eq!(inv.find_by_name_or_path("pool-002"), Some(1));
}

#[test]
fn test_find_by_path() {
    let mut inv = Inventory::new();
    inv.worktrees
        .push(make_entry("pool-001", WorktreeStatus::Available));

    assert_eq!(
        inv.find_by_name_or_path("/tmp/test/.worktrees/pool-001"),
        Some(0)
    );
}

#[test]
fn test_find_nonexistent() {
    let mut inv = Inventory::new();
    inv.worktrees
        .push(make_entry("pool-001", WorktreeStatus::Available));

    assert_eq!(inv.find_by_name_or_path("pool-999"), None);
    assert_eq!(inv.find_by_name_or_path("/wrong/path"), None);
}

// --- WorktreeStatus: Display ---

#[test]
fn test_status_display() {
    assert_eq!(WorktreeStatus::Available.to_string(), "available");
    assert_eq!(WorktreeStatus::Acquired.to_string(), "acquired");
}

// --- WorktreeStatus: serde ---

#[test]
fn test_status_serde_round_trip() {
    let json = serde_json::to_string(&WorktreeStatus::Available).unwrap();
    assert_eq!(json, "\"available\"");

    let json = serde_json::to_string(&WorktreeStatus::Acquired).unwrap();
    assert_eq!(json, "\"acquired\"");

    let parsed: WorktreeStatus = serde_json::from_str("\"available\"").unwrap();
    assert_eq!(parsed, WorktreeStatus::Available);
}

// --- PoolLock ---

#[test]
fn test_lock_creates_dir_and_file() {
    let dir = TempDir::new().unwrap();
    let pool_dir = dir.path().join("pool");
    assert!(!pool_dir.exists());

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
        // Lock is held here
    }
    // Lock should be released after drop

    // Should be able to re-acquire
    let _lock2 = PoolLock::acquire(&pool_dir).unwrap();
}

#[test]
fn test_lock_exclusive() {
    let dir = TempDir::new().unwrap();
    let pool_dir = dir.path().join("pool");

    let _lock1 = PoolLock::acquire(&pool_dir).unwrap();

    // Second acquire should fail (try_lock_exclusive returns error)
    let result = PoolLock::acquire(&pool_dir);
    assert!(result.is_err());
}
