//! Where the library lives on disk:
//!
//! ```text
//! <storage directory>/
//!   library.json      books, history, settings (forge_storage::JsonFile)
//!   covers/           saved cover images, named in library.json
//!   books/            saved EPUB files, named in library.json
//!   reader/           images of the chapter being read (emptied at launch)
//!   exports/          the latest export, for the share sheet
//! ```

use std::path::{Path, PathBuf};

use forge_storage::{write_atomic, JsonFile, StorageError};

use crate::domain::LibraryData;
use crate::error::LibraryError;

/// Schema of `library.json`. Additive changes use `#[serde(default)]` and keep
/// this; a change that old files cannot be read as bumps it and adds a step to
/// [`migrate`].
pub const STORE_VERSION: u32 = 1;

pub struct Store {
    dir: PathBuf,
    file: JsonFile<LibraryData>,
}

/// Upgrades older `library.json` data. Version 1 is the first, so there is
/// nothing to upgrade yet; the hook is registered so that the next schema
/// change only adds a match arm.
fn migrate(from: u32, _data: serde_json::Value) -> Result<serde_json::Value, StorageError> {
    Err(StorageError::CorruptData {
        reason: format!("no migration from schema_version {from}"),
    })
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let file = JsonFile::new(dir.join("library.json"), STORE_VERSION).with_migration(migrate);
        Self { dir, file }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.dir.join("covers")
    }

    pub fn books_dir(&self) -> PathBuf {
        self.dir.join("books")
    }

    pub fn reader_dir(&self) -> PathBuf {
        self.dir.join("reader")
    }

    pub fn exports_dir(&self) -> PathBuf {
        self.dir.join("exports")
    }

    pub fn library_file(&self) -> &Path {
        self.file.path()
    }

    /// The saved library, or an empty one on first launch. Unreadable files
    /// are reported and left untouched.
    pub fn load(&self) -> Result<LibraryData, LibraryError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| LibraryError::Storage {
            reason: format!("create {}: {e}", self.dir.display()),
        })?;
        Ok(self.file.load()?.unwrap_or_default())
    }

    pub fn save(&self, data: &LibraryData) -> Result<(), LibraryError> {
        Ok(self.file.save(data)?)
    }

    /// Moves an unreadable `library.json` aside as
    /// `library.unreadable-<ms>.json`, so the app can start empty without
    /// destroying it. Returns the backup's path (or `None` if there was no
    /// file).
    pub fn set_aside(&self, now_ms: i64) -> Result<Option<PathBuf>, LibraryError> {
        let path = self.file.path();
        if !path.exists() {
            return Ok(None);
        }
        let backup = self.dir.join(format!("library.unreadable-{now_ms}.json"));
        std::fs::rename(path, &backup).map_err(|e| LibraryError::Storage {
            reason: format!("move {} aside: {e}", path.display()),
        })?;
        Ok(Some(backup))
    }

    /// Saves cover bytes under a new name and returns the name.
    pub fn save_cover(
        &self,
        stem: &str,
        extension: &str,
        bytes: &[u8],
    ) -> Result<String, LibraryError> {
        let dir = self.covers_dir();
        std::fs::create_dir_all(&dir).map_err(|e| LibraryError::Storage {
            reason: format!("create {}: {e}", dir.display()),
        })?;
        let name = format!("{}.{extension}", sanitize(stem));
        write_atomic(&dir.join(&name), bytes)?;
        Ok(name)
    }

    /// Best effort: a cover that cannot be deleted only wastes space.
    pub fn delete_cover(&self, name: &str) {
        if safe_name(name) {
            let _ = std::fs::remove_file(self.covers_dir().join(name));
        }
    }

    /// Copies an EPUB into `books/` under a new name and returns the name.
    pub fn save_book_file(&self, stem: &str, bytes: &[u8]) -> Result<String, LibraryError> {
        let dir = self.books_dir();
        std::fs::create_dir_all(&dir).map_err(|e| LibraryError::Storage {
            reason: format!("create {}: {e}", dir.display()),
        })?;
        let name = format!("{}.epub", sanitize(stem));
        write_atomic(&dir.join(&name), bytes)?;
        Ok(name)
    }

    /// The saved EPUB's path for this launch, if `name` is a safe name.
    pub fn book_file(&self, name: &str) -> Option<PathBuf> {
        safe_name(name).then(|| self.books_dir().join(name))
    }

    /// Best effort, like [`Self::delete_cover`].
    pub fn delete_book_file(&self, name: &str) {
        if let Some(path) = self.book_file(name) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Empties `reader/` (chapter images from an earlier launch) and
    /// returns a fresh folder for `book_id`'s images.
    pub fn reset_reader_dir(&self, book_id: u64) -> Result<PathBuf, LibraryError> {
        let _ = std::fs::remove_dir_all(self.reader_dir());
        let dir = self.reader_dir().join(book_id.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| LibraryError::Storage {
            reason: format!("create {}: {e}", dir.display()),
        })?;
        Ok(dir)
    }

    /// Deletes covers and EPUBs that no book refers to (left behind by a
    /// crash between saving the file and saving the library), and the
    /// reader's images. Returns how many files went.
    pub fn remove_orphan_covers(&self, data: &LibraryData) -> u32 {
        let _ = std::fs::remove_dir_all(self.reader_dir());
        let covers = remove_unused(&self.covers_dir(), |name| {
            data.books
                .iter()
                .any(|b| b.cover_file.as_deref() == Some(name))
        });
        let books = remove_unused(&self.books_dir(), |name| {
            data.books
                .iter()
                .any(|b| b.epub_file.as_deref() == Some(name))
        });
        covers + books
    }

    pub fn write_export(&self, file_name: &str, bytes: &[u8]) -> Result<PathBuf, LibraryError> {
        let dir = self.exports_dir();
        // Only the latest export is kept.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| LibraryError::Storage {
            reason: format!("create {}: {e}", dir.display()),
        })?;
        let path = dir.join(file_name);
        write_atomic(&path, bytes)?;
        Ok(path)
    }

    /// Deletes covers, EPUBs, reader images and exports (the caller saves
    /// an empty library).
    pub fn clear_files(&self) {
        let _ = std::fs::remove_dir_all(self.covers_dir());
        let _ = std::fs::remove_dir_all(self.books_dir());
        let _ = std::fs::remove_dir_all(self.reader_dir());
        let _ = std::fs::remove_dir_all(self.exports_dir());
    }

    /// (number of saved covers, number of saved EPUBs, bytes of
    /// library.json + covers + EPUBs)
    pub fn usage(&self) -> (u32, u32, u64) {
        let file_bytes = std::fs::metadata(self.file.path())
            .map(|m| m.len())
            .unwrap_or(0);
        let (covers, cover_bytes) = folder_usage(&self.covers_dir());
        let (books, book_bytes) = folder_usage(&self.books_dir());
        (covers, books, file_bytes + cover_bytes + book_bytes)
    }
}

/// File names Rust made: no folders, no way out of the folder.
fn safe_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")
}

fn sanitize(stem: &str) -> String {
    stem.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn remove_unused(dir: &Path, used: impl Fn(&str) -> bool) -> u32 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !used(&name) && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn folder_usage(dir: &Path) -> (u32, u64) {
    let (mut files, mut bytes) = (0u32, 0u64);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    (files, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::validate;
    use crate::model::{BookInput, BookSource, LibrarySort, ReadingStatus};

    fn sample() -> LibraryData {
        let mut data = LibraryData::default();
        let input = BookInput {
            title: "Persisted".into(),
            author: "A".into(),
            description: "d".into(),
            status: ReadingStatus::Reading,
            current_page: 5,
            total_pages: Some(10),
            rating: Some(3),
            notes: "n".into(),
            cover_url: None,
        };
        data.add(validate(input).unwrap(), BookSource::Manual, None, 1)
            .unwrap();
        data.settings.default_sort = LibrarySort::Title;
        data
    }

    #[test]
    fn round_trips_and_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("nested/lib"));
        assert_eq!(store.load().unwrap(), LibraryData::default());
        store.save(&sample()).unwrap();
        assert_eq!(
            Store::new(dir.path().join("nested/lib")).load().unwrap(),
            sample()
        );
    }

    #[test]
    fn files_from_before_a_field_existed_still_load() {
        // A library written before history and settings were saved.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("library.json"),
            r#"{"schema_version": 1, "data": {"next_id": 1, "books": [{
                "id": 1, "title": "Old", "author": "", "status": "Finished",
                "current_page": 3, "total_pages": 3, "rating": null,
                "cover_file": null, "cover_url": null, "source": "Manual",
                "added_at_ms": 1, "started_at_ms": null, "finished_at_ms": 2,
                "updated_at_ms": 2}]}}"#,
        )
        .unwrap();
        let data = Store::new(dir.path()).load().unwrap();
        assert_eq!(data.books[0].title, "Old");
        assert_eq!(data.books[0].notes, "");
        assert!(data.activity.is_empty());
        assert!(data.settings.catalogue_enabled);
    }

    #[test]
    fn newer_and_corrupt_files_are_reported_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let newer = r#"{"schema_version": 7, "data": {}}"#;
        std::fs::write(store.library_file(), newer).unwrap();
        assert_eq!(
            store.load(),
            Err(LibraryError::NewerLibrary {
                found: 7,
                supported: STORE_VERSION
            })
        );
        assert!(
            store.save(&sample()).is_err(),
            "never overwrite a newer file"
        );
        assert_eq!(
            std::fs::read_to_string(store.library_file()).unwrap(),
            newer
        );

        std::fs::write(store.library_file(), "{ truncated").unwrap();
        assert!(matches!(store.load(), Err(LibraryError::Storage { .. })));
        let backup = store.set_aside(42).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{ truncated");
        assert_eq!(store.load().unwrap(), LibraryData::default());
        assert_eq!(store.set_aside(43).unwrap(), None);
    }

    #[test]
    fn covers_are_named_safely_and_orphans_cleaned() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let name = store.save_cover("ol/../OL1W", "jpg", b"img").unwrap();
        assert_eq!(name, "ol----OL1W.jpg");
        let orphan = store.save_cover("orphan", "png", b"x").unwrap();
        let mut data = sample();
        data.books[0].cover_file = Some(name.clone());
        assert_eq!(store.usage().0, 2);
        let book = store.save_book_file("epub-1", b"zip").unwrap();
        let stray = store.save_book_file("epub-2", b"zip").unwrap();
        data.books[0].epub_file = Some(book.clone());
        assert_eq!(store.usage().1, 2);
        assert_eq!(store.remove_orphan_covers(&data), 2);
        assert!(store.books_dir().join(&book).exists());
        assert!(!store.books_dir().join(stray).exists());
        assert_eq!(store.book_file("../library.json"), None);
        assert!(!store.covers_dir().join(orphan).exists());
        store.delete_cover("../library.json"); // ignored
        store.delete_cover(&name);
        assert_eq!(store.usage().0, 0);
    }
}
