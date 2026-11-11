//! CSV import and export, and backup archives.

use std::fs;

use forge_storage::backup::{self, Backup};
use forge_storage::csv_file::{export_csv, from_csv, import_csv, to_csv};
use forge_storage::{Database, StorageError};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Book {
    title: String,
    author: String,
    page_count: Option<u32>,
    finished: bool,
}

fn book(title: &str, pages: Option<u32>) -> Book {
    Book {
        title: title.into(),
        author: "Ann Échelle".into(),
        page_count: pages,
        finished: false,
    }
}

#[test]
fn csv_round_trips_with_a_bom_and_quoting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("books.csv");
    let books = vec![
        book("A, \"quoted\" title", Some(320)),
        book("Line\nbreak", None),
    ];
    export_csv(&path, &books).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"\xEF\xBB\xBFtitle,author,page_count,finished\n"));
    let read = import_csv::<Book>(&path).unwrap();
    assert_eq!(read.rows, books);
    assert!(read.problems.is_empty());
}

#[test]
fn csv_import_is_forgiving_and_reports_bad_rows_by_line() {
    // A spreadsheet's export: other header spellings and order, an extra
    // column, a blank line, and one bad number.
    let csv = "Finished,Title,Author,Page Count,Notes\n\
               true,Dune,Frank Herbert,412,loved it\n\
               \n\
               false,Emma,Jane Austen,lots,\n\
               false,Persuasion,Jane Austen,,\n";
    let read = from_csv::<Book>(csv.as_bytes()).unwrap();
    assert_eq!(read.rows.len(), 2);
    assert_eq!(
        read.rows[0],
        Book {
            title: "Dune".into(),
            author: "Frank Herbert".into(),
            page_count: Some(412),
            finished: true
        }
    );
    assert_eq!(read.rows[1].page_count, None);
    assert_eq!(read.problems.len(), 1);
    assert_eq!(read.problems[0].line, 4);
    assert!(
        read.problems[0].reason.contains("column 4"),
        "{}",
        read.problems[0].reason
    );
    assert!(from_csv::<Book>(b"").is_err());
    assert_eq!(to_csv::<Book>(&[]).unwrap(), b"\xEF\xBB\xBF");
}

fn app_data(dir: &std::path::Path) -> Database {
    fs::create_dir_all(dir.join("covers/2026")).unwrap();
    fs::write(dir.join("settings.json"), br#"{"sort":"title"}"#).unwrap();
    fs::write(dir.join("covers/2026/dune.jpg"), vec![7u8; 100_000]).unwrap();
    fs::write(dir.join("covers/.partial-upload"), b"skip me").unwrap();
    let db = Database::open(dir.join("library.db"), &["CREATE TABLE books (title TEXT)"])
        .unwrap()
        .0;
    db.connection()
        .execute("INSERT INTO books VALUES ('Dune')", [])
        .unwrap();
    db
}

#[test]
fn a_backup_restores_into_a_new_directory_and_swaps_in() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("Library");
    let db = app_data(&data);
    let archive = dir.path().join("exports/Library backup.zip");
    let manifest = Backup::new("org.example.library", 2, 1_000)
        .database("library.db", &db)
        .unwrap()
        .file("settings.json", &data.join("settings.json"))
        .file("missing.json", &data.join("missing.json"))
        .directory("covers", &data.join("covers"))
        .unwrap()
        .write(&archive)
        .unwrap();
    let names: Vec<_> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        names,
        ["library.db", "settings.json", "covers/2026/dune.jpg"]
    );
    assert_eq!(backup::read_manifest(&archive).unwrap(), manifest);

    // Another app's archive, and newer data, are refused before anything is written.
    let restored = dir.path().join("Library.restored");
    assert!(backup::restore(&archive, "org.example.other", 2, &restored).is_err());
    assert!(matches!(
        backup::restore(&archive, "org.example.library", 1, &restored),
        Err(StorageError::UnsupportedSchemaVersion {
            found: 2,
            supported: 1
        })
    ));
    assert!(!restored.exists());

    backup::restore(&archive, "org.example.library", 2, &restored).unwrap();
    assert_eq!(
        fs::read(restored.join("covers/2026/dune.jpg"))
            .unwrap()
            .len(),
        100_000
    );
    let copy = Database::open(
        restored.join("library.db"),
        &["CREATE TABLE books (title TEXT)"],
    )
    .unwrap()
    .0;
    let title: String = copy
        .connection()
        .query_row("SELECT title FROM books", [], |r| r.get(0))
        .unwrap();
    assert_eq!(title, "Dune");

    // The data changed since; restoring puts the backup back and keeps it.
    drop(db);
    fs::write(data.join("settings.json"), b"changed").unwrap();
    let kept = backup::replace_directory(&data, &restored, 5)
        .unwrap()
        .unwrap();
    assert_eq!(
        fs::read(data.join("settings.json")).unwrap(),
        br#"{"sort":"title"}"#
    );
    assert_eq!(fs::read(kept.join("settings.json")).unwrap(), b"changed");
    assert!(!restored.exists());
}

#[test]
fn a_damaged_or_foreign_archive_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("b.zip");
    // Data that doesn't compress, so it fills most of the archive.
    let mut seed = 1u32;
    let noise: Vec<u8> = (0..60_000)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 24) as u8
        })
        .collect();
    Backup::new("app", 1, 0)
        .bytes("a.bin", noise)
        .write(&archive)
        .unwrap();

    // Flip a byte inside the stored file: the zip CRC or our hash catches it.
    let mut bytes = fs::read(&archive).unwrap();
    let at = bytes.len() / 3;
    bytes[at] ^= 1;
    let damaged = dir.path().join("damaged.zip");
    fs::write(&damaged, &bytes).unwrap();
    let into = dir.path().join("out");
    assert!(backup::restore(&damaged, "app", 1, &into).is_err());
    assert!(!into.exists());
    assert!(!dir.path().join("out.restoring").exists());

    // Not a zip at all, and unsafe names on the way in.
    fs::write(dir.path().join("note.txt"), b"not a zip").unwrap();
    assert!(backup::read_manifest(&dir.path().join("note.txt")).is_err());
    assert!(Backup::new("app", 1, 0)
        .bytes("../escape", vec![])
        .write(&dir.path().join("x.zip"))
        .is_err());
    assert!(Backup::new("app", 1, 0)
        .bytes("a", vec![])
        .bytes("a", vec![])
        .write(&dir.path().join("y.zip"))
        .is_err());
    assert!(!dir.path().join("x.zip").exists());
}
