//! App-managed files: a folder the app owns, whose files it names, copies in,
//! checks and cleans up.
//!
//! A [`FileStore`] keeps files under one root (an attachments folder, a covers
//! folder, a models folder) and hands out names relative to it. Apps store
//! those names, never absolute paths: on iOS the app container's path changes
//! between installs and restores, so an absolute path saved today points
//! nowhere tomorrow.
//!
//! Every write lands in a hidden temporary file first, is synced, and is
//! renamed into place, and then the folder is synced, so a crash or a power
//! cut leaves either the old file, the new one, or a temporary that
//! [`FileStore::sweep`] removes later. Names starting with `.` are reserved
//! for those temporaries.
//!
//! # A file and the record that names it
//!
//! When a saved record (a book in `library.json`, a row in a database) names
//! a file, keep the file first and save the record second, so a committed
//! record never names a missing file. [`FileStore::stage_write`] and
//! [`FileStore::stage_import`] make that the API:
//!
//! ```no_run
//! # use forge_storage::{FileStore, JsonFile};
//! # fn run(covers: &FileStore, library: &JsonFile<Vec<String>>, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
//! let cover = covers.stage_write("book-17.jpg", bytes)?; // 1. the file, durably
//! library.save(&vec![cover.name().to_string()])?;         // 2. the record (on failure, `cover` is dropped and deleted)
//! cover.commit();                                         // 3. the file belongs to the record now
//! # Ok(()) }
//! ```
//!
//! A staged file is never written over an existing one, so a record already
//! saved never has its file changed underneath it, and [`FileStore::sweep`]
//! leaves it alone until it is committed or dropped. A crash between 1 and 2
//! leaves a file no record names, which the launch sweep removes. Removing
//! goes the other way: save the record without the name, then
//! [`FileStore::remove`] the file. See `docs/persistence.md`.
//!
//! With the `hash` feature the store can also name files by the SHA-256 of
//! their bytes ([`FileStore::import_hashed`]) and keep a download only when its
//! bytes match the hash it was promised ([`FileStore::accept_verified`]).

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::atomic::{create_dir_durably, sync_dir};

/// Why a [`FileStore`] operation failed.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum FileStoreError {
    /// The name is not a safe relative name (see [`FileStore::path`]).
    #[error("\"{name}\" is not a valid file name in this store")]
    InvalidName { name: String },
    /// The file is larger than the store's limit.
    #[error("the file is larger than {limit} bytes")]
    TooLarge { limit: u64 },
    /// The file is empty and the store does not keep empty files.
    #[error("the file is empty")]
    Empty,
    /// A download's bytes do not match the hash it was promised. The
    /// downloaded file has been deleted.
    #[error("the file's contents do not match: expected {expected}, got {actual}")]
    Mismatch { expected: String, actual: String },
    /// A file of that name is already in the store, or being staged: a
    /// staged file never replaces one a record may name.
    #[error("\"{name}\" is already in this store")]
    Exists { name: String },
    /// The file system refused: `context` says what was being done.
    #[error("{context}: {reason}")]
    Io { context: String, reason: String },
}

impl FileStoreError {
    fn io(context: impl Into<String>, err: impl std::fmt::Display) -> Self {
        Self::Io {
            context: context.into(),
            reason: err.to_string(),
        }
    }
}

/// A file the store has just kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFile {
    /// Its name in the store, relative to the root. Save this, not `path`.
    pub name: String,
    /// Where it is now.
    pub path: PathBuf,
    /// Its size in bytes.
    pub size: u64,
}

/// How much a store holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Files, not counting temporaries.
    pub files: u64,
    /// Their total size in bytes.
    pub bytes: u64,
}

static TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// A folder of files the app manages. See the [module documentation](self).
///
/// Clones share the set of files being staged, so stage and sweep through
/// one store (or its clones), not two stores made for the same folder.
#[derive(Debug, Clone)]
pub struct FileStore {
    root: PathBuf,
    max_bytes: Option<u64>,
    allow_empty: bool,
    staging: Arc<Mutex<HashSet<String>>>,
}

/// A file kept in a [`FileStore`] that no saved record names yet. Save the
/// record that names it, then [`commit`](Self::commit). Dropping it
/// uncommitted (the record could not be saved) deletes the file.
#[derive(Debug)]
#[must_use = "commit it once the record that names it is saved; dropping it deletes the file"]
pub struct Staged {
    store: FileStore,
    file: StoredFile,
    committed: bool,
}

impl Staged {
    /// The name to save in the record.
    pub fn name(&self) -> &str {
        &self.file.name
    }

    /// Where the file is now.
    pub fn path(&self) -> &Path {
        &self.file.path
    }

    /// Its size in bytes.
    pub fn size(&self) -> u64 {
        self.file.size
    }

    /// Hands the file over to the record that names it: from now on it is
    /// an ordinary file in the store, kept or swept by the record.
    pub fn commit(mut self) -> StoredFile {
        self.committed = true;
        self.store.unstage(&self.file.name);
        self.file.clone()
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.file.path);
            self.store.unstage(&self.file.name);
        }
    }
}

impl FileStore {
    /// A store rooted at `root`. The folder is created on the first write.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            max_bytes: None,
            allow_empty: false,
            staging: Arc::default(),
        }
    }

    /// Refuses imports larger than `limit` bytes, without reading past it.
    pub fn with_max_bytes(mut self, limit: u64) -> Self {
        self.max_bytes = Some(limit);
        self
    }

    /// Keeps empty files too. By default an empty import is refused, since
    /// it is nearly always a failed read or a cancelled picker.
    pub fn allowing_empty(mut self) -> Self {
        self.allow_empty = true;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where `name` is kept, whether or not it exists yet.
    ///
    /// A name is one or more `/`-separated parts, each made of ASCII letters,
    /// digits, `-`, `_` and `.`, not starting with `.`. So `covers/OL1W.jpg`
    /// is fine and `../library.json`, `/etc/passwd` or `.download-x` are not.
    /// Use [`safe_name`] to turn arbitrary text into a valid part.
    pub fn path(&self, name: &str) -> Result<PathBuf, FileStoreError> {
        if is_valid_name(name) {
            Ok(self.root.join(name))
        } else {
            Err(FileStoreError::InvalidName { name: name.into() })
        }
    }

    /// The file called `name`, if it is in the store.
    pub fn get(&self, name: &str) -> Option<PathBuf> {
        let path = self.path(name).ok()?;
        path.is_file().then_some(path)
    }

    /// Replaces `name` with `bytes`, atomically.
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<StoredFile, FileStoreError> {
        let destination = self.path(name)?;
        self.check_size(bytes.len() as u64)?;
        if bytes.is_empty() && !self.allow_empty {
            return Err(FileStoreError::Empty);
        }
        self.keep(&destination, |sink| {
            sink.write_all(bytes)?;
            Ok(bytes.len() as u64)
        })
        .map(|size| StoredFile {
            name: name.into(),
            path: destination,
            size,
        })
    }

    /// Copies the file at `source` into the store as `name`, replacing any
    /// file of that name. The copy outlives the original, so this is the step
    /// to take as soon as the user picks a file.
    pub fn import(&self, source: &Path, name: &str) -> Result<StoredFile, FileStoreError> {
        let destination = self.path(name)?;
        let mut reader = open(source)?;
        let size = self.keep(&destination, |sink| self.copy(&mut reader, sink))?;
        Ok(StoredFile {
            name: name.into(),
            path: destination,
            size,
        })
    }

    /// Keeps `bytes` as `name` for a record that is about to be saved. See
    /// [`Staged`] and the [module documentation](self). Fails with
    /// [`FileStoreError::Exists`] rather than replace a file.
    pub fn stage_write(&self, name: &str, bytes: &[u8]) -> Result<Staged, FileStoreError> {
        self.stage(name, || self.write(name, bytes))
    }

    /// Copies `source` in as `name` for a record that is about to be saved,
    /// like [`Self::stage_write`].
    pub fn stage_import(&self, source: &Path, name: &str) -> Result<Staged, FileStoreError> {
        self.stage(name, || self.import(source, name))
    }

    fn stage(
        &self,
        name: &str,
        keep: impl FnOnce() -> Result<StoredFile, FileStoreError>,
    ) -> Result<Staged, FileStoreError> {
        let path = self.path(name)?;
        {
            let mut staging = self.staging.lock().unwrap_or_else(|p| p.into_inner());
            if staging.contains(name) || path.exists() {
                return Err(FileStoreError::Exists { name: name.into() });
            }
            staging.insert(name.to_string());
        }
        match keep() {
            Ok(file) => Ok(Staged {
                store: self.clone(),
                file,
                committed: false,
            }),
            Err(err) => {
                self.unstage(name);
                Err(err)
            }
        }
    }

    fn unstage(&self, name: &str) {
        self.staging
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(name);
    }

    fn is_staged(&self, name: &str) -> bool {
        self.staging
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(name)
    }

    /// A fresh hidden path in the store for a download or other partial
    /// write. Hand it to the downloader, then [`accept`](Self::accept) it
    /// (or [`accept_verified`](Self::accept_verified)). Partial files left by
    /// a crash are removed by [`sweep`](Self::sweep).
    pub fn partial_path(&self) -> Result<PathBuf, FileStoreError> {
        self.ensure_root()?;
        Ok(self.root.join(format!(
            ".partial-{}-{}",
            std::process::id(),
            TEMPORARY.fetch_add(1, Ordering::Relaxed)
        )))
    }

    /// Moves a finished partial file into place as `name`.
    pub fn accept(&self, partial: &Path, name: &str) -> Result<StoredFile, FileStoreError> {
        let destination = self.path(name)?;
        let size = fs::metadata(partial)
            .map_err(|err| FileStoreError::io(format!("read {}", partial.display()), err))?
            .len();
        if let Err(err) = self
            .check_size(size)
            .and_then(|()| self.check_empty(size))
            .and_then(|()| self.place(partial, &destination))
        {
            let _ = fs::remove_file(partial);
            return Err(err);
        }
        Ok(StoredFile {
            name: name.into(),
            path: destination,
            size,
        })
    }

    /// Deletes `name`. Returns whether a file was removed. Invalid names are
    /// ignored, so this never deletes outside the store.
    pub fn remove(&self, name: &str) -> bool {
        self.path(name)
            .map(|path| fs::remove_file(path).is_ok())
            .unwrap_or(false)
    }

    /// Deletes the whole store.
    pub fn clear(&self) -> Result<(), FileStoreError> {
        match fs::remove_dir_all(&self.root) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(FileStoreError::io(
                format!("remove {}", self.root.display()),
                err,
            )),
        }
    }

    /// The names of every file in the store, sorted, not counting
    /// temporaries.
    pub fn names(&self) -> Vec<String> {
        let mut names = Vec::new();
        walk(&self.root, "", &mut |name, _| names.push(name.to_string()));
        names.sort();
        names
    }

    /// How many files the store holds and their total size.
    pub fn usage(&self) -> Usage {
        let mut usage = Usage::default();
        walk(&self.root, "", &mut |_, meta| {
            usage.files += 1;
            usage.bytes += meta.len();
        });
        usage
    }

    /// Deletes only the temporaries an earlier process left behind (a write
    /// or download killed with the app), never a named file. For stores
    /// whose records cannot be listed cheaply at launch. Returns how many
    /// were deleted.
    pub fn sweep_temporaries(&self) -> u64 {
        let mut doomed = Vec::new();
        collect_temporaries(&self.root, &std::process::id().to_string(), &mut doomed);
        doomed
            .iter()
            .filter(|path| fs::remove_file(path).is_ok())
            .count() as u64
    }

    /// Deletes every file `keep` says no to, and temporaries left by a crash.
    /// Returns how many files were deleted. Call it at launch, after loading
    /// the records that name the files, to clean up after a crash between
    /// writing a file and saving the record that refers to it.
    ///
    /// Files being staged through this store (see [`Staged`]) and
    /// temporaries from this process are left alone, since a write may be
    /// in progress on another thread. A file written with plain
    /// [`Self::write`] whose record is not saved yet is not protected, so
    /// sweep only at launch, before any work starts, or stage the file.
    pub fn sweep(&self, keep: impl Fn(&str) -> bool) -> u64 {
        let mut doomed = Vec::new();
        walk(&self.root, "", &mut |name, _| {
            if !keep(name) && !self.is_staged(name) {
                doomed.push(self.root.join(name));
            }
        });
        collect_temporaries(&self.root, &std::process::id().to_string(), &mut doomed);
        doomed
            .iter()
            .filter(|path| fs::remove_file(path).is_ok())
            .count() as u64
    }

    fn ensure_root(&self) -> Result<(), FileStoreError> {
        create_dir_durably(&self.root)
            .map_err(|err| FileStoreError::io(format!("create {}", self.root.display()), err))
    }

    fn check_size(&self, size: u64) -> Result<(), FileStoreError> {
        match self.max_bytes {
            Some(limit) if size > limit => Err(FileStoreError::TooLarge { limit }),
            _ => Ok(()),
        }
    }

    fn check_empty(&self, size: u64) -> Result<(), FileStoreError> {
        if size == 0 && !self.allow_empty {
            Err(FileStoreError::Empty)
        } else {
            Ok(())
        }
    }

    /// Copies `reader` to `sink`, stopping as soon as the limit is passed.
    fn copy(&self, reader: &mut impl Read, sink: &mut impl Write) -> std::io::Result<u64> {
        copy_limited(reader, sink, self.max_bytes, |_| {})
    }

    /// Writes a temporary with `fill`, checks it, and renames it to
    /// `destination`. The temporary is removed on any failure.
    fn keep(
        &self,
        destination: &Path,
        fill: impl FnOnce(&mut File) -> std::io::Result<u64>,
    ) -> Result<u64, FileStoreError> {
        let partial = self.partial_path()?;
        let result = (|| {
            let mut sink = File::create(&partial)
                .map_err(|err| FileStoreError::io("create a temporary file", err))?;
            let size = fill(&mut sink).map_err(|err| self.copy_error(err))?;
            sink.sync_all()
                .map_err(|err| FileStoreError::io("sync a temporary file", err))?;
            self.check_empty(size)?;
            self.place(&partial, destination)?;
            Ok(size)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&partial);
        }
        result
    }

    fn copy_error(&self, err: std::io::Error) -> FileStoreError {
        match (err.kind(), self.max_bytes) {
            (std::io::ErrorKind::FileTooLarge, Some(limit)) => FileStoreError::TooLarge { limit },
            _ => FileStoreError::io("copy the file", err),
        }
    }

    /// Renames a synced temporary to `destination` and syncs the folder,
    /// so the rename survives a power cut (foundation review S4).
    fn place(&self, from: &Path, destination: &Path) -> Result<(), FileStoreError> {
        let Some(parent) = destination.parent() else {
            return Ok(());
        };
        create_dir_durably(parent)
            .map_err(|err| FileStoreError::io(format!("create {}", parent.display()), err))?;
        if from != destination {
            fs::rename(from, destination).map_err(|err| {
                FileStoreError::io(format!("keep {}", destination.display()), err)
            })?;
            // A temporary in another folder (the root) leaves an entry there too.
            for dir in [Some(parent), from.parent().filter(|dir| *dir != parent)]
                .into_iter()
                .flatten()
            {
                sync_dir(dir)
                    .map_err(|err| FileStoreError::io(format!("sync {}", dir.display()), err))?;
            }
        }
        Ok(())
    }
}

#[cfg(feature = "hash")]
impl FileStore {
    /// Copies `source` into the store, named by the SHA-256 of its bytes
    /// (64 lowercase hex characters). Importing the same bytes twice keeps one
    /// file. Reads the source once.
    pub fn import_hashed(&self, source: &Path) -> Result<StoredFile, FileStoreError> {
        self.import_hashed_with_extension(source, None)
    }

    /// Like [`import_hashed`](Self::import_hashed), but the name keeps the
    /// source's extension (`<sha256>.m4a`), cleaned as [`safe_file_name`]
    /// cleans it. Apple's media frameworks and Quick Look use the extension
    /// to tell a file's type. A source without one gets the bare hash.
    pub fn import_hashed_keeping_extension(
        &self,
        source: &Path,
    ) -> Result<StoredFile, FileStoreError> {
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .map(safe_name)
            .filter(|e| !e.is_empty());
        self.import_hashed_with_extension(source, extension.as_deref())
    }

    /// Keeps `bytes` named by their SHA-256, with `.extension` if given
    /// (cleaned as [`safe_name`] cleans it). Writing the same bytes twice
    /// keeps one file and doesn't write again.
    pub fn write_hashed(
        &self,
        bytes: &[u8],
        extension: Option<&str>,
    ) -> Result<StoredFile, FileStoreError> {
        use sha2::Digest;
        let name = hashed_name(&hex(&sha2::Sha256::digest(bytes)), extension);
        match self.get(&name) {
            Some(path) => Ok(StoredFile {
                name,
                path,
                size: bytes.len() as u64,
            }),
            None => self.write(&name, bytes),
        }
    }

    /// Like [`import_hashed`](Self::import_hashed), named
    /// `<sha256>.<extension>` (cleaned as [`safe_name`] cleans it), whatever
    /// the source is called: for a file the app already checked is, say, a
    /// WAV recording.
    pub fn import_hashed_with_extension(
        &self,
        source: &Path,
        extension: Option<&str>,
    ) -> Result<StoredFile, FileStoreError> {
        use sha2::Digest;
        let mut reader = open(source)?;
        let partial = self.partial_path()?;
        let result = (|| {
            let mut sink = File::create(&partial)
                .map_err(|err| FileStoreError::io("create a temporary file", err))?;
            let mut hasher = sha2::Sha256::new();
            let size = copy_limited(&mut reader, &mut sink, self.max_bytes, |chunk| {
                hasher.update(chunk)
            })
            .map_err(|err| self.copy_error(err))?;
            sink.sync_all()
                .map_err(|err| FileStoreError::io("sync a temporary file", err))?;
            self.check_empty(size)?;
            let name = hashed_name(&hex(&hasher.finalize()), extension);
            let destination = self.root.join(&name);
            self.place(&partial, &destination)?;
            Ok(StoredFile {
                name,
                path: destination,
                size,
            })
        })();
        if result.is_err() {
            let _ = fs::remove_file(&partial);
        }
        result
    }

    /// Keeps a finished partial file (see [`partial_path`](Self::partial_path))
    /// as `expected`, a lowercase hex SHA-256, only if its bytes hash to
    /// that. Otherwise it is deleted and the error says what arrived.
    pub fn accept_verified(
        &self,
        partial: &Path,
        expected: &str,
    ) -> Result<StoredFile, FileStoreError> {
        if !is_sha256_hex(expected) {
            let _ = fs::remove_file(partial);
            return Err(FileStoreError::InvalidName {
                name: expected.into(),
            });
        }
        let actual = match sha256_file(partial) {
            Ok(actual) => actual,
            Err(err) => {
                let _ = fs::remove_file(partial);
                return Err(err);
            }
        };
        if actual != expected {
            let _ = fs::remove_file(partial);
            return Err(FileStoreError::Mismatch {
                expected: expected.into(),
                actual,
            });
        }
        self.accept(partial, expected)
    }
}

/// The SHA-256 of a file's bytes, as 64 lowercase hex characters.
#[cfg(feature = "hash")]
pub fn sha256_file(path: &Path) -> Result<String, FileStoreError> {
    use sha2::Digest;
    let mut reader = open(path)?;
    let mut hasher = sha2::Sha256::new();
    copy_limited(&mut reader, &mut std::io::sink(), None, |chunk| {
        hasher.update(chunk)
    })
    .map_err(|err| FileStoreError::io(format!("read {}", path.display()), err))?;
    Ok(hex(&hasher.finalize()))
}

/// Whether `s` is a SHA-256 as [`FileStore::import_hashed`] names files: 64
/// lowercase hex characters.
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(feature = "hash")]
fn hashed_name(hash: &str, extension: Option<&str>) -> String {
    match extension.map(safe_name).filter(|e| !e.is_empty()) {
        Some(extension) => format!("{hash}.{extension}"),
        None => hash.to_string(),
    }
}

#[cfg(feature = "hash")]
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Turns a file's name as a person or another device gave it into one valid
/// name part that keeps its extension: `Holiday photo.JPG` becomes
/// `Holiday-photo.JPG`, `../../etc/passwd` becomes `passwd`, `.hidden`
/// becomes `hidden`. Only the last extension is kept as one.
pub fn safe_file_name(text: &str) -> String {
    let base = text.rsplit(['/', '\\']).next().unwrap_or("");
    let base = base.trim_start_matches('.');
    match base.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => {
            format!("{}.{}", safe_name(stem), safe_name(extension))
        }
        _ => safe_name(base),
    }
}

/// Turns any text (a title, a catalogue key, a URL path) into one valid name
/// part: ASCII letters, digits, `-` and `_` are kept, everything else becomes
/// `-`. Empty text becomes `file`.
pub fn safe_name(text: &str) -> String {
    let safe: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if safe.is_empty() {
        "file".into()
    } else {
        safe
    }
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}

fn open(path: &Path) -> Result<File, FileStoreError> {
    File::open(path).map_err(|err| FileStoreError::io(format!("open {}", path.display()), err))
}

fn copy_limited(
    reader: &mut impl Read,
    sink: &mut impl Write,
    limit: Option<u64>,
    mut each: impl FnMut(&[u8]),
) -> std::io::Result<u64> {
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buffer) {
            Ok(0) => return Ok(total),
            Ok(n) => n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        total += n as u64;
        if limit.is_some_and(|limit| total > limit) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "file too large",
            ));
        }
        each(&buffer[..n]);
        sink.write_all(&buffer[..n])?;
    }
}

/// Calls `visit` with the name and metadata of every regular file under
/// `dir`, skipping hidden entries (temporaries) and symbolic links.
fn walk(dir: &Path, prefix: &str, visit: &mut dyn FnMut(&str, &fs::Metadata)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(part) = file_name.to_str() else {
            continue;
        };
        if part.starts_with('.') {
            continue;
        }
        let name = if prefix.is_empty() {
            part.to_string()
        } else {
            format!("{prefix}/{part}")
        };
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(&entry.path(), &name, visit);
        } else if kind.is_file() {
            visit(&name, &meta);
        }
    }
}

/// Hidden `.partial-<pid>-<n>` files under `dir` whose pid is not `ours`.
fn collect_temporaries(dir: &Path, ours: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let file_name = entry.file_name();
        let part = file_name.to_string_lossy();
        if kind.is_dir() && !part.starts_with('.') {
            collect_temporaries(&entry.path(), ours, out);
        } else if kind.is_file() {
            let pid = part
                .strip_prefix(".partial-")
                .and_then(|rest| rest.split('-').next());
            if pid.is_some_and(|pid| pid != ours) {
                out.push(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, FileStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path().join("files"));
        (dir, store)
    }

    fn hidden(store: &FileStore) -> Vec<String> {
        fs::read_dir(store.root())
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with('.'))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn safe_file_names_keep_their_extension() {
        assert_eq!(safe_file_name("Holiday photo.JPG"), "Holiday-photo.JPG");
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("C:\\Users\\me\\notes.txt"), "notes.txt");
        assert_eq!(safe_file_name(".hidden"), "hidden");
        assert_eq!(safe_file_name("archive.tar.gz"), "archive-tar.gz");
        assert_eq!(safe_file_name(""), "file");
        let (_dir, store) = store();
        assert!(store.path(&safe_file_name("../x.y")).is_ok());
    }

    #[cfg(feature = "hash")]
    #[test]
    fn hashed_names_can_keep_an_extension() {
        let (dir, store) = store();
        let source = dir.path().join("Take 1.WAV");
        fs::write(&source, b"riff").unwrap();
        let bare = store.import_hashed(&source).unwrap();
        assert!(is_sha256_hex(&bare.name));
        let kept = store.import_hashed_keeping_extension(&source).unwrap();
        assert_eq!(kept.name, format!("{}.WAV", bare.name));

        let written = store.write_hashed(b"riff", Some("wav")).unwrap();
        assert_eq!(written.name, format!("{}.wav", bare.name));
        assert_eq!(fs::read(&written.path).unwrap(), b"riff");
        // The same bytes again: same name, nothing rewritten.
        let again = store.write_hashed(b"riff", Some("wav")).unwrap();
        assert_eq!(again, written);
        assert_eq!(store.write_hashed(b"riff", None).unwrap().name, bare.name);

        let plain = dir.path().join("README");
        fs::write(&plain, b"x").unwrap();
        assert!(is_sha256_hex(
            &store.import_hashed_keeping_extension(&plain).unwrap().name
        ));
    }

    #[test]
    fn names_stay_inside_the_store() {
        let (_dir, store) = store();
        for good in ["a", "covers/OL1W.jpg", "models/x/weights_1.bin"] {
            assert!(store.path(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "../library.json",
            "covers/../../x",
            "/etc/passwd",
            ".partial-1",
            "a//b",
            "a/",
            "caf\u{e9}",
            "a b",
            "a\\b",
        ] {
            assert!(
                matches!(store.path(bad), Err(FileStoreError::InvalidName { .. })),
                "{bad}"
            );
            assert!(!store.remove(bad));
            assert_eq!(store.get(bad), None);
        }
        assert_eq!(safe_name("ol/../OL1W"), "ol----OL1W");
        assert_eq!(safe_name(""), "file");
        assert!(store.path(&safe_name("The Hobbit: 1937!")).is_ok());
    }

    #[test]
    fn writes_imports_and_reports_usage() {
        let (dir, store) = store();
        assert_eq!(store.usage(), Usage::default());
        assert_eq!(store.names(), Vec::<String>::new());

        let cover = store.write("covers/OL1W.jpg", b"img").unwrap();
        assert_eq!(cover.size, 3);
        assert_eq!(fs::read(&cover.path).unwrap(), b"img");

        let original = dir.path().join("Report.pdf");
        fs::write(&original, b"hello").unwrap();
        let report = store.import(&original, "report.pdf").unwrap();
        fs::remove_file(&original).unwrap();
        assert_eq!(
            fs::read(store.get("report.pdf").unwrap()).unwrap(),
            b"hello"
        );
        assert_eq!(report.size, 5);

        // Replacing keeps one file.
        store.write("report.pdf", b"hi").unwrap();
        assert_eq!(store.names(), ["covers/OL1W.jpg", "report.pdf"]);
        assert_eq!(store.usage(), Usage { files: 2, bytes: 5 });

        assert!(store.remove("report.pdf"));
        assert!(!store.remove("report.pdf"));
        store.clear().unwrap();
        store.clear().unwrap();
        assert_eq!(store.usage(), Usage::default());
    }

    #[test]
    fn refuses_empty_and_oversized_files_and_cleans_up() {
        let (dir, store) = store();
        let store = store.with_max_bytes(4);
        assert_eq!(store.write("a", b""), Err(FileStoreError::Empty));
        assert_eq!(
            store.write("a", b"12345"),
            Err(FileStoreError::TooLarge { limit: 4 })
        );
        let big = dir.path().join("big");
        fs::write(&big, b"123456789").unwrap();
        assert_eq!(
            store.import(&big, "big"),
            Err(FileStoreError::TooLarge { limit: 4 })
        );
        let empty = dir.path().join("empty");
        fs::write(&empty, b"").unwrap();
        assert_eq!(store.import(&empty, "e"), Err(FileStoreError::Empty));
        assert!(matches!(
            store.import(&dir.path().join("missing"), "m"),
            Err(FileStoreError::Io { .. })
        ));
        assert_eq!(hidden(&store), Vec::<String>::new());
        assert_eq!(store.names(), Vec::<String>::new());

        let allowing = FileStore::new(store.root()).allowing_empty();
        assert_eq!(allowing.write("e", b"").unwrap().size, 0);
    }

    #[test]
    fn accepts_partial_files() {
        let (_dir, store) = store();
        let partial = store.partial_path().unwrap();
        fs::write(&partial, b"model").unwrap();
        // Partial files are not listed while in progress.
        assert_eq!(store.usage(), Usage::default());
        let kept = store.accept(&partial, "models/m.bin").unwrap();
        assert_eq!(kept.size, 5);
        assert!(!partial.exists());
        assert_eq!(store.names(), ["models/m.bin"]);

        let store = store.with_max_bytes(2);
        let partial = store.partial_path().unwrap();
        fs::write(&partial, b"too big").unwrap();
        assert_eq!(
            store.accept(&partial, "x"),
            Err(FileStoreError::TooLarge { limit: 2 })
        );
        assert!(!partial.exists());
    }

    #[test]
    fn sweep_removes_unreferenced_files_and_stale_temporaries() {
        let (_dir, store) = store();
        store.write("covers/kept.jpg", b"1").unwrap();
        store.write("covers/orphan.jpg", b"2").unwrap();
        store.write("exports/old.csv", b"3").unwrap();
        // A temporary from a process that crashed, and one of ours.
        fs::write(store.root().join(".partial-0-0"), b"x").unwrap();
        fs::write(store.root().join("covers/.partial-0-1"), b"x").unwrap();
        let ours = store.partial_path().unwrap();
        fs::write(&ours, b"x").unwrap();

        let removed = store.sweep(|name| name == "covers/kept.jpg");
        assert_eq!(removed, 4);
        assert_eq!(store.names(), ["covers/kept.jpg"]);
        assert!(ours.exists());
    }

    #[test]
    fn a_staged_file_is_kept_only_once_committed() {
        let (dir, store) = store();
        // Committed: an ordinary file from then on.
        let staged = store.stage_write("covers/a.jpg", b"img").unwrap();
        assert_eq!(staged.name(), "covers/a.jpg");
        assert_eq!(staged.size(), 3);
        let kept = staged.commit();
        assert_eq!(fs::read(&kept.path).unwrap(), b"img");
        assert_eq!(store.sweep(|_| false), 1);

        // Dropped (the record could not be saved): deleted.
        let staged = store.stage_write("covers/b.jpg", b"img").unwrap();
        let path = staged.path().to_path_buf();
        assert!(path.exists());
        drop(staged);
        assert!(!path.exists());
        assert_eq!(store.names(), Vec::<String>::new());

        // Imports too.
        let original = dir.path().join("book.epub");
        fs::write(&original, b"epub").unwrap();
        store.stage_import(&original, "b.epub").unwrap().commit();
        assert_eq!(store.names(), ["b.epub"]);
    }

    #[test]
    fn staging_never_replaces_a_file() {
        let (_dir, store) = store();
        store.write("a", b"named by a record").unwrap();
        assert_eq!(
            store.stage_write("a", b"new").map(|s| s.commit()),
            Err(FileStoreError::Exists { name: "a".into() })
        );
        assert_eq!(
            fs::read(store.get("a").unwrap()).unwrap(),
            b"named by a record"
        );

        // Nor one being staged, through this store or a clone.
        let first = store.stage_write("b", b"1").unwrap();
        let clone = store.clone();
        assert!(matches!(
            clone.stage_write("b", b"2"),
            Err(FileStoreError::Exists { .. })
        ));
        drop(first);
        clone.stage_write("b", b"2").unwrap().commit();
        assert_eq!(fs::read(store.get("b").unwrap()).unwrap(), b"2");

        // A failed stage releases the name.
        let limited = FileStore::new(store.root()).with_max_bytes(1);
        assert!(limited.stage_write("c", b"too big").is_err());
        limited.stage_write("c", b"1").unwrap().commit();
    }

    #[test]
    fn sweep_leaves_staged_files_alone() {
        let (_dir, store) = store();
        let staged = store.stage_write("covers/new.jpg", b"img").unwrap();
        store.write("covers/orphan.jpg", b"x").unwrap();
        // A sweep on another thread while the record is being saved.
        assert_eq!(store.clone().sweep(|_| false), 1);
        assert!(staged.path().exists());
        let kept = staged.commit();
        assert_eq!(store.sweep(|name| name == kept.name), 0);
        assert_eq!(store.names(), ["covers/new.jpg"]);
    }

    #[cfg(feature = "hash")]
    #[test]
    fn names_by_content_and_verifies_downloads() {
        let (dir, store) = store();
        const HELLO: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let original = dir.path().join("photo.jpg");
        fs::write(&original, b"hello").unwrap();
        let first = store.import_hashed(&original).unwrap();
        assert_eq!(first.name, HELLO);
        assert_eq!(first.size, 5);
        assert!(is_sha256_hex(&first.name));
        // Same bytes, same file.
        store.import_hashed(&original).unwrap();
        assert_eq!(store.names(), [HELLO]);
        assert_eq!(sha256_file(&first.path).unwrap(), HELLO);

        let other = "0".repeat(64);
        let wrong = store.partial_path().unwrap();
        fs::write(&wrong, b"not it").unwrap();
        assert!(matches!(
            store.accept_verified(&wrong, &other),
            Err(FileStoreError::Mismatch { .. })
        ));
        assert!(!wrong.exists());

        let bad_name = store.partial_path().unwrap();
        fs::write(&bad_name, b"hello").unwrap();
        assert!(matches!(
            store.accept_verified(&bad_name, "../x"),
            Err(FileStoreError::InvalidName { .. })
        ));
        assert!(!bad_name.exists());

        store.remove(HELLO);
        let right = store.partial_path().unwrap();
        fs::write(&right, b"hello").unwrap();
        assert_eq!(
            store.accept_verified(&right, HELLO).unwrap().path,
            first.path
        );
        assert_eq!(store.names(), [HELLO]);
        assert_eq!(hidden(&store), Vec::<String>::new());

        let limited = FileStore::new(store.root()).with_max_bytes(3);
        assert_eq!(
            limited.import_hashed(&original),
            Err(FileStoreError::TooLarge { limit: 3 })
        );
        assert_eq!(hidden(&store), Vec::<String>::new());
    }
}
