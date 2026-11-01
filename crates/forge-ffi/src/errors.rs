use forge_core::CounterError;
use forge_storage::StorageError;

#[derive(uniffi::Error, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ForgeAppError {
    #[error("count cannot go below zero")]
    WouldUnderflow,
    #[error("count cannot exceed maximum")]
    WouldOverflow,
    #[error("persistence failed: {reason}")]
    PersistenceFailed { reason: String },
    /// Stored data is unreadable or invalid. Hosts may offer a reset.
    #[error("stored data is invalid: {reason}")]
    CorruptPersistence { reason: String },
    /// Stored data was written by a newer build. It is valid and must not be
    /// reset; the user needs to update the app.
    #[error("stored data uses schema version {found}; this build supports up to {supported}")]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    #[error("failed to read persisted data: {reason}")]
    PersistenceReadFailed { reason: String },
    #[error("storage directory is unavailable")]
    StorageDirectoryUnavailable,
    #[error("internal application failure")]
    InternalFailure,
}

impl From<CounterError> for ForgeAppError {
    fn from(value: CounterError) -> Self {
        match value {
            CounterError::WouldUnderflow { .. } => ForgeAppError::WouldUnderflow,
            CounterError::WouldOverflow { .. } => ForgeAppError::WouldOverflow,
        }
    }
}

impl From<StorageError> for ForgeAppError {
    fn from(value: StorageError) -> Self {
        match value {
            StorageError::CorruptData { reason } => ForgeAppError::CorruptPersistence { reason },
            StorageError::UnsupportedSchemaVersion { found, supported } => {
                ForgeAppError::UnsupportedSchemaVersion { found, supported }
            }
            StorageError::ReadFailed { reason } => ForgeAppError::PersistenceReadFailed { reason },
            StorageError::MissingDirectory => ForgeAppError::StorageDirectoryUnavailable,
            StorageError::WriteFailed { reason } => ForgeAppError::PersistenceFailed { reason },
        }
    }
}
