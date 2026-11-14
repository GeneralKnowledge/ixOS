//! The types Relay's API speaks in, and its errors.

use forge_app::{DocumentError, ErrorKind, OpenError, Stopped};
use forge_devproto::conn::ConnError;
use forge_storage::FileStoreError;

/// The longest device name, in characters.
pub const MAX_NAME_CHARS: u32 = 40;
/// The longest note, in characters.
pub const MAX_NOTE_CHARS: u32 = 20_000;
/// The largest file Relay sends or accepts.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// This device, as other devices see it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Random, made once; 32 hex characters.
    pub device_id: String,
    pub name: String,
}

/// A device this one is paired with.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub device_id: String,
    pub name: String,
    pub paired_ms: i64,
    /// Where it was last reached ("host:port"), a hint for next time.
    pub last_address: Option<String>,
}

/// A QR code's worth of pairing: show `code` as a QR code (or text) on
/// this device and scan it on the other. Valid once, until `expires_ms`.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Invitation {
    pub code: String,
    pub expires_ms: i64,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Sent,
    Received,
}

/// What went across.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Note {
        text: String,
    },
    /// `path` is the received file in Relay's inbox, or the file that was
    /// sent; `None` once it is gone.
    File {
        name: String,
        size: u64,
        path: Option<String>,
    },
}

#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum TransferStatus {
    /// Bytes so far, out of the item's size.
    InProgress {
        done: u64,
    },
    Done,
    Failed {
        reason: String,
    },
}

/// One note or file, sent or received.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Transfer {
    pub id: u64,
    pub peer_id: String,
    pub peer_name: String,
    pub direction: Direction,
    pub item: Item,
    pub status: TransferStatus,
    pub at_ms: i64,
}

/// Everything that can go wrong, in the app's words. `relay_error_kind`
/// maps each case to the framework's [`ErrorKind`].
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum RelayError {
    #[error("a name can't be empty")]
    EmptyName,
    #[error("names are at most {max} characters")]
    NameTooLong { max: u32 },
    #[error("a note needs some text")]
    EmptyNote,
    #[error("notes are at most {max} characters")]
    NoteTooLong { max: u32 },
    #[error("files are at most 2 GB")]
    FileTooLarge,
    #[error("that isn't a Relay pairing code")]
    BadCode,
    #[error("that pairing code has expired; make a new one")]
    CodeExpired,
    #[error("not paired with {id}")]
    UnknownPeer { id: String },
    #[error("nothing with id {id}")]
    NotFound { id: u64 },
    #[error("can't reach the other device: {reason}")]
    Unreachable { reason: String },
    #[error("the other device didn't accept the pairing or the connection")]
    Refused,
    #[error("the other device declined: {reason}")]
    Declined { reason: String },
    #[error("the connection broke: {reason}")]
    Connection { reason: String },
    #[error("the file can't be read or kept: {reason}")]
    File { reason: String },
    #[error("Relay is already listening")]
    AlreadyListening,
    #[error("Relay's data is open in another copy of the app")]
    InUse,
    #[error("Relay's saved data can't be read: {reason}")]
    Corrupt { reason: String },
    #[error("Relay's data was saved by a newer version of the app")]
    NewerData,
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// Stopped by the runtime: cancelled, interrupted by the background,
    /// or shut down. `kind` says which.
    #[error("stopped ({kind:?})")]
    Stopped { kind: ErrorKind },
}

/// The framework kind of a Relay error.
#[uniffi::export]
pub fn relay_error_kind(error: RelayError) -> ErrorKind {
    match error {
        RelayError::EmptyName
        | RelayError::NameTooLong { .. }
        | RelayError::EmptyNote
        | RelayError::NoteTooLong { .. }
        | RelayError::FileTooLarge
        | RelayError::BadCode
        | RelayError::CodeExpired
        | RelayError::File { .. }
        | RelayError::AlreadyListening
        | RelayError::Refused
        | RelayError::Declined { .. } => ErrorKind::Invalid,
        RelayError::UnknownPeer { .. } | RelayError::NotFound { .. } => ErrorKind::NotFound,
        // The framework has no network kind (friction R3): "try again".
        RelayError::Unreachable { .. } | RelayError::Connection { .. } | RelayError::InUse => {
            ErrorKind::Unavailable
        }
        RelayError::Corrupt { .. } => ErrorKind::Corrupt,
        RelayError::NewerData => ErrorKind::NewerData,
        RelayError::Storage { .. } => ErrorKind::Internal,
        RelayError::Stopped { kind } => kind,
    }
}

impl From<Stopped> for RelayError {
    fn from(stopped: Stopped) -> Self {
        Self::Stopped { kind: stopped.kind }
    }
}

impl From<DocumentError> for RelayError {
    fn from(err: DocumentError) -> Self {
        match err {
            DocumentError::Unreadable { reason } => Self::Corrupt { reason },
            DocumentError::Newer { .. } => Self::NewerData,
            DocumentError::Io { reason } => Self::Storage { reason },
        }
    }
}

impl From<FileStoreError> for RelayError {
    fn from(err: FileStoreError) -> Self {
        Self::File {
            reason: err.to_string(),
        }
    }
}

impl From<OpenError> for RelayError {
    fn from(err: OpenError) -> Self {
        match err {
            OpenError::InUse { .. } => Self::InUse,
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

impl From<ConnError> for RelayError {
    fn from(err: ConnError) -> Self {
        match err {
            ConnError::Proto(forge_devproto::ProtoError::HandshakeFailed) => Self::Refused,
            other => Self::Connection {
                reason: other.to_string(),
            },
        }
    }
}

impl From<std::io::Error> for RelayError {
    fn from(err: std::io::Error) -> Self {
        Self::Connection {
            reason: err.to_string(),
        }
    }
}

/// Trims a device name and checks its length.
pub fn clean_name(name: &str) -> Result<String, RelayError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(RelayError::EmptyName);
    }
    if name.chars().count() > MAX_NAME_CHARS as usize {
        return Err(RelayError::NameTooLong {
            max: MAX_NAME_CHARS,
        });
    }
    Ok(name.to_string())
}

/// Trims a note and checks its length.
pub fn clean_note(text: &str) -> Result<String, RelayError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(RelayError::EmptyNote);
    }
    if text.chars().count() > MAX_NOTE_CHARS as usize {
        return Err(RelayError::NoteTooLong {
            max: MAX_NOTE_CHARS,
        });
    }
    Ok(text.to_string())
}
