mod json_store;

pub use json_store::{JsonSnapshotStore, SnapshotEnvelope, StorageError, SCHEMA_VERSION};

/// Narrow persistence interface (SQLite or platform adapters can implement this later).
pub trait SnapshotStore {
    type Snapshot: Clone;

    fn load(&self) -> Result<Option<Self::Snapshot>, StorageError>;
    fn save(&self, snapshot: &Self::Snapshot) -> Result<(), StorageError>;
}
