//! Portable persistence for Forge apps: atomic, durable file writes,
//! versioned JSON documents with migrations ([`JsonFile`]), folders of
//! app-managed files ([`files::FileStore`]) and, with the `sqlite` feature, an
//! embedded SQLite database ([`Database`]).
//!
//! What these promise at each crash point, and what an app has to do, is in
//! `docs/persistence.md`.

mod atomic;
#[cfg(feature = "backup")]
pub mod backup;
pub mod copies;
#[cfg(feature = "csv")]
pub mod csv_file;
#[cfg(feature = "sqlite")]
mod database;
pub mod files;
mod json_file;

pub use atomic::{write_atomic, StorageError};
pub use copies::set_aside;
#[cfg(feature = "sqlite")]
pub use database::{Database, Opened};
pub use files::{FileStore, FileStoreError, Staged, StoredFile};
pub use json_file::{JsonFile, Migration};
/// The SQLite bindings [`Database`] uses, for an app's own queries.
#[cfg(feature = "sqlite")]
pub use rusqlite;
