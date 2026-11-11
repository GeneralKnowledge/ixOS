//! Backup archives (feature `backup`): the whole of an app's data in one
//! `.zip` a person can keep in Files or iCloud Drive and restore later, on
//! the same phone or a new one.
//!
//! An archive holds the app's files under their relative names plus
//! `forge-backup.json`, a manifest with the app, the data's schema version,
//! and each file's size and SHA-256. Writing is atomic (the archive is
//! complete or absent) and databases are copied consistently with SQLite's
//! `VACUUM INTO`, so a backup taken while the app runs is never torn.
//!
//! Restoring is two steps, because the app's data is open while it runs:
//!
//! 1. [`restore`] checks the archive (the right app, a schema version this
//!    build can read, safe names, every size and hash) and unpacks it into a
//!    new directory. Nothing of the current data changes; a bad archive
//!    fails here.
//! 2. With the core closed, [`replace_directory`] swaps the restored
//!    directory in and keeps the old data beside it, so a restore can be
//!    undone by hand. The app then opens the restored data as usual, and its
//!    normal migrations bring an older backup up to date.

use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::StorageError;

/// The manifest's name inside an archive.
pub const MANIFEST: &str = "forge-backup.json";
/// The archive layout this build writes and reads.
pub const FORMAT: u32 = 1;
/// The most an archive may unpack to (zip bombs, a mistaken file).
pub const MAX_RESTORED_BYTES: u64 = 4 << 30;

/// What an archive says about itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format: u32,
    /// The app the data belongs to, as passed to [`Backup::new`].
    pub app: String,
    /// The app's own schema version for the data, so a restore can refuse
    /// data from a newer build.
    pub schema_version: u32,
    pub created_unix_ms: i64,
    pub files: Vec<BackupEntry>,
}

impl BackupManifest {
    /// Total bytes of the files.
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupEntry {
    /// Relative, with `/` separators.
    pub path: String,
    pub bytes: u64,
    /// Lowercase hex.
    pub sha256: String,
}

enum Source {
    File(PathBuf),
    Bytes(Vec<u8>),
    /// A database copy made for this backup, deleted after writing.
    Temporary(PathBuf),
}

/// An archive being put together. Add the app's data, then [`Backup::write`].
///
/// ```no_run
/// # use forge_storage::backup::Backup;
/// # fn f(dir: &std::path::Path, db: &forge_storage::Database) -> Result<(), forge_storage::StorageError> {
/// let manifest = Backup::new("org.example.library", 3, forge_runtime_now())
///     .database("library.db", db)?          // a consistent copy, even while open
///     .file("settings.json", &dir.join("settings.json"))
///     .directory("covers", &dir.join("covers"))?
///     .write(&dir.join("exports/Library backup.zip"))?;
/// # Ok(()) }
/// # fn forge_runtime_now() -> i64 { 0 }
/// ```
pub struct Backup {
    app: String,
    schema_version: u32,
    created_unix_ms: i64,
    entries: Vec<(String, Source)>,
    scratch: Option<tempfile_dir::Scratch>,
}

impl Backup {
    pub fn new(app: impl Into<String>, schema_version: u32, now_unix_ms: i64) -> Self {
        Self {
            app: app.into(),
            schema_version,
            created_unix_ms: now_unix_ms,
            entries: Vec::new(),
            scratch: None,
        }
    }

    /// A file, stored as `name`. A missing file is left out (an app's
    /// optional files may not exist yet).
    pub fn file(mut self, name: &str, path: &Path) -> Self {
        if path.is_file() {
            self.entries
                .push((name.into(), Source::File(path.to_path_buf())));
        }
        self
    }

    /// Bytes the app has in memory (an export of its state), stored as `name`.
    pub fn bytes(mut self, name: &str, data: Vec<u8>) -> Self {
        self.entries.push((name.into(), Source::Bytes(data)));
        self
    }

    /// Every file under `dir`, stored under `name/`. A missing directory
    /// adds nothing. Temporary and partial files (names starting with `.`)
    /// are left out.
    pub fn directory(mut self, name: &str, dir: &Path) -> Result<Self, StorageError> {
        if !dir.is_dir() {
            return Ok(self);
        }
        let mut stack = vec![(dir.to_path_buf(), name.trim_end_matches('/').to_string())];
        while let Some((path, prefix)) = stack.pop() {
            let mut children: Vec<_> = fs::read_dir(&path)
                .map_err(|e| StorageError::read("list a folder to back up", e))?
                .filter_map(Result::ok)
                .collect();
            children.sort_by_key(|e| e.file_name());
            for child in children {
                let file_name = child.file_name().to_string_lossy().into_owned();
                if file_name.starts_with('.') {
                    continue;
                }
                let inner = format!("{prefix}/{file_name}");
                let kind = child
                    .file_type()
                    .map_err(|e| StorageError::read("read a file to back up", e))?;
                if kind.is_dir() {
                    stack.push((child.path(), inner));
                } else if kind.is_file() {
                    self.entries.push((inner, Source::File(child.path())));
                }
            }
        }
        Ok(self)
    }

    /// A consistent copy of an open SQLite database, stored as `name`. It
    /// waits for any write in progress and never includes half of one.
    #[cfg(feature = "sqlite")]
    pub fn database(mut self, name: &str, db: &crate::Database) -> Result<Self, StorageError> {
        let scratch = match &self.scratch {
            Some(s) => s,
            None => self.scratch.insert(tempfile_dir::Scratch::new()?),
        };
        let copy = scratch.path().join(format!("db-{}", self.entries.len()));
        db.connection()
            .execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()])
            .map_err(|e| StorageError::read("copy the database for a backup", e))?;
        self.entries.push((name.into(), Source::Temporary(copy)));
        Ok(self)
    }

    /// Writes the archive to `out` atomically and returns its manifest.
    pub fn write(self, out: &Path) -> Result<BackupManifest, StorageError> {
        let mut manifest = BackupManifest {
            format: FORMAT,
            app: self.app.clone(),
            schema_version: self.schema_version,
            created_unix_ms: self.created_unix_ms,
            files: Vec::new(),
        };
        let parent = out
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|e| StorageError::write("create the backup folder", e))?;
        let partial = parent.join(format!(
            ".{}.partial",
            out.file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        ));
        let result = (|| {
            let file = fs::File::create(&partial)
                .map_err(|e| StorageError::write("create the backup", e))?;
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .large_file(true);
            let mut seen = std::collections::HashSet::new();
            for (name, source) in &self.entries {
                check_name(name)?;
                if name == MANIFEST || !seen.insert(name.clone()) {
                    return Err(StorageError::write(
                        "add to the backup",
                        format!("\"{name}\" is in it twice"),
                    ));
                }
                zip.start_file(name.as_str(), options)
                    .map_err(|e| StorageError::write("write the backup", e))?;
                let mut hasher = Sha256::new();
                let mut bytes = 0u64;
                let mut copy = |chunk: &[u8]| -> Result<(), StorageError> {
                    hasher.update(chunk);
                    bytes += chunk.len() as u64;
                    zip.write_all(chunk)
                        .map_err(|e| StorageError::write("write the backup", e))
                };
                match source {
                    Source::Bytes(data) => copy(data)?,
                    Source::File(path) | Source::Temporary(path) => {
                        let mut file = fs::File::open(path)
                            .map_err(|e| StorageError::read("read a file to back up", e))?;
                        let mut buffer = vec![0u8; 64 * 1024];
                        loop {
                            let n = file
                                .read(&mut buffer)
                                .map_err(|e| StorageError::read("read a file to back up", e))?;
                            if n == 0 {
                                break;
                            }
                            copy(&buffer[..n])?;
                        }
                    }
                }
                manifest.files.push(BackupEntry {
                    path: name.clone(),
                    bytes,
                    sha256: hex(&hasher.finalize()),
                });
            }
            zip.start_file(MANIFEST, options)
                .map_err(|e| StorageError::write("write the backup", e))?;
            let json = serde_json::to_vec_pretty(&manifest)
                .map_err(|e| StorageError::write("write the backup", e))?;
            zip.write_all(&json)
                .map_err(|e| StorageError::write("write the backup", e))?;
            let file = zip
                .finish()
                .map_err(|e| StorageError::write("finish the backup", e))?;
            file.sync_all()
                .map_err(|e| StorageError::write("finish the backup", e))?;
            fs::rename(&partial, out).map_err(|e| StorageError::write("finish the backup", e))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&partial);
        }
        result.map(|()| manifest)
    }
}

/// Reads an archive's manifest without unpacking it, to show what it holds
/// ("Library backup from 3 May, 412 books") before restoring.
pub fn read_manifest(archive: &Path) -> Result<BackupManifest, StorageError> {
    let mut zip = open(archive)?;
    manifest_of(&mut zip)
}

/// Checks the archive and unpacks it into `into`, a directory that must not
/// exist yet. Refuses another app's archive, data newer than
/// `max_schema_version`, unsafe names, and any file whose size or hash
/// differs from the manifest. On failure nothing is left at `into`.
pub fn restore(
    archive: &Path,
    app: &str,
    max_schema_version: u32,
    into: &Path,
) -> Result<BackupManifest, StorageError> {
    if into.exists() {
        return Err(StorageError::write(
            "restore",
            format!("{} already exists", into.display()),
        ));
    }
    let mut zip = open(archive)?;
    let manifest = manifest_of(&mut zip)?;
    if manifest.format > FORMAT {
        return Err(StorageError::corrupt(
            "the backup was made by a newer version of the app",
        ));
    }
    if manifest.app != app {
        return Err(StorageError::corrupt(format!(
            "this is a backup of another app ({})",
            manifest.app
        )));
    }
    if manifest.schema_version > max_schema_version {
        return Err(StorageError::UnsupportedSchemaVersion {
            found: manifest.schema_version,
            supported: max_schema_version,
        });
    }
    if manifest.total_bytes() > MAX_RESTORED_BYTES {
        return Err(StorageError::corrupt("the backup is too large to restore"));
    }
    let partial = into.with_extension("restoring");
    let _ = fs::remove_dir_all(&partial);
    let result = (|| {
        for entry in &manifest.files {
            check_name(&entry.path)?;
            let target = partial.join(&entry.path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| StorageError::write("restore", e))?;
            }
            let mut source = zip.by_name(&entry.path).map_err(|e| {
                StorageError::corrupt(format!("{} is missing from the backup: {e}", entry.path))
            })?;
            let mut out =
                fs::File::create(&target).map_err(|e| StorageError::write("restore", e))?;
            let mut hasher = Sha256::new();
            let mut bytes = 0u64;
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let n = source
                    .read(&mut buffer)
                    .map_err(|e| StorageError::corrupt(format!("read {}: {e}", entry.path)))?;
                if n == 0 {
                    break;
                }
                bytes += n as u64;
                if bytes > entry.bytes {
                    return Err(StorageError::corrupt(format!(
                        "{} is larger than the backup says",
                        entry.path
                    )));
                }
                hasher.update(&buffer[..n]);
                out.write_all(&buffer[..n])
                    .map_err(|e| StorageError::write("restore", e))?;
            }
            if bytes != entry.bytes || hex(&hasher.finalize()) != entry.sha256 {
                return Err(StorageError::corrupt(format!(
                    "{} is damaged in the backup",
                    entry.path
                )));
            }
            out.sync_all()
                .map_err(|e| StorageError::write("restore", e))?;
        }
        fs::create_dir_all(&partial).map_err(|e| StorageError::write("restore", e))?;
        fs::rename(&partial, into).map_err(|e| StorageError::write("restore", e))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&partial);
    }
    result.map(|()| manifest)
}

/// Swaps `restored` (from [`restore`]) in as `data_dir`, with the app's data
/// closed. The old data is kept as `<data_dir>.before-restore-<now_ms>` and
/// that path returned, so a restore can be undone. `data_dir` may not exist
/// (a new phone).
pub fn replace_directory(
    data_dir: &Path,
    restored: &Path,
    now_ms: i64,
) -> Result<Option<PathBuf>, StorageError> {
    if !restored.is_dir() {
        return Err(StorageError::write(
            "restore",
            "the restored data is missing",
        ));
    }
    let kept = if data_dir.exists() {
        let name = data_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "data".into());
        let kept = data_dir.with_file_name(format!("{name}.before-restore-{now_ms}"));
        fs::rename(data_dir, &kept)
            .map_err(|e| StorageError::write("set the old data aside", e))?;
        Some(kept)
    } else {
        None
    };
    if let Err(e) = fs::rename(restored, data_dir) {
        // Put the old data back rather than leave the app with nothing.
        if let Some(kept) = &kept {
            let _ = fs::rename(kept, data_dir);
        }
        return Err(StorageError::write("put the restored data in place", e));
    }
    Ok(kept)
}

fn open(archive: &Path) -> Result<zip::ZipArchive<fs::File>, StorageError> {
    let file = fs::File::open(archive).map_err(|e| StorageError::read("open the backup", e))?;
    zip::ZipArchive::new(file)
        .map_err(|e| StorageError::corrupt(format!("not a backup archive: {e}")))
}

fn manifest_of(zip: &mut zip::ZipArchive<fs::File>) -> Result<BackupManifest, StorageError> {
    let mut entry = zip
        .by_name(MANIFEST)
        .map_err(|_| StorageError::corrupt("not a backup from this app (no manifest)"))?;
    if entry.size() > 16 << 20 {
        return Err(StorageError::corrupt("the backup's manifest is too large"));
    }
    let mut json = Vec::new();
    entry
        .read_to_end(&mut json)
        .map_err(|e| StorageError::corrupt(format!("read the backup's manifest: {e}")))?;
    serde_json::from_slice(&json)
        .map_err(|e| StorageError::corrupt(format!("the backup's manifest: {e}")))
}

/// Relative, `/`-separated, no `..`, no empty parts.
fn check_name(name: &str) -> Result<(), StorageError> {
    let ok = !name.is_empty()
        && !name.starts_with('/')
        && !name.contains('\\')
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    if ok {
        Ok(())
    } else {
        Err(StorageError::corrupt(format!(
            "unsafe name in a backup: {name:?}"
        )))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A private scratch directory for database copies, removed when the
/// backup is dropped.
mod tempfile_dir {
    use std::path::{Path, PathBuf};

    use crate::StorageError;

    pub struct Scratch(PathBuf);

    impl Scratch {
        pub fn new() -> Result<Self, StorageError> {
            let n = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path =
                std::env::temp_dir().join(format!("forge-backup-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&path)
                .map_err(|e| StorageError::write("make a scratch folder", e))?;
            Ok(Self(path))
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
