use forge_core::CounterError;
use forge_storage::StorageError;

#[derive(uniffi::Error, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ForgeAppError {
    #[error("count cannot go below zero")]
    WouldUnderflow,
    #[error("count cannot exceed maximum")]
    WouldOverflow,
    #[error("persistence failed")]
    PersistenceFailed,
    #[error("stored data is invalid")]
    CorruptPersistence,
    #[error("failed to read persisted data")]
    PersistenceReadFailed,
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
            StorageError::CorruptData => ForgeAppError::CorruptPersistence,
            StorageError::ReadFailed => ForgeAppError::PersistenceReadFailed,
            StorageError::MissingDirectory => ForgeAppError::StorageDirectoryUnavailable,
            StorageError::WriteFailed => ForgeAppError::PersistenceFailed,
        }
    }
}
