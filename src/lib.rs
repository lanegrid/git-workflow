//! git-workflow - Git workflow CLI
//!
//! Type-safe worktree-aware git operations with compile-time protection
//! for protected branches.

pub mod cli;
pub mod commands;
pub mod error;
pub mod git;
pub mod github;
pub mod output;
pub mod state;

pub use error::{GwError, Result};
