//! The types FieldKit's API speaks in, and its errors.

use forge_app::{ErrorKind, OpenError, Stopped};
use forge_storage::{FileStoreError, StorageError};

/// The longest trip title, in characters.
pub const MAX_TITLE_CHARS: u32 = 80;
/// The longest text note or caption, in characters.
pub const MAX_NOTE_CHARS: u32 = 4000;

/// A trip as the list shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct TripSummary {
    pub id: i64,
    pub title: String,
    /// Milliseconds since 1970.
    pub started_ms: i64,
    /// `None` while it is being recorded.
    pub ended_ms: Option<i64>,
    pub recording: bool,
    /// Along the kept track, in metres.
    pub distance_m: f64,
    /// Total ascent, in metres, ignoring wobbles under 3 m.
    pub climb_m: f64,
    pub point_count: u32,
    pub note_count: u32,
}

/// A place, without the details of a track point.
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub latitude: f64,
    pub longitude: f64,
}

/// What a note holds. File paths are absolute and valid until the note is
/// removed.
#[derive(uniffi::Enum, Clone, Debug, PartialEq)]
pub enum NoteKind {
    Text { body: String },
    Photo { path: String, caption: String },
    Voice { path: String, duration_ms: i64 },
}

/// Something noted along a trip.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Note {
    pub id: i64,
    pub trip_id: i64,
    pub created_ms: i64,
    /// Where it was taken: the trip's last kept point when the note was
    /// added during recording, or the place the app passed in.
    pub at: Option<Position>,
    pub kind: NoteKind,
}

/// A trip with its notes, for the trip screen. The track is fetched
/// separately (`track`), because it can be long.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct TripDetail {
    pub summary: TripSummary,
    pub notes: Vec<Note>,
}

/// What `record` did with a batch of fixes.
#[derive(uniffi::Record, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Recorded {
    pub kept: u32,
    /// Invalid or less accurate than 50 m.
    pub inaccurate: u32,
    /// Repeated, within 3 m of the last kept fix, or an impossible jump.
    pub skipped: u32,
}

/// Everything that can go wrong, in the app's words. `fieldkit_error_kind`
/// maps each case to the framework's [`ErrorKind`].
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum FieldKitError {
    #[error("a trip needs a title")]
    EmptyTitle,
    #[error("titles are at most {max} characters")]
    TitleTooLong { max: u32 },
    #[error("a note needs some text")]
    EmptyNote,
    #[error("notes are at most {max} characters")]
    NoteTooLong { max: u32 },
    #[error("nothing with id {id}")]
    NotFound { id: i64 },
    #[error("a trip is already being recorded")]
    AlreadyRecording,
    #[error("no trip is being recorded")]
    NotRecording,
    #[error("the file can't be added: {reason}")]
    File { reason: String },
    #[error("FieldKit's data is open in another copy of the app")]
    InUse,
    #[error("the saved trips can't be read: {reason}")]
    Corrupt { reason: String },
    #[error("the trips were saved by a newer version of the app")]
    NewerData,
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// Stopped by the runtime: cancelled, interrupted by the background,
    /// or shut down. `kind` says which.
    #[error("stopped ({kind:?})")]
    Stopped { kind: ErrorKind },
}

/// The framework kind of a FieldKit error.
#[uniffi::export]
pub fn fieldkit_error_kind(error: FieldKitError) -> ErrorKind {
    match error {
        FieldKitError::EmptyTitle
        | FieldKitError::TitleTooLong { .. }
        | FieldKitError::EmptyNote
        | FieldKitError::NoteTooLong { .. }
        | FieldKitError::AlreadyRecording
        | FieldKitError::NotRecording
        | FieldKitError::File { .. } => ErrorKind::Invalid,
        FieldKitError::NotFound { .. } => ErrorKind::NotFound,
        FieldKitError::InUse => ErrorKind::Unavailable,
        FieldKitError::Corrupt { .. } => ErrorKind::Corrupt,
        FieldKitError::NewerData => ErrorKind::NewerData,
        FieldKitError::Storage { .. } => ErrorKind::Internal,
        FieldKitError::Stopped { kind } => kind,
    }
}

impl From<Stopped> for FieldKitError {
    fn from(stopped: Stopped) -> Self {
        Self::Stopped { kind: stopped.kind }
    }
}

impl From<StorageError> for FieldKitError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::CorruptData { reason } => Self::Corrupt { reason },
            StorageError::UnsupportedSchemaVersion { .. } => Self::NewerData,
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

impl From<forge_storage::rusqlite::Error> for FieldKitError {
    fn from(err: forge_storage::rusqlite::Error) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

impl From<FileStoreError> for FieldKitError {
    fn from(err: FileStoreError) -> Self {
        match err {
            FileStoreError::Io { .. } => Self::Storage {
                reason: err.to_string(),
            },
            other => Self::File {
                reason: other.to_string(),
            },
        }
    }
}

impl From<OpenError> for FieldKitError {
    fn from(err: OpenError) -> Self {
        match err {
            OpenError::InUse { .. } => Self::InUse,
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

/// Trims a title and checks its length.
pub fn clean_title(title: &str) -> Result<String, FieldKitError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(FieldKitError::EmptyTitle);
    }
    if title.chars().count() > MAX_TITLE_CHARS as usize {
        return Err(FieldKitError::TitleTooLong {
            max: MAX_TITLE_CHARS,
        });
    }
    Ok(title.to_string())
}

/// Trims note text and checks its length. `required` refuses empty text
/// (a text note); a caption may be empty.
pub fn clean_note(text: &str, required: bool) -> Result<String, FieldKitError> {
    let text = text.trim();
    if required && text.is_empty() {
        return Err(FieldKitError::EmptyNote);
    }
    if text.chars().count() > MAX_NOTE_CHARS as usize {
        return Err(FieldKitError::NoteTooLong {
            max: MAX_NOTE_CHARS,
        });
    }
    Ok(text.to_string())
}
