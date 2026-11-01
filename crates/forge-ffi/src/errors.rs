use forge_core::CounterError;
use forge_storage::StorageError;

/// Every error that crosses the FFI boundary.
///
/// Grouped by what the host should do about it (see `docs/ffi-policy.md`):
/// domain validation, persistence, corrupt or newer data, cancellation,
/// lifecycle/resource, and internal failures. `reason` fields carry diagnostic
/// text for logs; they are not meant to be shown to users verbatim.
#[derive(uniffi::Error, Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ForgeAppError {
    // Domain validation. State is unchanged.
    #[error("count cannot go below zero")]
    WouldUnderflow,
    #[error("count cannot exceed maximum")]
    WouldOverflow,
    /// An argument was outside what the operation accepts.
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    // Persistence.
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
    // Cancellation. Not a failure: the operation stopped because it was asked to,
    // and nothing it would have changed was committed.
    #[error("operation was cancelled")]
    Cancelled,
    // Lifecycle and resources.
    /// The app instance has been shut down (or dropped); open a new one.
    #[error("the app instance has been shut down")]
    ShutDown,
    /// Only one long-running operation runs at a time.
    #[error("another long-running operation is already in progress")]
    OperationInProgress,
    // Internal. A bug, not an expected condition.
    #[error("internal application failure: {reason}")]
    InternalFailure { reason: String },
}

impl ForgeAppError {
    pub(crate) fn internal(reason: impl std::fmt::Display) -> Self {
        Self::InternalFailure {
            reason: reason.to_string(),
        }
    }
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
