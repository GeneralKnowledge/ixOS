//! The library through its exported API, as the host uses it.

use super::*;
use crate::catalogue::tests::{catalogue, API, COVERS, SEARCH_BODY};
use crate::epub::tests::sample_epub;
use forge_net::{Method, NetError, Response, StaticClient};
use forge_runtime::block_on;
use std::path::Path;

fn dir_string(dir: &Path) -> String {
    dir.to_string_lossy().into_owned()
}

/// A library whose catalogue never answers (as if offline).
fn open(dir: &Path) -> Arc<Library> {
    Library::open(dir_string(dir), Arc::new(OfflineCatalogue)).unwrap()
}

struct OfflineCatalogue;

impl BookCatalogue for OfflineCatalogue {
    fn search(&self, _: &str) -> Result<Vec<CatalogueHit>, LibraryError> {
        Err(NetError::Offline {
            reason: "no route to host".into(),
        }
        .into())
    }
    fn description(&self, _: &str) -> Result<Option<String>, LibraryError> {
        Err(LibraryError::Timeout)
    }
    fn cover(&self, _: &str) -> Result<(Vec<u8>, &'static str), LibraryError> {
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
        block_on(library.search_catalogue("dune".into())),
        Err(LibraryError::Offline { .. })
    ));
    assert_eq!(library.books(all()), before);
    assert_eq!(library.revision(), revision);
    // Imports need a result from a search.
    assert!(matches!(
        block_on(library.import_from_catalogue("/works/OL1W".into())),
        Err(LibraryError::InvalidInput { .. })
    ));
    assert!(matches!(
        block_on(library.search_catalogue(" d ".into())),
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

    let results = block_on(library.search_catalogue("hobbit".into())).unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].library_book_id, None);

    let imported = block_on(library.import_from_catalogue("/works/OL27482W".into())).unwrap();
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
    let again = block_on(library.search_catalogue("hobbit".into())).unwrap();
    assert_eq!(again[0].library_book_id, Some(book.id));
    assert_eq!(
        block_on(library.import_from_catalogue("/works/OL27482W".into())),
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
    block_on(library.search_catalogue("hobbit".into())).unwrap();
    let imported = block_on(library.import_from_catalogue("/works/OL27482W".into())).unwrap();
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
        block_on(library.search_catalogue("anything".into())),
        Err(LibraryError::CatalogueDisabled)
    );
}

#[test]
fn epub_import_saves_metadata_and_cover_only() {
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
    // The host may delete its copy; the library does not need it.
    std::fs::remove_file(&file).unwrap();
    assert_eq!(
        open(dir.path()).book(book.id).unwrap().title,
        "The Test Book"
    );

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
    // Failed imports leave no stray covers.
    assert_eq!(library.storage_info().saved_covers, 1);
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
}

#[test]
fn events_announce_saved_changes_and_end_on_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let events = library.events().unwrap();
    block_on(library.add_book(input("One", None))).unwrap();
    assert_eq!(
        block_on(events.next()),
        Some(LibraryEvent::Changed { revision: 1 })
    );
    let settings = LibrarySettings {
        default_sort: LibrarySort::Author,
        catalogue_enabled: true,
    };
    block_on(library.update_settings(settings.clone())).unwrap();
    assert_eq!(
        block_on(events.next()),
        Some(LibraryEvent::SettingsChanged { settings })
    );
    // A failed command announces nothing.
    assert!(block_on(library.update_progress(99, 1)).is_err());

    library.shutdown();
    assert_eq!(block_on(events.next()), None);
    assert_eq!(
        block_on(library.add_book(input("Late", None))),
        Err(LibraryError::ShutDown)
    );
    assert!(library.events().is_err());
    // Reads still work after shutdown: the host may render the last state.
    assert_eq!(library.books(all()).len(), 1);
}

#[test]
fn a_slow_subscriber_is_told_it_missed_events() {
    let dir = tempfile::tempdir().unwrap();
    let library = open(dir.path());
    let events = library.events().unwrap();
    for i in 0..(forge_runtime::DEFAULT_QUEUE_CAPACITY + 5) {
        block_on(library.add_book(input(&format!("Book {i}"), None))).unwrap();
    }
    assert!(matches!(
        block_on(events.next()),
        Some(LibraryEvent::EventsDropped { count: 5 })
    ));
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
    assert!(open(dir.path()).books(all()).is_empty());
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
    let mut ids: Vec<u64> = open(dir.path()).books(all()).iter().map(|b| b.id).collect();
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
