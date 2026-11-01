use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use forge_core::CounterSnapshot;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Schema version written by this build.
///
/// Bump it when the envelope shape changes and add a step to [`migrate`] that
/// upgrades the previous version. Data with a *higher* version than this was
/// written by a newer build and is never treated as corrupt or overwritten.
pub const SCHEMA_VERSION: u32 = 1;

/// The snapshot filename. It must stay the same across schema versions so a
/// newer build finds (and migrates) data written by an older one.
const DEFAULT_FILENAME: &str = "forge-counter-v1.json";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StorageError {
    #[error("persistence directory is missing")]
    MissingDirectory,
    #[error("stored data is invalid or corrupted: {reason}")]
    CorruptData { reason: String },
    #[error("stored data uses schema version {found}, but this build supports up to {supported}")]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    #[error("failed to write persistence data: {reason}")]
    WriteFailed { reason: String },
    #[error("failed to read persistence data: {reason}")]
    ReadFailed { reason: String },
}

impl StorageError {
    fn corrupt(reason: impl ToString) -> Self {
        Self::CorruptData {
            reason: reason.to_string(),
        }
    }

    fn write(context: &str, err: impl std::fmt::Display) -> Self {
        Self::WriteFailed {
            reason: format!("{context}: {err}"),
        }
    }

    fn read(context: &str, err: impl std::fmt::Display) -> Self {
        Self::ReadFailed {
            reason: format!("{context}: {err}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotEnvelope {
    pub schema_version: u32,
    pub count: i64,
}

/// Just enough of any envelope version to decide how to read the rest.
#[derive(Deserialize)]
struct EnvelopeHeader {
    schema_version: u32,
}

impl SnapshotEnvelope {
    pub fn from_snapshot(snapshot: &CounterSnapshot) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            count: snapshot.count,
        }
    }

    /// Parses stored JSON of any known version, migrating older versions up to
    /// [`SCHEMA_VERSION`]. Newer versions fail with
    /// [`StorageError::UnsupportedSchemaVersion`], never `CorruptData`.
    pub fn decode(json: &str) -> Result<Self, StorageError> {
        let value: Value = serde_json::from_str(json).map_err(StorageError::corrupt)?;
        let header: EnvelopeHeader =
            serde_json::from_value(value.clone()).map_err(StorageError::corrupt)?;
        if header.schema_version > SCHEMA_VERSION {
            return Err(StorageError::UnsupportedSchemaVersion {
                found: header.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        let migrated = migrate(header.schema_version, value)?;
        serde_json::from_value(migrated).map_err(StorageError::corrupt)
    }

    pub fn into_snapshot(self) -> Result<CounterSnapshot, StorageError> {
        if self.schema_version > SCHEMA_VERSION {
            return Err(StorageError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        if self.schema_version != SCHEMA_VERSION {
            return Err(StorageError::corrupt(format!(
                "unexpected schema version {}",
                self.schema_version
            )));
        }
        let snap = CounterSnapshot { count: self.count };
        snap.validate().map_err(StorageError::corrupt)?;
        Ok(snap)
    }
}

/// Migration hook: upgrades a stored envelope from `version` to
/// [`SCHEMA_VERSION`], one step at a time.
///
/// When bumping the schema, add an arm for the previous version that rewrites
/// `value` into the next version's shape (including its `schema_version`) and
/// falls through to the next step. The upgraded data is written back on the
/// next successful save.
fn migrate(version: u32, value: Value) -> Result<Value, StorageError> {
    match version {
        SCHEMA_VERSION => Ok(value),
        // Example for a future v2:
        // 1 => migrate(2, v1_to_v2(value)?),
        other => Err(StorageError::corrupt(format!(
            "unknown schema version {other}"
        ))),
    }
}

#[derive(Debug, Clone)]
pub struct JsonSnapshotStore {
    directory: PathBuf,
    filename: String,
}

impl JsonSnapshotStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            filename: DEFAULT_FILENAME.to_string(),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.directory.join(&self.filename)
    }

    fn read_envelope(&self) -> Result<Option<SnapshotEnvelope>, StorageError> {
        let path = self.path();
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(StorageError::read("open snapshot", err)),
        };
        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .map_err(|err| StorageError::read("read snapshot", err))?;
        if buf.trim().is_empty() {
            return Err(StorageError::corrupt("snapshot file is empty"));
        }
        SnapshotEnvelope::decode(&buf).map(Some)
    }
}

/// Makes a completed rename durable by syncing the directory that holds it.
#[cfg(unix)]
fn sync_directory(dir: &Path) -> Result<(), StorageError> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|err| StorageError::write("sync directory", err))
}

#[cfg(not(unix))]
fn sync_directory(_dir: &Path) -> Result<(), StorageError> {
    // Directories cannot be opened for syncing on Windows; NTFS journals renames.
    Ok(())
}

impl super::SnapshotStore for JsonSnapshotStore {
    type Snapshot = CounterSnapshot;

    fn load(&self) -> Result<Option<Self::Snapshot>, StorageError> {
        if !self.directory.exists() {
            return Err(StorageError::MissingDirectory);
        }
        match self.read_envelope()? {
            None => Ok(None),
            Some(env) => env.into_snapshot().map(Some),
        }
    }

    fn save(&self, snapshot: &Self::Snapshot) -> Result<(), StorageError> {
        if !self.directory.exists() {
            fs::create_dir_all(&self.directory).map_err(|_| StorageError::MissingDirectory)?;
        }
        let envelope = SnapshotEnvelope::from_snapshot(snapshot);
        let data = serde_json::to_string_pretty(&envelope)
            .map_err(|err| StorageError::write("encode snapshot", err))?;
        let path = self.path();
        let tmp = path.with_extension("json.tmp");
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)
                .map_err(|err| StorageError::write("create temp file", err))?;
            file.write_all(data.as_bytes())
                .map_err(|err| StorageError::write("write temp file", err))?;
            file.sync_all()
                .map_err(|err| StorageError::write("sync temp file", err))?;
        }
        fs::rename(&tmp, &path).map_err(|err| StorageError::write("replace snapshot", err))?;
        sync_directory(&self.directory)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SnapshotStore;
    use forge_core::{CounterSnapshot, COUNTER_MAX};

    fn store_with_contents(contents: &str) -> (tempfile::TempDir, JsonSnapshotStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSnapshotStore::new(dir.path());
        fs::write(store.path(), contents).unwrap();
        (dir, store)
    }

    fn assert_corrupt(result: Result<Option<CounterSnapshot>, StorageError>) {
        match result {
            Err(StorageError::CorruptData { .. }) => {}
            other => panic!("expected CorruptData, got {other:?}"),
        }
    }

    #[test]
    fn save_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSnapshotStore::new(dir.path());
        let snap = CounterSnapshot { count: 42 };
        store.save(&snap).unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.count, 42);
    }

    #[test]
    fn missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSnapshotStore::new(dir.path());
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn missing_directory_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSnapshotStore::new(dir.path().join("absent"));
        assert_eq!(store.load().unwrap_err(), StorageError::MissingDirectory);
    }

    #[test]
    fn corrupt_json() {
        let (_dir, store) = store_with_contents("{not json");
        assert_corrupt(store.load());
    }

    #[test]
    fn empty_file_is_corrupt() {
        let (_dir, store) = store_with_contents("  \n");
        assert_corrupt(store.load());
    }

    #[test]
    fn missing_schema_version_is_corrupt() {
        let (_dir, store) = store_with_contents(r#"{"count": 3}"#);
        assert_corrupt(store.load());
    }

    #[test]
    fn out_of_range_count_is_corrupt() {
        let (_dir, store) = store_with_contents(&format!(
            r#"{{"schema_version": 1, "count": {}}}"#,
            COUNTER_MAX + 1
        ));
        assert_corrupt(store.load());
        let (_dir, store) = store_with_contents(r#"{"schema_version": 1, "count": -1}"#);
        assert_corrupt(store.load());
    }

    #[test]
    fn unknown_older_schema_version_is_corrupt() {
        let (_dir, store) = store_with_contents(r#"{"schema_version": 0, "count": 3}"#);
        assert_corrupt(store.load());
    }

    #[test]
    fn newer_schema_version_is_unsupported_not_corrupt() {
        // A newer build may change the shape entirely; only the version matters.
        let newer = SCHEMA_VERSION + 1;
        let (_dir, store) = store_with_contents(&format!(
            r#"{{"schema_version": {newer}, "counter": {{"value": 9}}}}"#
        ));
        assert_eq!(
            store.load().unwrap_err(),
            StorageError::UnsupportedSchemaVersion {
                found: newer,
                supported: SCHEMA_VERSION,
            }
        );
    }

    #[test]
    fn leftover_temp_file_is_ignored_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSnapshotStore::new(dir.path());
        store.save(&CounterSnapshot { count: 5 }).unwrap();
        let tmp = store.path().with_extension("json.tmp");
        fs::write(&tmp, "{partial").unwrap();
        assert_eq!(store.load().unwrap().unwrap().count, 5);
        store.save(&CounterSnapshot { count: 6 }).unwrap();
        assert!(!tmp.exists());
        assert_eq!(store.load().unwrap().unwrap().count, 6);
    }

    #[test]
    fn read_errors_carry_the_io_reason() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSnapshotStore::new(dir.path());
        // A directory where the file should be makes reading fail.
        fs::create_dir(store.path()).unwrap();
        match store.load() {
            Err(StorageError::ReadFailed { reason }) => assert!(!reason.is_empty()),
            other => panic!("expected ReadFailed, got {other:?}"),
        }
    }

    #[test]
    fn write_failure_readonly_parent() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("ro");
        fs::create_dir(&parent).unwrap();
        let mut perms = fs::metadata(&parent).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o500);
            fs::set_permissions(&parent, perms).unwrap();
            // Root (common in containers and CI) ignores directory permissions,
            // so there is no write failure to observe.
            if fs::write(parent.join("probe"), b"").is_ok() {
                eprintln!("skipping: read-only directory is still writable (running as root?)");
                return;
            }
        }
        let store = JsonSnapshotStore::new(&parent);
        let snap = CounterSnapshot { count: 1 };
        #[cfg(unix)]
        {
            let err = store.save(&snap).unwrap_err();
            assert!(
                matches!(
                    err,
                    StorageError::WriteFailed { .. } | StorageError::MissingDirectory
                ),
                "unexpected {err:?}"
            );
        }
    }
}
