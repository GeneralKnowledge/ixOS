//! The Counter domain: rules and validation, with no FFI or platform code.

mod actions;
mod errors;
mod state;

pub use actions::{CounterAction, CounterEngine};
pub use errors::CounterError;
pub use state::CounterSnapshot;
#[cfg(test)]
pub use state::COUNTER_MAX;
