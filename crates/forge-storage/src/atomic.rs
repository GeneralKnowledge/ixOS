//! The storage error type and the atomic file write every store builds on.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

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
/// go to a sibling temp file (unique to this call) which is synced, renamed over `path`, and then
/// the directory is synced. Readers see the old file or the new one, never a
/// torn write. The parent directory must exist.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), StorageError> {
    let directory = path.parent().ok_or(StorageError::MissingDirectory)?;
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| StorageError::write("write file", "path has no file name"))?
        .to_os_string();
    // A name of its own per call, so two writers of the same file never
    // share (and truncate) one temporary file: the last rename wins whole.
    tmp_name.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp = directory.join(tmp_name);
    let result = write_and_replace(&tmp, path, data);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    sync_directory(directory)?;
    remove_stale_temporaries(directory, path);
    Ok(())
}

/// Removes what earlier writes of `path` left behind when their process
/// died between creating the temporary and renaming it: the old fixed name
/// (`<name>.tmp`) and other processes' `<name>.tmp-<pid>-<n>`. Temporaries
/// of this process are left alone, since another thread may be filling one.
fn remove_stale_temporaries(directory: &Path, path: &Path) {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let prefix = format!("{name}.tmp");
    let ours = format!("{prefix}-{}-", std::process::id());
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(other) = file_name.to_str() else {
            continue;
        };
        let stale = other == prefix
            || (other.starts_with(&format!("{prefix}-")) && !other.starts_with(&ours));
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
}

static TEMPORARY: AtomicU64 = AtomicU64::new(0);

fn write_and_replace(tmp: &Path, path: &Path, data: &[u8]) -> Result<(), StorageError> {
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(tmp)
            .map_err(|err| StorageError::write("create temp file", err))?;
        file.write_all(data)
            .map_err(|err| StorageError::write("write temp file", err))?;
        file.sync_all()
            .map_err(|err| StorageError::write("sync temp file", err))?;
    }
    fs::rename(tmp, path).map_err(|err| StorageError::write("replace file", err))
}

/// Makes a completed rename durable by syncing the directory that holds it.
fn sync_directory(dir: &Path) -> Result<(), StorageError> {
    sync_dir(dir).map_err(|err| StorageError::write("sync directory", err))
}

/// Syncs a directory, so the renames, creations and deletions in it reach
/// the disk. Without this, a power cut can undo a rename whose file's bytes
/// were synced.
#[cfg(unix)]
pub(crate) fn sync_dir(dir: &Path) -> std::io::Result<()> {
    File::open(dir).and_then(|d| d.sync_all())
}

#[cfg(not(unix))]
pub(crate) fn sync_dir(_dir: &Path) -> std::io::Result<()> {
    // Directories cannot be opened for syncing on Windows; NTFS journals renames.
    Ok(())
}

/// `create_dir_all`, then syncs the parent of every directory it created,
/// so a new folder survives a power cut along with the files put in it.
pub(crate) fn create_dir_durably(dir: &Path) -> std::io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    let mut missing = Vec::new();
    let mut current = dir;
    while !current.is_dir() {
        missing.push(current);
        match current.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => current = parent,
            _ => break,
        }
    }
    fs::create_dir_all(dir)?;
    for created in missing.iter().rev() {
        if let Some(parent) = created.parent().filter(|p| !p.as_os_str().is_empty()) {
            sync_dir(parent)?;
        }
    }
    Ok(())
}
