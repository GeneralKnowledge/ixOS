//! A typed JSON file for an app's own data.
//!
//! ```json
//! { "schema_version": 1, "data": { ... } }
//! ```
//!
//! Writes are atomic ([`write_atomic`]). A file written by a newer build
//! (higher `schema_version`) is reported as
//! [`StorageError::UnsupportedSchemaVersion`] and never overwritten by
//! [`JsonFile::save`]; anything unreadable is [`StorageError::CorruptData`],
//! and `save` does not replace it either unless the app asks
//! ([`JsonFile::save_over_corrupt`]).
//! Older versions are handed to the app's migration function.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::atomic::write_atomic;
use crate::StorageError;

/// Upgrades the `data` of an older `schema_version` to the current shape.
pub type Migration = fn(from_version: u32, data: Value) -> Result<Value, StorageError>;

/// One JSON document on disk, holding a `T`.
///
/// Clones share what the file was last seen to hold, so a clone saves
/// without re-reading the file as well.
#[derive(Debug, Clone)]
pub struct JsonFile<T> {
    path: PathBuf,
    version: u32,
    migrate: Option<Migration>,
    known: Arc<Mutex<Known>>,
    _type: std::marker::PhantomData<fn() -> T>,
}

/// A file's size and modification time.
type Stamp = (u64, Option<std::time::SystemTime>);

/// What this `JsonFile` last saw on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Known {
    /// Not read or written yet, or the last read failed: check before writing.
    Unchecked,
    /// Missing, or a document this build can read: safe to replace while
    /// the file still has this size and modification time (`None`: absent).
    Ours(Option<Stamp>),
    /// Unreadable: never replaced by `save`.
    Corrupt,
    /// Written by a newer build: never replaced.
    Newer,
}

impl<T: Serialize + DeserializeOwned> JsonFile<T> {
    /// `path` is the file; `version` is the schema version this build writes.
    pub fn new(path: impl Into<PathBuf>, version: u32) -> Self {
        Self {
            path: path.into(),
            version,
            migrate: None,
            known: Arc::new(Mutex::new(Known::Unchecked)),
            _type: std::marker::PhantomData,
        }
    }

    /// Called for files with a lower `schema_version` than this build's.
    /// Without one, older files are reported as corrupt.
    pub fn with_migration(mut self, migrate: Migration) -> Self {
        self.migrate = Some(migrate);
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored value, or `None` if the file does not exist yet.
    pub fn load(&self) -> Result<Option<T>, StorageError> {
        let result = self.read();
        self.remember(match &result {
            Ok(_) => Known::Ours(self.stamp()),
            Err(StorageError::CorruptData { .. }) => Known::Corrupt,
            Err(StorageError::UnsupportedSchemaVersion { .. }) => Known::Newer,
            Err(_) => Known::Unchecked,
        });
        result
    }

    fn remember(&self, known: Known) {
        *self.known.lock().unwrap_or_else(|p| p.into_inner()) = known;
    }

    /// The file's size and modification time, or `None` when it is absent.
    fn stamp(&self) -> Option<Stamp> {
        let meta = std::fs::metadata(&self.path).ok()?;
        Some((meta.len(), meta.modified().ok()))
    }

    fn known(&self) -> Known {
        *self.known.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn read(&self) -> Result<Option<T>, StorageError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(StorageError::ReadFailed {
                    reason: format!("{}: {err}", self.path.display()),
                })
            }
        };
        let corrupt = |reason: String| StorageError::CorruptData { reason };
        let mut envelope: Value =
            serde_json::from_str(&text).map_err(|e| corrupt(e.to_string()))?;
        let found = envelope
            .get("schema_version")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| corrupt("missing schema_version".into()))?;
        if found > self.version {
            return Err(StorageError::UnsupportedSchemaVersion {
                found,
                supported: self.version,
            });
        }
        let mut data = envelope
            .get_mut("data")
            .map(Value::take)
            .ok_or_else(|| corrupt("missing data".into()))?;
        if found < self.version {
            let migrate = self
                .migrate
                .ok_or_else(|| corrupt(format!("no migration from schema_version {found}")))?;
            data = migrate(found, data)?;
        }
        serde_json::from_value(data)
            .map(Some)
            .map_err(|e| corrupt(e.to_string()))
    }

    /// Writes `value` atomically, creating the directory if needed.
    ///
    /// Refuses to replace a file written by a newer build
    /// (`UnsupportedSchemaVersion`) or one that could not be read
    /// (`CorruptData`): an unreadable file may still be recoverable, so
    /// replacing it is the app's explicit decision ([`Self::save_over_corrupt`]).
    /// The file is read before the first write, and again only when its
    /// size or modification time changed since this `JsonFile` last loaded
    /// or saved it; otherwise saving costs one `stat` on top of the write.
    pub fn save(&self, value: &T) -> Result<(), StorageError> {
        if self.known() != Known::Ours(self.stamp()) {
            // Unchecked, changed by someone else since we last saw it, or
            // something was wrong last time (the app may have moved a bad
            // file aside since): look again. Otherwise one `stat` replaces
            // reading and parsing the whole file.
            self.load()?;
        }
        self.write(value)
    }

    /// Like [`Self::save`], but replaces a file that could not be read.
    /// For an app's explicit "start again" after telling the user; it still
    /// refuses to replace a file from a newer build.
    pub fn save_over_corrupt(&self, value: &T) -> Result<(), StorageError> {
        match self.load() {
            Ok(_) | Err(StorageError::CorruptData { .. }) => self.write(value),
            Err(err) => Err(err),
        }
    }

    fn write(&self, value: &T) -> Result<(), StorageError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|err| StorageError::WriteFailed {
                reason: format!("create {}: {err}", dir.display()),
            })?;
        }
        // Serialised straight from `value`, without building a `Value` tree.
        #[derive(Serialize)]
        struct Envelope<'a, T> {
            schema_version: u32,
            data: &'a T,
        }
        let json = serde_json::to_vec_pretty(&Envelope {
            schema_version: self.version,
            data: value,
        })
        .map_err(|err| StorageError::WriteFailed {
            reason: err.to_string(),
        })?;
        write_atomic(&self.path, &json)?;
        self.remember(Known::Ours(self.stamp()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Notes {
        items: Vec<String>,
    }

    fn file(dir: &Path, version: u32) -> JsonFile<Notes> {
        JsonFile::new(dir.join("app/notes.json"), version)
    }

    #[test]
    fn missing_file_loads_as_none_then_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let notes = file(dir.path(), 1);
        assert_eq!(notes.load().unwrap(), None);
        let value = Notes {
            items: vec!["a".into(), "b".into()],
        };
        notes.save(&value).unwrap();
        assert_eq!(notes.load().unwrap(), Some(value));
    }

    #[test]
    fn newer_file_is_reported_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        file(dir.path(), 2).save(&Notes { items: vec![] }).unwrap();
        let old_build = file(dir.path(), 1);
        assert_eq!(
            old_build.load(),
            Err(StorageError::UnsupportedSchemaVersion {
                found: 2,
                supported: 1
            })
        );
        assert!(old_build.save(&Notes { items: vec![] }).is_err());
        assert!(file(dir.path(), 2).load().is_ok());
    }

    #[test]
    fn garbage_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let notes = file(dir.path(), 1);
        std::fs::create_dir_all(notes.path().parent().unwrap()).unwrap();
        std::fs::write(notes.path(), "{not json").unwrap();
        assert!(matches!(
            notes.load(),
            Err(StorageError::CorruptData { .. })
        ));
    }

    #[test]
    fn a_corrupt_file_is_replaced_only_on_request() {
        let dir = tempfile::tempdir().unwrap();
        let notes = file(dir.path(), 1);
        std::fs::create_dir_all(notes.path().parent().unwrap()).unwrap();
        std::fs::write(notes.path(), "{half a docu").unwrap();
        let value = Notes {
            items: vec!["new".into()],
        };
        assert!(matches!(
            notes.save(&value),
            Err(StorageError::CorruptData { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(notes.path()).unwrap(),
            "{half a docu"
        );
        notes.save_over_corrupt(&value).unwrap();
        assert_eq!(notes.load().unwrap(), Some(value));
    }

    #[test]
    fn a_file_moved_aside_after_a_failed_load_can_be_saved_again() {
        let dir = tempfile::tempdir().unwrap();
        let notes = file(dir.path(), 1);
        std::fs::create_dir_all(notes.path().parent().unwrap()).unwrap();
        std::fs::write(notes.path(), "garbage").unwrap();
        assert!(notes.load().is_err());
        std::fs::rename(notes.path(), dir.path().join("aside")).unwrap();
        notes.save(&Notes { items: vec![] }).unwrap();
    }

    #[test]
    fn a_file_changed_behind_its_back_is_checked_again() {
        let dir = tempfile::tempdir().unwrap();
        let notes = file(dir.path(), 1);
        notes.save(&Notes { items: vec![] }).unwrap();
        std::fs::write(notes.path(), "garbage, and longer than before").unwrap();
        assert!(matches!(
            notes.save(&Notes {
                items: vec!["x".into()]
            }),
            Err(StorageError::CorruptData { .. })
        ));
    }

    #[test]
    fn older_files_go_through_the_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        std::fs::write(&path, r#"{"schema_version":1,"data":["x"]}"#).unwrap();
        let notes = JsonFile::<Notes>::new(&path, 2).with_migration(|from, data| {
            assert_eq!(from, 1);
            Ok(serde_json::json!({ "items": data }))
        });
        assert_eq!(
            notes.load().unwrap(),
            Some(Notes {
                items: vec!["x".into()]
            })
        );
        assert!(matches!(
            JsonFile::<Notes>::new(&path, 2).load(),
            Err(StorageError::CorruptData { .. })
        ));
    }
}
