//! An app's saved state as one versioned JSON document, with the rules
//! every app needs and none should rewrite (docs/app-foundations.md):
//!
//! - **Change a copy, save, then keep it.** [`Document::change`] edits a
//!   copy and replaces the value only once the save succeeded, which is
//!   what [`crate::Runtime::command`] requires of a failed command.
//! - **Never destroy what can't be read.** A file this build can't read is
//!   reported, not replaced. A file from a newer version of the app is
//!   never touched. An unreadable one is set aside (renamed, kept) only when
//!   the person asks to start again ([`Document::start_fresh`]).
//! - **Old files keep opening.** Additive changes use `#[serde(default)]`;
//!   a change old files can't be read as raises the version and adds a step
//!   to the migration ([`Document::open_with_migration`]).
//!
//! The document is the app's choice of storage, not the runtime's: an app
//! with a database or no data at all doesn't use it.
//!
//! ```ignore
//! let directory = DataDirectory::open(&data_directory)?;
//! let notes = Document::<Notes>::open(directory.path().join("notes.json"), 1)?;
//! let rt = Runtime::with_directory("notes", directory, notes);
//! rt.command("add", move |doc| doc.change(|notes| notes.add(text))).await
//! ```

use std::path::{Path, PathBuf};

use forge_storage::{JsonFile, Migration, StorageError};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::contract::ErrorKind;

/// Why a document could not be opened or saved. Apps map it into their own
/// error (`impl From<DocumentError> for AppError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentError {
    /// The file exists but can't be read as this document. It is left as
    /// it is; [`Document::start_fresh`] sets it aside.
    Unreadable { reason: String },
    /// The file was written by a newer version of the app. It is never
    /// overwritten or set aside: updating the app opens it.
    Newer { found: u32, supported: u32 },
    /// Reading, writing or renaming failed (disk full, permissions).
    Io { reason: String },
}

impl DocumentError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            DocumentError::Unreadable { .. } => ErrorKind::Corrupt,
            DocumentError::Newer { .. } => ErrorKind::NewerData,
            DocumentError::Io { .. } => ErrorKind::Internal,
        }
    }

    /// Whether starting again with an empty document is allowed: only for
    /// an unreadable file, never for a newer one.
    pub fn can_start_fresh(&self) -> bool {
        matches!(self, DocumentError::Unreadable { .. })
    }
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocumentError::Unreadable { reason } => write!(f, "the saved data can't be read: {reason}"),
            DocumentError::Newer { found, supported } => write!(
                f,
                "the saved data is from a newer version (schema {found}, this build reads up to {supported})"
            ),
            DocumentError::Io { reason } => write!(f, "storage error: {reason}"),
        }
    }
}

impl std::error::Error for DocumentError {}

impl From<StorageError> for DocumentError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::CorruptData { reason } => DocumentError::Unreadable { reason },
            StorageError::UnsupportedSchemaVersion { found, supported } => {
                DocumentError::Newer { found, supported }
            }
            other => DocumentError::Io {
                reason: other.to_string(),
            },
        }
    }
}

/// A value of `T` kept in one JSON file. Starts as `T::default()` when
/// there is no file yet.
pub struct Document<T> {
    file: JsonFile<T>,
    value: T,
}

impl<T> Document<T>
where
    T: Serialize + DeserializeOwned + Clone + Default,
{
    /// Opens the document at `path`, written with schema `version`. Files
    /// with an older version are refused as unreadable; use
    /// [`Self::open_with_migration`] once there has been a second version.
    pub fn open(path: impl Into<PathBuf>, version: u32) -> Result<Self, DocumentError> {
        Self::from_file(JsonFile::new(path, version))
    }

    /// Opens the document, upgrading files from older versions with
    /// `migrate(from_version, data)`.
    pub fn open_with_migration(
        path: impl Into<PathBuf>,
        version: u32,
        migrate: Migration,
    ) -> Result<Self, DocumentError> {
        Self::from_file(JsonFile::new(path, version).with_migration(migrate))
    }

    /// After the person agreed to start again: sets an unreadable file aside
    /// (see [`set_aside`]) and opens an empty document. Refuses, and
    /// changes nothing, when the file is from a newer version. `now_ms`
    /// names the kept copy.
    pub fn start_fresh(
        path: impl Into<PathBuf>,
        version: u32,
        migrate: Option<Migration>,
        now_ms: i64,
    ) -> Result<(Self, Option<PathBuf>), DocumentError> {
        let path = path.into();
        let mut file = JsonFile::<T>::new(&path, version);
        if let Some(migrate) = migrate {
            file = file.with_migration(migrate);
        }
        match file.load() {
            Err(StorageError::UnsupportedSchemaVersion { found, supported }) => {
                return Err(DocumentError::Newer { found, supported })
            }
            Ok(Some(_)) | Ok(None) => return Ok((Self::from_file(file)?, None)),
            Err(_) => {}
        }
        let kept = set_aside(&path, now_ms)?;
        let fresh = JsonFile::<T>::new(&path, version);
        let fresh = match migrate {
            Some(m) => fresh.with_migration(m),
            None => fresh,
        };
        Ok((Self::from_file(fresh)?, kept))
    }

    fn from_file(file: JsonFile<T>) -> Result<Self, DocumentError> {
        if let Some(dir) = file.path().parent() {
            std::fs::create_dir_all(dir).map_err(|e| DocumentError::Io {
                reason: format!("create {}: {e}", dir.display()),
            })?;
        }
        let value = file.load()?.unwrap_or_default();
        Ok(Self { file, value })
    }

    /// The current value: what is on disk.
    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// Edits a copy, saves it, then keeps it. If `edit` fails or the save
    /// fails, the document is as it was and the error is returned, so a
    /// command built on it leaves the state untouched when it fails.
    pub fn change<R, E>(&mut self, edit: impl FnOnce(&mut T) -> Result<R, E>) -> Result<R, E>
    where
        E: From<DocumentError>,
    {
        let mut next = self.value.clone();
        let result = edit(&mut next)?;
        self.file.save(&next).map_err(DocumentError::from)?;
        self.value = next;
        Ok(result)
    }

    /// Saves `value` in place of the current one (erasing, importing).
    pub fn replace(&mut self, value: T) -> Result<(), DocumentError> {
        self.file.save(&value)?;
        self.value = value;
        Ok(())
    }

    /// The size of the file on disk, in bytes (0 before the first save).
    pub fn bytes(&self) -> u64 {
        std::fs::metadata(self.file.path())
            .map(|m| m.len())
            .unwrap_or(0)
    }
}

/// Renames the file at `path` to `<stem>.unreadable-<now_ms>.<ext>` next to
/// it, so an app can start again without destroying it, keeping only the
/// newest three such copies ([`forge_storage::set_aside`]). Returns the kept
/// copy's path, or `None` when there was no file.
pub fn set_aside(path: &Path, now_ms: i64) -> Result<Option<PathBuf>, DocumentError> {
    forge_storage::set_aside(path, now_ms).map_err(|e| DocumentError::Io {
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
    struct Notes {
        items: Vec<String>,
        #[serde(default)]
        pinned: bool,
    }

    #[derive(Debug, PartialEq)]
    enum AppError {
        Empty,
        Doc(DocumentError),
    }

    impl From<DocumentError> for AppError {
        fn from(e: DocumentError) -> Self {
            AppError::Doc(e)
        }
    }

    fn add(doc: &mut Document<Notes>, text: &str) -> Result<usize, AppError> {
        doc.change(|notes| {
            if text.is_empty() {
                return Err(AppError::Empty);
            }
            notes.items.push(text.into());
            Ok(notes.items.len())
        })
    }

    #[test]
    fn starts_empty_saves_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/notes.json");
        let mut doc = Document::<Notes>::open(&path, 1).unwrap();
        assert_eq!(doc.value(), &Notes::default());
        assert_eq!(doc.bytes(), 0);
        assert_eq!(add(&mut doc, "milk"), Ok(1));
        assert!(doc.bytes() > 0);
        let again = Document::<Notes>::open(&path, 1).unwrap();
        assert_eq!(again.value().items, vec!["milk".to_string()]);
    }

    #[test]
    fn a_refused_change_leaves_value_and_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        let mut doc = Document::<Notes>::open(&path, 1).unwrap();
        add(&mut doc, "milk").unwrap();
        let before = std::fs::read(&path).unwrap();
        assert_eq!(add(&mut doc, ""), Err(AppError::Empty));
        assert_eq!(doc.value().items.len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn a_failed_save_keeps_the_old_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        let mut doc = Document::<Notes>::open(&path, 1).unwrap();
        add(&mut doc, "milk").unwrap();
        // Another build wrote a newer file meanwhile: never overwritten.
        std::fs::write(&path, r#"{"schema_version": 9, "data": {}}"#).unwrap();
        assert!(matches!(add(&mut doc, "eggs"), Err(AppError::Doc(_))));
        assert_eq!(doc.value().items, vec!["milk".to_string()]);
    }

    #[test]
    fn newer_files_are_refused_and_never_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        let newer = r#"{"schema_version": 3, "data": {}}"#;
        std::fs::write(&path, newer).unwrap();
        let err = Document::<Notes>::open(&path, 1).err().unwrap();
        assert_eq!(
            err,
            DocumentError::Newer {
                found: 3,
                supported: 1
            }
        );
        assert_eq!(err.kind(), ErrorKind::NewerData);
        assert!(!err.can_start_fresh());
        assert!(Document::<Notes>::start_fresh(&path, 1, None, 5).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);
    }

    #[test]
    fn unreadable_files_are_kept_when_starting_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        std::fs::write(&path, "{ truncated").unwrap();
        let err = Document::<Notes>::open(&path, 1).err().unwrap();
        assert!(err.can_start_fresh());
        assert_eq!(err.kind(), ErrorKind::Corrupt);

        let (mut doc, kept) = Document::<Notes>::start_fresh(&path, 1, None, 42).unwrap();
        let kept = kept.unwrap();
        assert_eq!(kept.file_name().unwrap(), "notes.unreadable-42.json");
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "{ truncated");
        assert_eq!(doc.value(), &Notes::default());
        add(&mut doc, "fresh").unwrap();

        // Starting fresh on a readable file changes nothing.
        let (doc, kept) = Document::<Notes>::start_fresh(&path, 1, None, 43).unwrap();
        assert_eq!(kept, None);
        assert_eq!(doc.value().items, vec!["fresh".to_string()]);
    }

    #[test]
    fn older_files_migrate_and_fields_added_later_default() {
        fn migrate(from: u32, data: serde_json::Value) -> Result<serde_json::Value, StorageError> {
            assert_eq!(from, 1);
            // Version 1 kept a single "note".
            let note = data["note"].as_str().unwrap_or_default().to_string();
            Ok(serde_json::json!({ "items": [note] }))
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        std::fs::write(&path, r#"{"schema_version": 1, "data": {"note": "old"}}"#).unwrap();
        assert!(
            Document::<Notes>::open(&path, 2).is_err(),
            "no migration, no guess"
        );
        let doc = Document::<Notes>::open_with_migration(&path, 2, migrate).unwrap();
        assert_eq!(doc.value().items, vec!["old".to_string()]);
        assert!(!doc.value().pinned);
    }

    #[test]
    fn replace_erases() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        let mut doc = Document::<Notes>::open(&path, 1).unwrap();
        add(&mut doc, "milk").unwrap();
        doc.replace(Notes::default()).unwrap();
        assert_eq!(
            Document::<Notes>::open(&path, 1).unwrap().value(),
            &Notes::default()
        );
    }
}
