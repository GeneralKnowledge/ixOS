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

use forge_storage::files::{safe_name, FileStore, Staged};
use forge_storage::{JsonFile, StorageError};

use crate::domain::LibraryData;
use crate::error::LibraryError;

/// Schema of `library.json`. Additive changes use `#[serde(default)]` and keep
/// this; a change that old files cannot be read as bumps it and adds a step to
/// [`migrate`].
pub const STORE_VERSION: u32 = 1;

/// The library file plus one [`FileStore`] per folder. The folders' file
/// handling (atomic writes, safe names, crash clean-up, usage) is Forge's;
/// what goes in each folder and when it is cleaned is the app's.
pub struct Store {
    dir: PathBuf,
    file: JsonFile<LibraryData>,
    covers: FileStore,
    books: FileStore,
    reader: FileStore,
    exports: FileStore,
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
        Self {
            covers: FileStore::new(dir.join("covers")),
            books: FileStore::new(dir.join("books")),
            reader: FileStore::new(dir.join("reader")),
            exports: FileStore::new(dir.join("exports")),
            dir,
            file,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn covers_dir(&self) -> &Path {
        self.covers.root()
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
    /// destroying it, keeping the newest three. Returns the backup's path
    /// (or `None` if there was no file).
    pub fn set_aside(&self, now_ms: i64) -> Result<Option<PathBuf>, LibraryError> {
        Ok(forge_storage::set_aside(self.file.path(), now_ms)?)
    }

    /// Stages cover bytes under a new name for a book about to be saved:
    /// commit it once the library is saved; dropping it deletes the file.
    pub fn stage_cover(
        &self,
        stem: &str,
        extension: &str,
        bytes: &[u8],
    ) -> Result<Staged, LibraryError> {
        let name = format!("{}.{}", safe_name(stem), safe_name(extension));
        Ok(self.covers.stage_write(&name, bytes)?)
    }

    /// Best effort: a cover that cannot be deleted only wastes space.
    /// Invalid names are ignored.
    pub fn delete_cover(&self, name: &str) {
        self.covers.remove(name);
    }

    /// Stages an EPUB in `books/` under a new name, like
    /// [`Self::stage_cover`].
    pub fn stage_book_file(&self, stem: &str, bytes: &[u8]) -> Result<Staged, LibraryError> {
        let name = format!("{}.epub", safe_name(stem));
        Ok(self.books.stage_write(&name, bytes)?)
    }

    /// The saved EPUB's path for this launch, if `name` is a valid name.
    pub fn book_file(&self, name: &str) -> Option<PathBuf> {
        self.books.path(name).ok()
    }

    /// Best effort, like [`Self::delete_cover`].
    pub fn delete_book_file(&self, name: &str) {
        self.books.remove(name);
    }

    /// Empties `reader/` (chapter images from an earlier book or launch).
    pub fn reset_reader(&self) {
        let _ = self.reader.clear();
    }

    /// Where the reader saves `book_id`'s chapter images. The folder is
    /// created on the first image.
    pub fn reader_images(&self, book_id: u64) -> FileStore {
        FileStore::new(self.reader.root().join(book_id.to_string()))
    }

    /// Deletes covers and EPUBs that no book refers to (left behind by a
    /// crash between saving the file and saving the library), temporaries
    /// left by a crash mid-write, and the reader's images. Returns how many
    /// covers and EPUBs went.
    pub fn remove_orphan_covers(&self, data: &LibraryData) -> u32 {
        self.reset_reader();
        let covers = self.covers.sweep(|name| {
            data.books
                .iter()
                .any(|b| b.cover_file.as_deref() == Some(name))
        });
        let books = self.books.sweep(|name| {
            data.books
                .iter()
                .any(|b| b.epub_file.as_deref() == Some(name))
        });
        u32::try_from(covers + books).unwrap_or(u32::MAX)
    }

    /// Saves an export for the share sheet. Only the latest one is kept.
    pub fn write_export(&self, file_name: &str, bytes: &[u8]) -> Result<PathBuf, LibraryError> {
        let _ = self.exports.clear();
        Ok(self.exports.write(file_name, bytes)?.path)
    }

    /// Deletes covers, EPUBs, reader images and exports (the caller saves
    /// an empty library).
    pub fn clear_files(&self) {
        for files in [&self.covers, &self.books, &self.reader, &self.exports] {
            let _ = files.clear();
        }
    }

    /// (number of saved covers, number of saved EPUBs, bytes of
    /// library.json + covers + EPUBs)
    pub fn usage(&self) -> (u32, u32, u64) {
        let file_bytes = std::fs::metadata(self.file.path())
            .map(|m| m.len())
            .unwrap_or(0);
        let covers = self.covers.usage();
        let books = self.books.usage();
        let count = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
        (
            count(covers.files),
            count(books.files),
            file_bytes + covers.bytes + books.bytes,
        )
    }
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
        let name = store.stage_cover("ol/../OL1W", "jpg", b"img").unwrap();
        let name = name.commit().name;
        assert_eq!(name, "ol----OL1W.jpg");
        let orphan = store
            .stage_cover("orphan", "png", b"x")
            .unwrap()
            .commit()
            .name;
        let mut data = sample();
        data.books[0].cover_file = Some(name.clone());
        assert_eq!(store.usage().0, 2);
        let book = store
            .stage_book_file("epub-1", b"zip")
            .unwrap()
            .commit()
            .name;
        let stray = store
            .stage_book_file("epub-2", b"zip")
            .unwrap()
            .commit()
            .name;
        data.books[0].epub_file = Some(book.clone());
        assert_eq!(store.usage().1, 2);
        assert_eq!(store.remove_orphan_covers(&data), 2);
        assert!(store.book_file(&book).unwrap().exists());
        assert!(!store.book_file(&stray).unwrap().exists());
        assert_eq!(store.book_file("../library.json"), None);
        assert!(!store.covers_dir().join(orphan).exists());
        store.delete_cover("../library.json"); // ignored
        store.delete_cover(&name);
        assert_eq!(store.usage().0, 0);
    }

    #[test]
    fn a_crash_mid_write_is_cleaned_at_launch() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        store.stage_cover("kept", "jpg", b"img").unwrap().commit();
        // A temporary from another process that died before renaming it.
        let partial = store.covers_dir().join(".partial-999999999-0");
        std::fs::write(&partial, b"half").unwrap();
        assert_eq!(store.usage().0, 1, "temporaries are not covers");
        store.remove_orphan_covers(&LibraryData::default());
        assert!(!partial.exists());
        assert_eq!(store.usage().0, 0);
    }

    /// Every `fixtures/library-v<N>.json` (a library as version N of the
    /// app saved it) loads with today's code and saves again, and there is
    /// one for the current [`STORE_VERSION`]. Old fixtures are never edited.
    #[test]
    fn every_saved_version_still_opens() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let mut versions = Vec::new();
        for entry in std::fs::read_dir(&fixtures).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(version) = name
                .strip_prefix("library-v")
                .and_then(|rest| rest.strip_suffix(".json"))
            else {
                continue;
            };
            versions.push(version.parse::<u32>().unwrap());
            let dir = tempfile::tempdir().unwrap();
            std::fs::copy(entry.path(), dir.path().join("library.json")).unwrap();
            let store = Store::new(dir.path());
            let data = store
                .load()
                .unwrap_or_else(|e| panic!("{name} does not load: {e}"));
            assert!(!data.books.is_empty(), "{name} lost its books");
            assert!(!data.activity.is_empty(), "{name} lost its history");
            store.save(&data).unwrap();
            assert_eq!(Store::new(dir.path()).load().unwrap(), data, "{name}");
        }
        assert!(
            versions.contains(&STORE_VERSION),
            "add fixtures/library-v{STORE_VERSION}.json before releasing"
        );
    }
}
