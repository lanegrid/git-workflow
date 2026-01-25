//! Git abstraction layer

pub mod mutation;
pub mod query;

#[cfg(test)]
mod tests;

pub use mutation::*;
pub use query::*;
