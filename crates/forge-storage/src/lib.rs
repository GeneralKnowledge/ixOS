//! Portable persistence for Forge apps: atomic, durable file writes and
//! versioned JSON documents with migrations ([`JsonFile`]).

mod atomic;
mod json_file;

pub use atomic::{write_atomic, StorageError};
pub use json_file::{JsonFile, Migration};
