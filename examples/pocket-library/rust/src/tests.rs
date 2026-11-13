//! The library through its exported API, as the host uses it.

use super::*;
use crate::catalogue::tests::{catalogue, API, COVERS, SEARCH_BODY};
use crate::epub::tests::sample_epub;
use forge_app::RuntimeEvent;
use forge_net::{Method, NetError, Response, StaticClient};
use forge_runtime::{block_on, CancelHandle};
use std::path::Path;

fn dir_string(dir: &Path) -> String {
    dir.to_string_lossy().into_owned()
}

/// Closes `library` and opens what it saved, as the next launch would.
fn reopen(library: Arc<Library>, dir: &Path) -> Arc<Library> {
    library.shutdown();
    drop(library);
    open(dir)
}

/// A library whose catalogue never answers (as if offline).
fn open(dir: &Path) -> Arc<Library> {
    Library::open(dir_string(dir), Arc::new(OfflineCatalogue)).unwrap()
}

struct OfflineCatalogue;

impl BookCatalogue for OfflineCatalogue {
    fn search(&self, _: &str, _: &CancelHandle) -> Result<Vec<CatalogueHit>, LibraryError> {
        Err(NetError::Offline {
            reason: "no route to host".into(),
        }
        .into())
    }
    fn description(&self, _: &str, _: &CancelHandle) -> Result<Option<String>, LibraryError> {
        Err(LibraryError::Timeout)
    }
    fn cover(&self, _: &str, _: &CancelHandle) -> Result<(Vec<u8>, &'static str), LibraryError> {
        Err(LibraryError::Timeout)
    }
}

fn input(title: &str, total: Option<u32>) -> BookInput {
    BookInput {
        title: title.into(),
        author: "Author".into(),
        description: String::new(),
        status: ReadingStatus::WantToRead,
        current_page: 0,
        total_pages: total,
        rating: None,
        notes: String::new(),
        cover_url: None,
    }
}

fn all() -> LibraryQuery {
    LibraryQuery {
        text: String::new(),
        status: None,
        sort: LibrarySort::RecentlyAdded,
    }
}

#[test]
fn everything_survives_a_relaunch() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let dune = block_on(library.add_book(input("Dune", Some(400)))).unwrap();
    let emma = block_on(library.add_book(input("Emma", None))).unwrap();
    block_on(library.update_progress(dune.id, 120)).unwrap();
    block_on(library.update_settings(LibrarySettings {
        default_sort: LibrarySort::Title,
        catalogue_enabled: false,
    }))
    .unwrap();
    block_on(library.delete_book(emma.id)).unwrap();
    let before = (
        library.books(all()),
        library.history(None, 100),
        library.settings(),
    );
    // The host terminates the app.
    library.shutdown();
    drop(library);

    let reopened = open(dir.path());
    let after = (
        reopened.books(all()),
        reopened.history(None, 100),
        reopened.settings(),
    );
    assert_eq!(before, after);
    let dune = reopened.book(dune.id).unwrap();
    assert_eq!(
        (dune.status, dune.current_page),
        (ReadingStatus::Reading, 120)
    );
    assert_eq!(reopened.settings().default_sort, LibrarySort::Title);
    assert_eq!(
        reopened.book(emma.id),
        Err(LibraryError::NotFound { id: emma.id })
    );
}

#[test]
fn offline_the_library_works_and_lookups_fail_without_harm() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let book = block_on(library.add_book(input("Offline book", Some(10)))).unwrap();
    let edited = block_on(library.update_book(
        book.id,
        BookInput {
            notes: "written on a plane".into(),
            ..input("Offline book", Some(10))
        },
    ))
    .unwrap();
    assert_eq!(edited.notes, "written on a plane");
    let revision = library.revision();
    let before = library.books(all());

    assert!(matches!(
        block_on(library.search_catalogue("dune".into(), None)),
        Err(LibraryError::Offline { .. })
    ));
    assert_eq!(library.books(all()), before);
    assert_eq!(library.revision(), revision);
    // Imports need a result from a search.
    assert!(matches!(
        block_on(library.import_from_catalogue("/works/OL1W".into(), None)),
        Err(LibraryError::InvalidInput { .. })
    ));
    assert!(matches!(
        block_on(library.search_catalogue(" d ".into(), None)),
        Err(LibraryError::InvalidInput { .. })
    ));
}

#[test]
fn search_then_import_saves_description_and_cover() {
    let dir = tempfile::tempdir().unwrap();
    let probe = catalogue(StaticClient::new());
    let client = StaticClient::new()
        .route(
            Method::Get,
            probe.search_url("hobbit"),
            Ok(Response::new(200, SEARCH_BODY)),
        )
        .route(
            Method::Get,
            format!("{API}/works/OL27482W.json"),
            Ok(Response::new(
                200,
                r#"{"description": "There and back again."}"#,
            )),
        )
        .route(
            Method::Get,
            format!("{COVERS}/b/id/14627509-L.jpg?default=false"),
            Ok(Response::new(200, vec![0xFF, 0xD8, 0xFF, 0xE0, 9, 9])),
        );
    let library = Library::open(dir_string(dir.path()), Arc::new(catalogue(client))).unwrap();

    let results = block_on(library.search_catalogue("hobbit".into(), None)).unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].library_book_id, None);

    let imported = block_on(library.import_from_catalogue("/works/OL27482W".into(), None)).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let book = imported.book;
    assert_eq!(book.title, "The Hobbit");
    assert_eq!(book.description, "There and back again.");
    assert_eq!(book.total_pages, Some(310));
    assert_eq!(book.status, ReadingStatus::WantToRead);
    let cover = book.cover_file.clone().unwrap();
    assert!(cover.starts_with(&dir_string(dir.path())), "{cover}");
    assert_eq!(std::fs::read(&cover).unwrap()[..3], [0xFF, 0xD8, 0xFF]);

    // Searching again marks it; importing it twice is refused.
    let again = block_on(library.search_catalogue("hobbit".into(), None)).unwrap();
    assert_eq!(again[0].library_book_id, Some(book.id));
    assert_eq!(
        block_on(library.import_from_catalogue("/works/OL27482W".into(), None)),
        Err(LibraryError::AlreadyInLibrary { id: book.id })
    );

    // Deleting the book deletes its saved cover.
    block_on(library.delete_book(book.id)).unwrap();
    assert!(!Path::new(&cover).exists());
}

#[test]
fn a_partial_import_keeps_the_book_and_says_what_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let probe = catalogue(StaticClient::new());
    // Search works; the description times out and the cover is missing (404).
    let client = StaticClient::new()
        .route(
            Method::Get,
            probe.search_url("hobbit"),
            Ok(Response::new(200, SEARCH_BODY)),
        )
        .route(
            Method::Get,
            format!("{API}/works/OL27482W.json"),
            Err(NetError::Timeout),
        );
    let library = Library::open(dir_string(dir.path()), Arc::new(catalogue(client))).unwrap();
    block_on(library.search_catalogue("hobbit".into(), None)).unwrap();
    let imported = block_on(library.import_from_catalogue("/works/OL27482W".into(), None)).unwrap();
    assert_eq!(imported.warnings.len(), 2, "{:?}", imported.warnings);
    assert_eq!(imported.book.description, "");
    assert_eq!(imported.book.cover_file, None);
    // The remote cover is still there for when the device is online.
    assert_eq!(
        imported.book.cover_url.as_deref(),
        Some(format!("{COVERS}/b/id/14627509-M.jpg").as_str())
    );
}

#[test]
fn the_catalogue_can_be_switched_off() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    block_on(library.update_settings(LibrarySettings {
        catalogue_enabled: false,
        ..LibrarySettings::default()
    }))
    .unwrap();
    assert_eq!(
        block_on(library.search_catalogue("anything".into(), None)),
        Err(LibraryError::CatalogueDisabled)
    );
}

#[test]
fn epub_import_saves_metadata_cover_and_file() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let file = dir.path().join("The Test Book.epub");
    std::fs::write(&file, sample_epub("The Test Book", 1000, true)).unwrap();

    let book = block_on(library.import_epub(dir_string(&file))).unwrap();
    assert_eq!(book.title, "The Test Book");
    assert_eq!(book.author, "Ada Writer, Bo Coauthor");
    assert_eq!(book.total_pages, Some(8));
    assert_eq!(
        book.source,
        BookSource::Epub {
            file_name: "The Test Book.epub".into()
        }
    );
    assert!(Path::new(book.cover_file.as_ref().unwrap()).exists());
    assert!(book.can_read);
    assert_eq!(book.reading_position, None);
    // The host may delete its copy; the library keeps its own.
    std::fs::remove_file(&file).unwrap();
    let library = reopen(library, dir.path());
    assert_eq!(library.book(book.id).unwrap().title, "The Test Book");

    std::fs::write(&file, sample_epub("The Test Book", 10, false)).unwrap();
    assert_eq!(
        block_on(library.import_epub(dir_string(&file))),
        Err(LibraryError::AlreadyInLibrary { id: book.id })
    );
    std::fs::write(&file, b"not a zip").unwrap();
    assert!(matches!(
        block_on(library.import_epub(dir_string(&file))),
        Err(LibraryError::UnsupportedFile { .. })
    ));
    assert!(matches!(
        block_on(library.import_epub(dir_string(&dir.path().join("missing.epub")))),
        Err(LibraryError::UnsupportedFile { .. })
    ));
    // Failed imports leave no stray covers or EPUBs.
    let info = library.storage_info();
    assert_eq!((info.saved_covers, info.saved_books), (1, 1));
}

/// A two-chapter EPUB of 1000 words each (plus a heading word).
fn import_readable(library: &Library, dir: &Path, title: &str) -> Book {
    let file = dir.join(format!("{title}.epub"));
    std::fs::write(&file, sample_epub(title, 1000, true)).unwrap();
    block_on(library.import_epub(dir_string(&file))).unwrap()
}

#[test]
fn an_imported_epub_opens_in_the_reader() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let book = import_readable(&library, dir.path(), "Readable");

    let contents = block_on(library.open_reader(book.id)).unwrap();
    assert_eq!(contents.title, "Readable");
    assert_eq!(contents.chapters.len(), 2);
    assert_eq!(contents.chapters[0].title, "Chapter");
    assert_eq!(contents.chapters[0].words, 1001);
    assert_eq!(contents.total_words, 2002);
    assert_eq!(
        contents.start,
        ReadingPosition {
            chapter: 0,
            fraction: 0.0
        }
    );

    let chapter = block_on(library.reader_chapter(book.id, 1)).unwrap();
    assert_eq!(chapter.index, 1);
    assert_eq!(chapter.blocks[0].kind, TextBlockKind::Heading { level: 1 });
    assert_eq!(chapter.blocks[1].kind, TextBlockKind::Paragraph);
    assert!(chapter.blocks[1].spans[0].text.starts_with("word word"));
    assert!(matches!(
        block_on(library.reader_chapter(book.id, 2)),
        Err(LibraryError::InvalidInput { .. })
    ));
}

#[test]
fn reading_moves_the_page_and_is_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let book = import_readable(&library, dir.path(), "Progress");
    assert_eq!(book.total_pages, Some(8));
    assert!(
        matches!(
            block_on(library.save_reading_position(
                book.id,
                ReadingPosition {
                    chapter: 0,
                    fraction: 0.5
                }
            )),
            Err(LibraryError::InvalidInput { .. })
        ),
        "the reader must be open first"
    );
    block_on(library.open_reader(book.id)).unwrap();

    // Halfway through chapter 2 of 2 is three quarters of the book.
    let middle = ReadingPosition {
        chapter: 1,
        fraction: 0.5,
    };
    let book = block_on(library.save_reading_position(book.id, middle)).unwrap();
    assert_eq!(book.status, ReadingStatus::Reading);
    assert_eq!(book.current_page, 6);
    assert_eq!(book.reading_position, Some(middle));
    assert!(matches!(
        library.history(Some(book.id), 10)[0].kind,
        ActivityKind::Progress {
            from_page: 0,
            to_page: 6
        }
    ));

    // The very end stops a page short: finishing is the reader's choice.
    let end = ReadingPosition {
        chapter: 1,
        fraction: 1.0,
    };
    let book = block_on(library.save_reading_position(book.id, end)).unwrap();
    assert_eq!(
        (book.status, book.current_page),
        (ReadingStatus::Reading, 7)
    );

    // Reopened later, the reader starts where it was left.
    library.shutdown();
    drop(library);
    let library = open(dir.path());
    let contents = block_on(library.open_reader(book.id)).unwrap();
    assert_eq!(contents.start, end);

    // A page typed in by hand wins over the saved position.
    let book = block_on(library.update_progress(book.id, 2)).unwrap();
    let start = block_on(library.open_reader(book.id)).unwrap().start;
    assert_eq!(start.chapter, 0);
    assert!((start.fraction - 0.5).abs() < 0.01, "{start:?}");

    // A finished book keeps its page and status while it is re-read.
    block_on(library.set_status(book.id, ReadingStatus::Finished)).unwrap();
    let book = block_on(library.save_reading_position(book.id, middle)).unwrap();
    assert_eq!(
        (book.status, book.current_page),
        (ReadingStatus::Finished, 8)
    );
    assert_eq!(book.reading_position, Some(middle));
    assert_eq!(
        block_on(library.open_reader(book.id)).unwrap().start,
        middle
    );
}

#[test]
fn books_without_an_epub_cannot_be_opened_until_one_is_imported() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let mut typed = input("Attach Me", Some(300));
    typed.author = "Ada Writer, Bo Coauthor".into();
    typed.status = ReadingStatus::Reading;
    typed.current_page = 150;
    let book = block_on(library.add_book(typed)).unwrap();
    assert!(!book.can_read);
    assert!(matches!(
        block_on(library.open_reader(book.id)),
        Err(LibraryError::NotReadable { .. })
    ));

    // Importing the same book's EPUB attaches it, keeping the progress.
    let attached = import_readable(&library, dir.path(), "Attach Me");
    assert_eq!(attached.id, book.id);
    assert!(attached.can_read);
    assert_eq!(
        (attached.current_page, attached.total_pages),
        (150, Some(300))
    );
    assert_eq!(library.storage_info().books, 1);
    // …and the reader opens at that page: halfway, so chapter 2's start.
    let start = block_on(library.open_reader(book.id)).unwrap().start;
    assert_eq!(start.chapter, 1);
    assert!(start.fraction < 0.01, "{start:?}");
    // A second import is refused as before.
    let file = dir.path().join("again.epub");
    std::fs::write(&file, sample_epub("Attach Me", 10, false)).unwrap();
    assert_eq!(
        block_on(library.import_epub(dir_string(&file))),
        Err(LibraryError::AlreadyInLibrary { id: book.id })
    );
}

#[test]
fn deleting_a_book_deletes_its_epub() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let book = import_readable(&library, dir.path(), "Gone");
    block_on(library.open_reader(book.id)).unwrap();
    assert_eq!(library.storage_info().saved_books, 1);
    block_on(library.delete_book(book.id)).unwrap();
    assert_eq!(library.storage_info().saved_books, 0);
    assert!(matches!(
        block_on(library.reader_chapter(book.id, 0)),
        Err(LibraryError::InvalidInput { .. })
    ));
}

/// Imports a real EPUB when `POCKET_LIBRARY_EPUB` names one (the book files
/// themselves are not part of the repository). Prints what was read.
#[test]
fn imports_a_real_epub_when_given_one() {
    let Ok(path) = std::env::var("POCKET_LIBRARY_EPUB") else {
        eprintln!("skipped: set POCKET_LIBRARY_EPUB=/path/to/book.epub");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let book = block_on(library.import_epub(path)).unwrap();
    eprintln!(
        "{:?} by {:?}: {:?} pages, cover {:?}, description {:?}",
        book.title, book.author, book.total_pages, book.cover_file, book.description
    );
    assert!(!book.title.is_empty());

    let contents = block_on(library.open_reader(book.id)).unwrap();
    eprintln!("{} words", contents.total_words);
    for chapter in &contents.chapters {
        let text = block_on(library.reader_chapter(book.id, chapter.index)).unwrap();
        eprintln!(
            "{:>3} {:<40} listed={} words={} blocks={}",
            chapter.index,
            chapter.title,
            chapter.in_contents,
            chapter.words,
            text.blocks.len()
        );
    }
    if std::env::var("POCKET_LIBRARY_DUMP").is_ok() {
        let first = contents.chapters.iter().find(|c| c.words > 200).unwrap();
        let text = block_on(library.reader_chapter(book.id, first.index)).unwrap();
        for block in text.blocks.iter().take(25) {
            eprintln!("{block:?}");
        }
    }
}

#[test]
fn events_announce_saved_changes_and_end_on_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let events = library.events().unwrap();
    block_on(library.add_book(input("One", None))).unwrap();
    assert_eq!(
        block_on(events.next()),
        Some(RuntimeEvent::Changed { revision: 1 })
    );
    let settings = LibrarySettings {
        default_sort: LibrarySort::Author,
        catalogue_enabled: true,
    };
    block_on(library.update_settings(settings.clone())).unwrap();
    assert_eq!(
        block_on(events.next()),
        Some(RuntimeEvent::Changed { revision: 2 })
    );
    assert_eq!(library.settings(), settings);
    // A failed command announces nothing and keeps the revision.
    assert!(block_on(library.update_progress(99, 1)).is_err());
    assert_eq!(library.revision(), 2);

    library.shutdown();
    assert!(matches!(
        block_on(events.next()),
        Some(RuntimeEvent::PhaseChanged { .. })
    ));
    assert_eq!(block_on(events.next()), None);
    assert_eq!(
        block_on(library.add_book(input("Late", None))),
        Err(LibraryError::ShutDown)
    );
    assert!(library.events().is_none());
    // Reads still work after shutdown: the host may render the last state.
    assert_eq!(library.books(all()).len(), 1);
}

#[test]
fn a_second_core_cannot_open_the_same_library() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    assert!(matches!(
        Library::open(dir_string(dir.path()), Arc::new(OfflineCatalogue)),
        Err(LibraryError::InUse { .. })
    ));
    drop(library);
    open(dir.path());
}

#[test]
fn a_failed_save_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let book = block_on(library.add_book(input("Kept", Some(100)))).unwrap();
    // Another build wrote a newer file underneath us: saves must refuse.
    std::fs::write(
        dir.path().join("library.json"),
        r#"{"schema_version": 99, "data": {}}"#,
    )
    .unwrap();
    assert!(matches!(
        block_on(library.update_progress(book.id, 50)),
        Err(LibraryError::NewerLibrary { .. })
    ));
    assert_eq!(library.book(book.id).unwrap().current_page, 0);
    assert_eq!(library.revision(), 1);
}

#[test]
fn an_unreadable_library_can_be_set_aside() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("library.json"),
        "{\"schema_version\": 1, \"data\": [",
    )
    .unwrap();
    assert!(matches!(
        Library::new(dir_string(dir.path())),
        Err(LibraryError::Storage { .. })
    ));
    let fresh = Library::open_fresh(dir_string(dir.path())).unwrap();
    assert!(fresh.books(all()).is_empty());
    let backups: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("library.unreadable-")
        })
        .collect();
    assert_eq!(backups.len(), 1);

    // A newer library is never set aside.
    let newer = tempfile::tempdir().unwrap();
    std::fs::write(
        newer.path().join("library.json"),
        r#"{"schema_version": 50, "data": {}}"#,
    )
    .unwrap();
    assert!(matches!(
        Library::open_fresh(dir_string(newer.path())),
        Err(LibraryError::NewerLibrary { .. })
    ));
    assert!(matches!(
        Library::new("  ".into()),
        Err(LibraryError::InvalidInput { .. })
    ));
}

#[test]
fn export_and_erase() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let file = dir.path().join("b.epub");
    std::fs::write(&file, sample_epub("Covered", 10, true)).unwrap();
    block_on(library.import_epub(dir_string(&file))).unwrap();
    block_on(library.add_book(input("Plain", None))).unwrap();

    let path = block_on(library.export_library()).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(json["format"], "pocket-library-export");
    assert_eq!(json["library"]["books"].as_array().unwrap().len(), 2);

    let info = library.storage_info();
    assert_eq!((info.books, info.saved_covers), (2, 1));
    assert!(info.bytes > 0);

    block_on(library.update_settings(LibrarySettings {
        default_sort: LibrarySort::Progress,
        catalogue_enabled: true,
    }))
    .unwrap();
    block_on(library.erase_library()).unwrap();
    let info = library.storage_info();
    assert_eq!(
        (info.books, info.saved_covers, info.activity_entries),
        (0, 0, 0)
    );
    assert_eq!(library.settings().default_sort, LibrarySort::Progress);
    assert!(reopen(library, dir.path()).books(all()).is_empty());
}

#[test]
fn concurrent_commands_all_land() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let futures: Vec<_> = (0..16)
        .map(|i| {
            let library = Arc::clone(&library);
            std::thread::spawn(move || block_on(library.add_book(input(&format!("B{i}"), None))))
        })
        .collect();
    for f in futures {
        f.join().unwrap().unwrap();
    }
    let mut ids: Vec<u64> = reopen(library, dir.path())
        .books(all())
        .iter()
        .map(|b| b.id)
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, (1..=16).collect::<Vec<_>>());
}

#[test]
fn default_query_uses_the_preferred_order() {
    let q = default_query(LibrarySettings {
        default_sort: LibrarySort::Title,
        catalogue_enabled: true,
    });
    assert_eq!(q.sort, LibrarySort::Title);
    assert!(q.text.is_empty() && q.status.is_none());
}

/// A catalogue whose requests hang until they are cancelled, like a server
/// that never answers. Searches after the first `release` answer at once.
struct HangingCatalogue {
    started: std::sync::mpsc::SyncSender<()>,
}

impl HangingCatalogue {
    fn hang(&self, cancel: &CancelHandle) -> LibraryError {
        let _ = self.started.try_send(());
        if cancel.wait_timeout(std::time::Duration::from_secs(30)) {
            LibraryError::Cancelled
        } else {
            LibraryError::Timeout
        }
    }
}

impl BookCatalogue for HangingCatalogue {
    fn search(&self, _: &str, cancel: &CancelHandle) -> Result<Vec<CatalogueHit>, LibraryError> {
        Err(self.hang(cancel))
    }
    fn description(&self, _: &str, cancel: &CancelHandle) -> Result<Option<String>, LibraryError> {
        Err(self.hang(cancel))
    }
    fn cover(
        &self,
        _: &str,
        cancel: &CancelHandle,
    ) -> Result<(Vec<u8>, &'static str), LibraryError> {
        Err(self.hang(cancel))
    }
}

fn open_hanging(dir: &Path) -> (Arc<Library>, std::sync::mpsc::Receiver<()>) {
    let (started, on_start) = std::sync::mpsc::sync_channel(8);
    let library = Library::open(dir_string(dir), Arc::new(HangingCatalogue { started })).unwrap();
    (library, on_start)
}

/// Runs `call` on another thread; returns its result and how long it took
/// after `trigger` ran (once the request had started).
fn stop_in_flight<T: Send + 'static>(
    on_start: &std::sync::mpsc::Receiver<()>,
    call: impl FnOnce() -> T + Send + 'static,
    trigger: impl FnOnce(),
) -> (T, std::time::Duration) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(call()).unwrap());
    on_start
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the request started");
    let stopped_at = std::time::Instant::now();
    trigger();
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the request stopped promptly");
    (result, stopped_at.elapsed())
}

#[test]
fn a_cancelled_search_stops_at_once_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (library, on_start) = open_hanging(dir.path());
    let cancellation = Cancellation::new();
    let (result, took) = {
        let library = Arc::clone(&library);
        let passed = Arc::clone(&cancellation);
        stop_in_flight(
            &on_start,
            move || block_on(library.search_catalogue("dune".into(), Some(passed))),
            || cancellation.cancel(),
        )
    };
    assert_eq!(result, Err(LibraryError::Cancelled));
    assert!(took < std::time::Duration::from_secs(2), "took {took:?}");
    // The library still works, and other requests are not affected.
    assert_eq!(library.revision(), 0);
    block_on(library.add_book(input("Dune", None))).unwrap();
}

#[test]
fn a_cancelled_import_adds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (started, on_start) = std::sync::mpsc::sync_channel(8);
    let probe = catalogue(StaticClient::new());
    let client = StaticClient::new().route(
        Method::Get,
        probe.search_url("hobbit"),
        Ok(Response::new(200, SEARCH_BODY)),
    );
    // Search with a working catalogue, then import through a hanging one,
    // reusing the first library's results.
    let library = Library::open(dir_string(dir.path()), Arc::new(catalogue(client))).unwrap();
    block_on(library.search_catalogue("hobbit".into(), None)).unwrap();
    let last_search = lock(&library.last_search).clone();
    library.shutdown();
    drop(library);
    let library = Library::open(
        dir_string(dir.path()),
        Arc::new(HangingCatalogue { started }),
    )
    .unwrap();
    *lock(&library.last_search) = last_search;

    let cancellation = Cancellation::new();
    let (result, _) = {
        let library = Arc::clone(&library);
        let passed = Arc::clone(&cancellation);
        stop_in_flight(
            &on_start,
            move || block_on(library.import_from_catalogue("/works/OL27482W".into(), Some(passed))),
            || cancellation.cancel(),
        )
    };
    assert_eq!(result.unwrap_err(), LibraryError::Cancelled);
    assert_eq!(library.books(all()), vec![]);
}

#[test]
fn shutdown_stops_a_request_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let (library, on_start) = open_hanging(dir.path());
    let (result, took) = {
        let searching = Arc::clone(&library);
        stop_in_flight(
            &on_start,
            move || block_on(searching.search_catalogue("dune".into(), None)),
            || library.shutdown(),
        )
    };
    assert_eq!(result, Err(LibraryError::ShutDown));
    assert!(took < std::time::Duration::from_secs(2), "took {took:?}");
}

#[test]
fn books_go_out_and_come_back_as_a_spreadsheet() {
    let dir = tempfile::tempdir().unwrap();
    let lib = open(dir.path());
    let mut dune = input("Dune", Some(412));
    dune.author = "Frank Herbert".into();
    dune.current_page = 100;
    dune.status = ReadingStatus::Reading;
    block_on(lib.add_book(dune)).unwrap();
    let path = block_on(lib.export_books_csv()).unwrap();
    let csv = std::fs::read_to_string(&path).unwrap();
    assert!(
        csv.contains("Title,Author,Status,Pages Read,Total Pages,Rating,Notes"),
        "{csv}"
    );
    assert!(
        csv.contains("Dune,Frank Herbert,Reading,100,412,,"),
        "{csv}"
    );

    // Another app's spreadsheet: different columns, one bad row.
    let other = dir.path().join("other.csv");
    std::fs::write(
        &other,
        "title,author,status,rating\nEmma,Jane Austen,read,5\nNo Name,,Lost,\n,Nobody,,\n",
    )
    .unwrap();
    let summary = block_on(lib.import_books_csv(other.to_string_lossy().into_owned())).unwrap();
    assert_eq!(summary.added, 1);
    assert_eq!(summary.skipped.len(), 2, "{:?}", summary.skipped);
    let emma = lib
        .books(all())
        .into_iter()
        .find(|b| b.title == "Emma")
        .unwrap();
    assert_eq!(
        (emma.status, emma.rating),
        (ReadingStatus::Finished, Some(5))
    );
}

#[test]
fn the_background_interrupts_a_search_but_not_an_import() {
    let dir = tempfile::tempdir().unwrap();
    let (library, on_start) = open_hanging(dir.path());
    let (result, _) = {
        let searching = Arc::clone(&library);
        stop_in_flight(
            &on_start,
            move || block_on(searching.search_catalogue("dune".into(), None)),
            || library.set_phase(forge_app::AppPhase::Background),
        )
    };
    assert_eq!(result.unwrap_err(), LibraryError::Interrupted);
    assert_eq!(
        LibraryError::Interrupted.kind(),
        forge_app::ErrorKind::Interrupted
    );

    // An import is finishing work: it carries on in the background, and
    // the host holds background time until it ends or the time runs out.
    *lock(&library.last_search) = HashMap::from([(
        "/works/OL1W".to_string(),
        CatalogueHit {
            key: "/works/OL1W".into(),
            title: "Dune".into(),
            author: "Frank Herbert".into(),
            first_published: None,
            total_pages: None,
            cover_url: None,
            large_cover_url: None,
        },
    )]);
    let (result, _) = {
        let importing = Arc::clone(&library);
        stop_in_flight(
            &on_start,
            move || block_on(importing.import_from_catalogue("/works/OL1W".into(), None)),
            || {
                assert!(library.background_work_pending());
                library.background_time_expired();
            },
        )
    };
    assert_eq!(result.unwrap_err(), LibraryError::Interrupted);
    assert!(!library.background_work_pending());
    assert_eq!(library.books(all()), vec![]);
}
