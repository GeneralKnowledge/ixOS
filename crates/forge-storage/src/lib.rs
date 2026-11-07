//! Portable persistence for Forge apps: atomic, durable file writes,
//! versioned JSON documents with migrations ([`JsonFile`]) and, with the
//! `sqlite` feature, an embedded SQLite database ([`Database`]).

mod atomic;
#[cfg(feature = "sqlite")]
mod database;
mod json_file;

pub use atomic::{write_atomic, StorageError};
#[cfg(feature = "sqlite")]
pub use database::{Database, Opened};
pub use json_file::{JsonFile, Migration};
/// The SQLite bindings [`Database`] uses, for an app's own queries.
#[cfg(feature = "sqlite")]
pub use rusqlite;
