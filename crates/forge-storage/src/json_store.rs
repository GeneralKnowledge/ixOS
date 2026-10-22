use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;

use forge_core::CounterSnapshot;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = 1;
const DEFAULT_FILENAME: &str = "forge-counter-v1.json";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StorageError {
    #[error("persistence directory is missing")]
    MissingDirectory,
    #[error("stored data is invalid or corrupted")]
    CorruptData,
    #[error("failed to write persistence data")]
    WriteFailed,
    #[error("failed to read persistence data")]
    ReadFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotEnvelope {
    pub schema_version: u32,
    pub count: i64,
}

impl SnapshotEnvelope {
    pub fn from_snapshot(snapshot: &CounterSnapshot) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            count: snapshot.count,
        }
    }

    pub fn into_snapshot(self) -> Result<CounterSnapshot, StorageError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(StorageError::CorruptData);
        }
        let snap = CounterSnapshot { count: self.count };
        snap.validate().map_err(|_| StorageError::CorruptData)?;
        Ok(snap)
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
        if !path.exists() {
            return Ok(None);
        }
        let mut file = File::open(&path).map_err(|_| StorageError::ReadFailed)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .map_err(|_| StorageError::ReadFailed)?;
        if buf.trim().is_empty() {
            return Err(StorageError::CorruptData);
        }
        serde_json::from_str(&buf)
            .map_err(|_| StorageError::CorruptData)
            .map(Some)
    }
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
        let data =
            serde_json::to_string_pretty(&envelope).map_err(|_| StorageError::WriteFailed)?;
        let path = self.path();
        let tmp = path.with_extension("json.tmp");
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)
                .map_err(|_| StorageError::WriteFailed)?;
            file.write_all(data.as_bytes())
                .map_err(|_| StorageError::WriteFailed)?;
            file.sync_all().map_err(|_| StorageError::WriteFailed)?;
        }
        fs::rename(&tmp, &path).map_err(|_| StorageError::WriteFailed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SnapshotStore;
    use forge_core::CounterSnapshot;
    use std::io::Write;

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
    fn corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path()).unwrap();
        let path = dir.path().join(DEFAULT_FILENAME);
        let mut f = File::create(path).unwrap();
        write!(f, "{{not json").unwrap();
        let store = JsonSnapshotStore::new(dir.path());
        assert_eq!(store.load().unwrap_err(), StorageError::CorruptData);
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
                err == StorageError::WriteFailed || err == StorageError::MissingDirectory,
                "unexpected {err:?}"
            );
        }
    }
}
