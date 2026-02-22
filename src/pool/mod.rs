//! Worktree pool management

pub mod inventory;
pub mod lock;

#[cfg(test)]
mod tests;

pub use inventory::{Inventory, PoolEntry, WorktreeStatus};
pub use lock::PoolLock;
