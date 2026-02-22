//! Worktree pool management

pub mod inventory;
pub mod lock;

pub use inventory::{Inventory, PoolEntry, WorktreeStatus};
pub use lock::PoolLock;
