//! Worktree pool management

pub mod detect;
pub mod lock;

#[cfg(test)]
mod tests;

pub use detect::{PoolEntry, PoolNextAction, PoolState, WorktreeStatus};
pub use lock::PoolLock;
