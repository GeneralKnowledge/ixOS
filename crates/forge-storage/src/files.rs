//! App-managed files: a folder the app owns, whose files it names, copies in,
//! checks and cleans up.
//!
//! A [`FileStore`] keeps files under one root (an attachments folder, a covers
//! folder, a models folder) and hands out names relative to it. Apps store
//! those names, never absolute paths: on iOS the app container's path changes
//! between installs and restores, so an absolute path saved today points
//! nowhere tomorrow.
//!
//! Every write lands in a hidden temporary file first and is renamed into
//! place, so a crash leaves either the old file, the new one, or a temporary
//! that [`FileStore::sweep`] removes later. Names starting with `.` are
//! reserved for those temporaries.
//!
//! With the `hash` feature the store can also name files by the SHA-256 of
//! their bytes ([`FileStore::import_hashed`]) and keep a download only when its
//! bytes match the hash it was promised ([`FileStore::accept_verified`]).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

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
#[derive(Debug, Clone)]
pub struct FileStore {
    root: PathBuf,
    max_bytes: Option<u64>,
    allow_empty: bool,
}

impl FileStore {
    /// A store rooted at `root`. The folder is created on the first write.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            max_bytes: None,
            allow_empty: false,
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
    /// Temporaries from this process are left alone, since a write may be in
    /// progress on another thread.
    pub fn sweep(&self, keep: impl Fn(&str) -> bool) -> u64 {
        let mut doomed = Vec::new();
        walk(&self.root, "", &mut |name, _| {
            if !keep(name) {
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
        fs::create_dir_all(&self.root)
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

    fn place(&self, from: &Path, destination: &Path) -> Result<(), FileStoreError> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| FileStoreError::io(format!("create {}", parent.display()), err))?;
        }
        if from != destination {
            fs::rename(from, destination).map_err(|err| {
                FileStoreError::io(format!("keep {}", destination.display()), err)
            })?;
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
            let name = hex(&hasher.finalize());
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
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
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
