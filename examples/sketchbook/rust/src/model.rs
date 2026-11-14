//! The types Sketchbook's API speaks in, and its errors.

use forge_app::{DocumentError, ErrorKind, OpenError, Stopped};
use forge_storage::FileStoreError;
use serde::{Deserialize, Serialize};

/// The longest sketch or layer name, in characters.
pub const MAX_NAME_CHARS: u32 = 60;
/// The most layers a sketch can have. Each visible layer is a PencilKit
/// canvas on screen, and eight is plenty for finger drawing.
pub const MAX_LAYERS: u32 = 8;
/// How many steps undo remembers per sketch.
pub const MAX_UNDO_STEPS: u32 = 100;

/// What the page looks like under the drawing.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Paper {
    #[default]
    Plain,
    Lined,
    Grid,
    Dots,
}

/// One layer of a sketch, bottom first in [`SketchDetail::layers`].
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    /// From 0 (invisible) to 1.
    pub opacity: f64,
    /// The layer's PencilKit drawing (`PKDrawing.dataRepresentation()`), or
    /// `None` while nothing is drawn on it. Absolute; valid until the next
    /// change to this layer.
    pub drawing_path: Option<String>,
}

/// A sketch as the list shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SketchSummary {
    pub id: u64,
    pub title: String,
    /// Milliseconds since 1970.
    pub created_ms: i64,
    pub modified_ms: i64,
    pub paper: Paper,
    pub layer_count: u32,
    /// A PNG the app rendered after the last change, or `None`.
    pub thumbnail_path: Option<String>,
}

/// A sketch with everything the drawing screen needs.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SketchDetail {
    pub summary: SketchSummary,
    /// Bottom first.
    pub layers: Vec<Layer>,
    /// What Undo would undo, in words for its menu ("Undo Drawing"), or
    /// `None` when there is nothing to undo.
    pub undo_name: Option<String>,
    pub redo_name: Option<String>,
}

/// The file kinds a sketch exports to. PencilKit renders them; Sketchbook
/// plans what goes on each page.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    /// One image per sketch.
    Png,
    /// One document, a page per sketch.
    Pdf,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Png => "png",
            ExportFormat::Pdf => "pdf",
        }
    }
}

/// One layer to draw onto an exported page, bottom first.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ExportLayer {
    pub drawing_path: String,
    pub opacity: f64,
}

/// One page of an export: a sketch's visible, non-empty layers.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ExportPage {
    pub sketch_id: u64,
    pub title: String,
    pub paper: Paper,
    pub layers: Vec<ExportLayer>,
}

/// What to render and where: the app draws each page with PencilKit and
/// writes `paths[i]` (PNG, one per page) or `paths[0]` (PDF, every page).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ExportPlan {
    pub format: ExportFormat,
    pub pages: Vec<ExportPage>,
    pub paths: Vec<String>,
}

/// Everything that can go wrong, in the app's words. `sketchbook_error_kind`
/// maps each case to the framework's [`ErrorKind`].
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum SketchbookError {
    #[error("a name can't be empty")]
    EmptyName,
    #[error("names are at most {max} characters")]
    NameTooLong { max: u32 },
    #[error("nothing with id {id}")]
    NotFound { id: u64 },
    #[error("a sketch has at most {max} layers")]
    TooManyLayers { max: u32 },
    #[error("a sketch keeps at least one layer")]
    LastLayer,
    #[error("opacity goes from 0 to 1")]
    BadOpacity,
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
    #[error("choose at least one sketch")]
    NothingToExport,
    #[error("the drawing can't be kept: {reason}")]
    Drawing { reason: String },
    #[error("Sketchbook's data is open in another copy of the app")]
    InUse,
    #[error("the saved sketches can't be read: {reason}")]
    Corrupt { reason: String },
    #[error("the sketches were saved by a newer version of the app")]
    NewerData,
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// Stopped by the runtime: cancelled, interrupted by the background,
    /// or shut down. `kind` says which.
    #[error("stopped ({kind:?})")]
    Stopped { kind: ErrorKind },
}

/// The framework kind of a Sketchbook error.
#[uniffi::export]
pub fn sketchbook_error_kind(error: SketchbookError) -> ErrorKind {
    match error {
        SketchbookError::EmptyName
        | SketchbookError::NameTooLong { .. }
        | SketchbookError::TooManyLayers { .. }
        | SketchbookError::LastLayer
        | SketchbookError::BadOpacity
        | SketchbookError::NothingToUndo
        | SketchbookError::NothingToRedo
        | SketchbookError::NothingToExport
        | SketchbookError::Drawing { .. } => ErrorKind::Invalid,
        SketchbookError::NotFound { .. } => ErrorKind::NotFound,
        SketchbookError::InUse => ErrorKind::Unavailable,
        SketchbookError::Corrupt { .. } => ErrorKind::Corrupt,
        SketchbookError::NewerData => ErrorKind::NewerData,
        SketchbookError::Storage { .. } => ErrorKind::Internal,
        SketchbookError::Stopped { kind } => kind,
    }
}

impl From<Stopped> for SketchbookError {
    fn from(stopped: Stopped) -> Self {
        Self::Stopped { kind: stopped.kind }
    }
}

impl From<DocumentError> for SketchbookError {
    fn from(err: DocumentError) -> Self {
        match err {
            DocumentError::Unreadable { reason } => Self::Corrupt { reason },
            DocumentError::Newer { .. } => Self::NewerData,
            DocumentError::Io { reason } => Self::Storage { reason },
        }
    }
}

impl From<FileStoreError> for SketchbookError {
    fn from(err: FileStoreError) -> Self {
        match err {
            FileStoreError::Io { .. } => Self::Storage {
                reason: err.to_string(),
            },
            other => Self::Drawing {
                reason: other.to_string(),
            },
        }
    }
}

impl From<OpenError> for SketchbookError {
    fn from(err: OpenError) -> Self {
        match err {
            OpenError::InUse { .. } => Self::InUse,
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

/// Trims a sketch or layer name and checks its length.
pub fn clean_name(name: &str) -> Result<String, SketchbookError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(SketchbookError::EmptyName);
    }
    if name.chars().count() > MAX_NAME_CHARS as usize {
        return Err(SketchbookError::NameTooLong {
            max: MAX_NAME_CHARS,
        });
    }
    Ok(name.to_string())
}
