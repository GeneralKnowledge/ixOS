use forge_net::NetError;
use forge_storage::{FileStoreError, StorageError};

/// Every error the API returns. `reason` fields are diagnostics for logs and
/// detail text; the host writes its own headline per case. (Fields avoid the
/// name `message`, which clashes with Kotlin's `Throwable.message`.)
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum LibraryError {
    /// The form or request is not acceptable; `reason` says which field.
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error("no book with id {id}")]
    NotFound { id: u64 },
    /// That book is already in the library (importing the same catalogue
    /// work or EPUB twice).
    #[error("already in the library as book {id}")]
    AlreadyInLibrary { id: u64 },
    /// The library file could not be read or written. Nothing in memory
    /// changed; the file on disk is left as it was.
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// The library on disk was written by a newer version of the app.
    #[error(
        "library file is from a newer version (schema {found}, this build reads up to {supported})"
    )]
    NewerLibrary { found: u32, supported: u32 },
    /// The online catalogue is switched off in Settings.
    #[error("the online catalogue is turned off")]
    CatalogueDisabled,
    /// No connection, DNS failure, or connection refused.
    #[error("offline: {reason}")]
    Offline { reason: String },
    #[error("the request timed out")]
    Timeout,
    /// The catalogue answered, but not usefully (5xx, 429, maintenance).
    #[error("catalogue unavailable: {reason}")]
    CatalogueUnavailable { reason: String },
    /// The catalogue's answer could not be understood.
    #[error("unexpected catalogue response: {reason}")]
    BadResponse { reason: String },
    /// The server's TLS certificate was rejected.
    #[error("certificate rejected: {reason}")]
    Certificate { reason: String },
    /// Any other network failure.
    #[error("network error: {reason}")]
    Network { reason: String },
    /// The file is not an EPUB this app can read.
    #[error("unsupported file: {reason}")]
    UnsupportedFile { reason: String },
    /// The book has no saved EPUB to read (typed in, from the catalogue, or
    /// imported before Pocket Library kept EPUBs), or the saved one is gone.
    #[error("this book can't be opened: {reason}")]
    NotReadable { reason: String },
    /// The request was stopped with its `Cancellation`. Nothing changed.
    #[error("cancelled")]
    Cancelled,
    /// `shutdown()` was called; the instance accepts no more work.
    #[error("the library has been closed")]
    ShutDown,
}

impl LibraryError {
    pub(crate) fn invalid(reason: impl Into<String>) -> Self {
        Self::InvalidInput {
            reason: reason.into(),
        }
    }
}

impl From<StorageError> for LibraryError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::UnsupportedSchemaVersion { found, supported } => {
                Self::NewerLibrary { found, supported }
            }
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

impl From<NetError> for LibraryError {
    fn from(err: NetError) -> Self {
        match err {
            NetError::InvalidRequest { reason } => Self::InvalidInput { reason },
            NetError::Offline { reason } => Self::Offline { reason },
            NetError::Certificate { reason } => Self::Certificate { reason },
            NetError::Timeout => Self::Timeout,
            NetError::Cancelled => Self::Cancelled,
            other => Self::Network {
                reason: other.to_string(),
            },
        }
    }
}

impl From<FileStoreError> for LibraryError {
    fn from(err: FileStoreError) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}
