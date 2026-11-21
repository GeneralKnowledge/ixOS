//! The types Relay's API speaks in, and its errors.

use forge_app::{DocumentError, ErrorKind, OpenError, Stopped};
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
    pub name: String,
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
    /// Queued for a trusted device that hasn't been reached yet: it goes
    /// when the device is seen again, while Relay is open.
    Waiting,
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
    /// A transfer code (`7-acid-tidy-rug`) that can't be right as typed.
    #[error("{reason}")]
    WordCode { reason: String },
    #[error("{id} isn't a trusted device")]
    UnknownPeer { id: String },
    #[error("nothing with id {id}")]
    NotFound { id: u64 },
    #[error("can't reach the other device: {reason}")]
    Unreachable { reason: String },
    #[error("the other device didn't accept the connection")]
    Refused,
    #[error("the other device declined: {reason}")]
    Declined { reason: String },
    #[error("the connection broke: {reason}")]
    Connection { reason: String },
    #[error("the file can't be read or kept: {reason}")]
    File { reason: String },
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

impl RelayError {
    /// Why a transfer failed, in words for its row in the history. The
    /// diagnostic inside (an OS error, a protocol detail) goes to the log
    /// instead: it means nothing on the screen.
    pub fn saved_reason(&self) -> String {
        let words = match self {
            RelayError::Unreachable { .. } => "Couldn't reach the other device",
            RelayError::Connection { .. } => "The connection broke",
            RelayError::File { .. } => "The file couldn't be read or saved",
            RelayError::Storage { .. } | RelayError::Corrupt { .. } => {
                "Relay couldn't save it on this device"
            }
            RelayError::Stopped {
                kind: ErrorKind::Cancelled,
            } => "Cancelled",
            RelayError::Stopped { .. } => "Stopped before it finished",
            // The rest are already plain words (a declined reason is the
            // other person's, a word code's says what to fix).
            other => {
                let text = other.to_string();
                let mut chars = text.chars();
                return match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => text,
                };
            }
        };
        forge_runtime::logging::info!("relay transfer failed: {self}");
        words.to_string()
    }
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
        | RelayError::WordCode { .. }
        | RelayError::File { .. } => ErrorKind::Invalid,
        // The other device said no: pairing again (or asking its owner)
        // is the way on, not trying again.
        RelayError::Refused | RelayError::Declined { .. } => ErrorKind::Refused,
        RelayError::UnknownPeer { .. } | RelayError::NotFound { .. } => ErrorKind::NotFound,
        RelayError::Unreachable { .. } | RelayError::Connection { .. } => ErrorKind::Network,
        RelayError::InUse => ErrorKind::Unavailable,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_failures_and_refusals_have_their_own_kinds() {
        let reason = String::new;
        assert_eq!(
            relay_error_kind(RelayError::Unreachable { reason: reason() }),
            ErrorKind::Network
        );
        assert_eq!(
            relay_error_kind(RelayError::Connection { reason: reason() }),
            ErrorKind::Network
        );
        assert_eq!(relay_error_kind(RelayError::Refused), ErrorKind::Refused);
        assert_eq!(
            relay_error_kind(RelayError::Declined { reason: reason() }),
            ErrorKind::Refused
        );
    }
}
