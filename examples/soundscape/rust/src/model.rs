//! Soundscape's errors, and the name rule playlists share.

use forge_app::ErrorKind;
use forge_storage::FileStoreError;

/// The longest playlist name, in characters.
pub const MAX_NAME_CHARS: u32 = 60;

/// Everything that can go wrong, in the app's words.
/// `soundscape_error_kind` maps each case to the framework's [`ErrorKind`].
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum SoundscapeError {
    #[error("a name can't be empty")]
    EmptyName,
    #[error("names are at most {max} characters")]
    NameTooLong { max: u32 },
    #[error("{setting} is out of range")]
    OutOfRange { setting: String },
    #[error("the file can't be used: {reason}")]
    Audio { reason: String },
    #[error("Soundscape's data is open in another copy of the app")]
    InUse,
    #[error("the saved data can't be read: {reason}")]
    Corrupt { reason: String },
    #[error("the data was saved by a newer version of the app")]
    NewerData,
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// Listen: the typed server address can't be used.
    #[error("{reason}")]
    BadAddress { reason: String },
    /// Listen: the take server couldn't be reached.
    #[error("can't reach the take server: {reason}")]
    Unreachable { reason: String },
    /// Listen: the address isn't usable, or the server answered with
    /// something other than what was asked.
    #[error("{reason}")]
    Server { reason: String },
    /// Library: no artist, album or track with that id (it may have gone
    /// from the server since the screen was opened).
    #[error("that isn't in the library any more")]
    NotInLibrary { id: String },
    /// Stopped by the runtime: cancelled, interrupted by the background,
    /// or shut down. `kind` says which.
    #[error("stopped ({kind:?})")]
    Stopped { kind: ErrorKind },
}

/// The framework kind of a Soundscape error.
#[uniffi::export]
pub fn soundscape_error_kind(error: SoundscapeError) -> ErrorKind {
    match error {
        SoundscapeError::EmptyName
        | SoundscapeError::NameTooLong { .. }
        | SoundscapeError::OutOfRange { .. }
        | SoundscapeError::Audio { .. }
        | SoundscapeError::BadAddress { .. } => ErrorKind::Invalid,
        SoundscapeError::NotInLibrary { .. } => ErrorKind::NotFound,
        SoundscapeError::Unreachable { .. } => ErrorKind::Network,
        SoundscapeError::Server { .. } => ErrorKind::Refused,
        SoundscapeError::InUse => ErrorKind::Unavailable,
        SoundscapeError::Corrupt { .. } => ErrorKind::Corrupt,
        SoundscapeError::NewerData => ErrorKind::NewerData,
        SoundscapeError::Storage { .. } => ErrorKind::Internal,
        SoundscapeError::Stopped { kind } => kind,
    }
}

impl SoundscapeError {
    pub(crate) fn range(setting: &str) -> Self {
        Self::OutOfRange {
            setting: setting.into(),
        }
    }
}

forge_app::app_error_conversions!(SoundscapeError);

impl From<FileStoreError> for SoundscapeError {
    fn from(err: FileStoreError) -> Self {
        match err {
            FileStoreError::Io { .. } => Self::Storage {
                reason: err.to_string(),
            },
            other => Self::Audio {
                reason: other.to_string(),
            },
        }
    }
}

/// Trims a name and checks its length.
pub fn clean_name(name: &str) -> Result<String, SoundscapeError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(SoundscapeError::EmptyName);
    }
    if name.chars().count() > MAX_NAME_CHARS as usize {
        return Err(SoundscapeError::NameTooLong {
            max: MAX_NAME_CHARS,
        });
    }
    Ok(name.to_string())
}
