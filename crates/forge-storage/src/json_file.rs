//! A typed JSON file for an app's own data.
//!
//! ```json
//! { "schema_version": 1, "data": { ... } }
//! ```
//!
//! Writes are atomic ([`write_atomic`]). A file written by a newer build
//! (higher `schema_version`) is reported as
//! [`StorageError::UnsupportedSchemaVersion`] and never overwritten by
//! [`JsonFile::save`]; anything unreadable is [`StorageError::CorruptData`].
//! Older versions are handed to the app's migration function.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::atomic::write_atomic;
use crate::StorageError;

/// Upgrades the `data` of an older `schema_version` to the current shape.
pub type Migration = fn(from_version: u32, data: Value) -> Result<Value, StorageError>;

/// One JSON document on disk, holding a `T`.
#[derive(Debug, Clone)]
pub struct JsonFile<T> {
    path: PathBuf,
    version: u32,
    migrate: Option<Migration>,
    _type: std::marker::PhantomData<fn() -> T>,
}

impl<T: Serialize + DeserializeOwned> JsonFile<T> {
    /// `path` is the file; `version` is the schema version this build writes.
    pub fn new(path: impl Into<PathBuf>, version: u32) -> Self {
        Self {
            path: path.into(),
            version,
            migrate: None,
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

    /// Writes `value` atomically, creating the directory if needed. Refuses
    /// to replace a file written by a newer build.
    pub fn save(&self, value: &T) -> Result<(), StorageError> {
        if let Err(err @ StorageError::UnsupportedSchemaVersion { .. }) = self.load() {
            return Err(err);
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|err| StorageError::WriteFailed {
                reason: format!("create {}: {err}", dir.display()),
            })?;
        }
        let json = serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": self.version,
            "data": value,
        }))
        .map_err(|err| StorageError::WriteFailed {
            reason: err.to_string(),
        })?;
        write_atomic(&self.path, &json)
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
