//! The types Soundscape's API speaks in, and its errors.

use forge_app::{DocumentError, ErrorKind, OpenError, Stopped};
use forge_storage::FileStoreError;
use serde::{Deserialize, Serialize};

/// The longest project or track name, in characters.
pub const MAX_NAME_CHARS: u32 = 60;
/// The most tracks a project can have.
pub const MAX_TRACKS: u32 = 8;
/// Tempo limits, in beats per minute.
pub const MIN_TEMPO: f64 = 40.0;
pub const MAX_TEMPO: f64 = 240.0;
/// The longest loop, in bars of four beats.
pub const MAX_BARS: u32 = 16;
/// Track gain limits, in decibels.
pub const MIN_GAIN_DB: f64 = -60.0;
pub const MAX_GAIN_DB: f64 = 6.0;

/// A track's echo, in step with the tempo.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Echo {
    /// The time between repeats, in beats (0.25 is a sixteenth note at 4/4).
    pub beats: f64,
    /// How much of each repeat comes back, from 0 to 0.9.
    pub feedback: f64,
    /// How loud the repeats are against the dry sound, from 0 to 1.
    pub mix: f64,
}

/// What a track sounds like in the mix. Saved as the person set it;
/// [`TrackMix`] says what it means in AVAudioEngine's units.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct TrackSettings {
    /// Decibels, from -60 to +6.
    pub gain_db: f64,
    /// From -1 (left) to 1 (right).
    pub pan: f64,
    pub muted: bool,
    pub solo: bool,
    /// How bright the track is, from 0 (muffled) to 1 (untouched): a
    /// low-pass filter.
    pub tone: f64,
    pub echo: Option<Echo>,
}

impl Default for TrackSettings {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            solo: false,
            tone: 1.0,
            echo: None,
        }
    }
}

/// A recorded or imported loop.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Clip {
    /// The WAV file. Absolute; valid until the clip is replaced or removed.
    pub path: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub duration_ms: i64,
}

/// One track of a project, in the order the mixer shows them.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub clip: Option<Clip>,
    pub settings: TrackSettings,
    /// Whether this track is heard, given mute and every track's solo.
    pub audible: bool,
    /// The settings in AVAudioEngine's units, for live playback.
    pub mix: TrackMix,
}

/// A track's settings as AVAudioEngine takes them: `AVAudioPlayerNode`
/// volume and pan, an `AVAudioUnitEQ` low-pass band, an `AVAudioUnitDelay`.
/// The mixdown uses the same numbers, so what you hear is what you export.
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq)]
pub struct TrackMix {
    /// Linear gain, 0 when the track isn't audible.
    pub volume: f32,
    pub pan: f32,
    /// The low-pass cutoff in hertz, or `None` when tone is untouched.
    pub low_pass_hz: Option<f32>,
    /// `AVAudioUnitDelay.delayTime`, in seconds, or `None` without echo.
    pub delay_seconds: Option<f64>,
    /// `AVAudioUnitDelay.feedback`, in percent.
    pub delay_feedback_percent: f32,
    /// `AVAudioUnitDelay.wetDryMix`, in percent.
    pub delay_wet_dry_percent: f32,
}

/// A project as the list shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ProjectSummary {
    pub id: u64,
    pub title: String,
    pub created_ms: i64,
    pub modified_ms: i64,
    pub tempo_bpm: f64,
    pub bars: u32,
    pub track_count: u32,
}

/// A project with everything the mixer needs.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ProjectDetail {
    pub summary: ProjectSummary,
    /// How long one pass of the loop is, from tempo and bars (4/4).
    pub loop_seconds: f64,
    pub tracks: Vec<Track>,
}

/// What `mixdown` wrote.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Mixdown {
    pub path: String,
    pub sample_rate: u32,
    pub duration_ms: i64,
    /// The loudest sample before any limiting, in dBFS (0 is full scale).
    pub peak_db: f64,
    /// Whether the mix went over full scale and was turned down to fit.
    pub limited: bool,
}

/// Everything that can go wrong, in the app's words.
/// `soundscape_error_kind` maps each case to the framework's [`ErrorKind`].
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum SoundscapeError {
    #[error("a name can't be empty")]
    EmptyName,
    #[error("names are at most {max} characters")]
    NameTooLong { max: u32 },
    #[error("nothing with id {id}")]
    NotFound { id: u64 },
    #[error("a project has at most {max} tracks")]
    TooManyTracks { max: u32 },
    #[error("{setting} is out of range")]
    OutOfRange { setting: String },
    #[error("the recording can't be used: {reason}")]
    Audio { reason: String },
    #[error("there is nothing to hear: no track has a recording, or every one is muted")]
    Silent,
    #[error("Soundscape's data is open in another copy of the app")]
    InUse,
    #[error("the saved projects can't be read: {reason}")]
    Corrupt { reason: String },
    #[error("the projects were saved by a newer version of the app")]
    NewerData,
    #[error("storage error: {reason}")]
    Storage { reason: String },
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
        | SoundscapeError::TooManyTracks { .. }
        | SoundscapeError::OutOfRange { .. }
        | SoundscapeError::Audio { .. }
        | SoundscapeError::Silent => ErrorKind::Invalid,
        SoundscapeError::NotFound { .. } => ErrorKind::NotFound,
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

impl From<Stopped> for SoundscapeError {
    fn from(stopped: Stopped) -> Self {
        Self::Stopped { kind: stopped.kind }
    }
}

impl From<DocumentError> for SoundscapeError {
    fn from(err: DocumentError) -> Self {
        match err {
            DocumentError::Unreadable { reason } => Self::Corrupt { reason },
            DocumentError::Newer { .. } => Self::NewerData,
            DocumentError::Io { reason } => Self::Storage { reason },
        }
    }
}

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

impl From<OpenError> for SoundscapeError {
    fn from(err: OpenError) -> Self {
        match err {
            OpenError::InUse { .. } => Self::InUse,
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

impl From<hound::Error> for SoundscapeError {
    fn from(err: hound::Error) -> Self {
        match err {
            hound::Error::IoError(err) => Self::Storage {
                reason: err.to_string(),
            },
            other => Self::Audio {
                reason: other.to_string(),
            },
        }
    }
}

/// Trims a project or track name and checks its length.
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
