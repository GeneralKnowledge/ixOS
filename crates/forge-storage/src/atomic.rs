//! The storage error type and the atomic file write every store builds on.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use thiserror::Error;

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
    /// `CorruptData` with `reason`.
    pub fn corrupt(reason: impl ToString) -> Self {
        Self::CorruptData {
            reason: reason.to_string(),
        }
    }

    /// `WriteFailed`, keeping the underlying error's text after `context`.
    pub fn write(context: &str, err: impl std::fmt::Display) -> Self {
        Self::WriteFailed {
            reason: format!("{context}: {err}"),
        }
    }

    /// `ReadFailed`, keeping the underlying error's text after `context`.
    pub fn read(context: &str, err: impl std::fmt::Display) -> Self {
        Self::ReadFailed {
            reason: format!("{context}: {err}"),
        }
    }
}

/// Replaces the file at `path` with `data` atomically and durably: the bytes
/// go to a sibling temp file which is synced, renamed over `path`, and then
/// the directory is synced. Readers see the old file or the new one, never a
/// torn write. The parent directory must exist.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), StorageError> {
    let directory = path.parent().ok_or(StorageError::MissingDirectory)?;
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| StorageError::write("write file", "path has no file name"))?
        .to_os_string();
    tmp_name.push(".tmp");
    let tmp = directory.join(tmp_name);
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|err| StorageError::write("create temp file", err))?;
        file.write_all(data)
            .map_err(|err| StorageError::write("write temp file", err))?;
        file.sync_all()
            .map_err(|err| StorageError::write("sync temp file", err))?;
    }
    fs::rename(&tmp, path).map_err(|err| StorageError::write("replace file", err))?;
    sync_directory(directory)
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
