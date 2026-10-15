//! Domain logic for Forge Mobile applications (no FFI or platform dependencies).

mod actions;
mod errors;
mod state;

pub use actions::{CounterAction, CounterEngine};
pub use errors::CounterError;
pub use state::{CounterSnapshot, COUNTER_MAX, COUNTER_MIN};
