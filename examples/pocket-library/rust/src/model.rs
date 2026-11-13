//! The records Swift sees. Everything here is plain data: the rules that
//! change it live in `library.rs`.

use serde::{Deserialize, Serialize};

/// Where a book is in the reader's life.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReadingStatus {
    WantToRead,
    Reading,
    Finished,
}

/// How the library list is ordered.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LibrarySort {
    /// Newest first.
    #[default]
    RecentlyAdded,
    /// A–Z, ignoring a leading "The", "A" or "An".
    Title,
    /// A–Z by author, then title.
    Author,
    /// Most read first; books without a page count last.
    Progress,
}

/// What the library list shows: a search, an optional status filter and an order.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct LibraryQuery {
    /// Words that must each appear in the title or author (case-insensitive).
    pub text: String,
    /// `None` shows every status.
    pub status: Option<ReadingStatus>,
    pub sort: LibrarySort,
}

/// Where a book came from. Catalogue books remember their key, so searching
/// the catalogue again shows them as already in the library.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum BookSource {
    Manual,
    OpenLibrary { work_key: String },
    Epub { file_name: String },
}

/// One book, as the host shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Book {
    pub id: u64,
    pub title: String,
    pub author: String,
    pub description: String,
    pub status: ReadingStatus,
    pub current_page: u32,
    pub total_pages: Option<u32>,
    /// 0.0–1.0. Finished books are 1.0; books without a page count are 0.0
    /// unless finished.
    pub progress: f64,
    /// 1–5 stars.
    pub rating: Option<u8>,
    pub notes: String,
    /// A cover saved on the device. Absolute path, valid for this launch only
    /// (iOS moves the app's container between installs), so never store it.
    pub cover_file: Option<String>,
    /// A remote cover, used when there is no saved one.
    pub cover_url: Option<String>,
    pub source: BookSource,
    /// The book's page on Open Library, for catalogue books.
    pub web_page: Option<String>,
    /// The book's EPUB is saved, so it can be opened in the reader.
    pub can_read: bool,
    /// Where the reader was left, if the book has been opened in it.
    pub reading_position: Option<ReadingPosition>,
    /// Unix time in milliseconds.
    pub added_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub updated_at_ms: i64,
}

/// What the user typed in the add or edit form. The library validates it and
/// applies the status rules (see `Library::add_book`).
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct BookInput {
    pub title: String,
    pub author: String,
    pub description: String,
    pub status: ReadingStatus,
    pub current_page: u32,
    pub total_pages: Option<u32>,
    pub rating: Option<u8>,
    pub notes: String,
    /// `https://` only.
    pub cover_url: Option<String>,
}

/// One meaningful moment in the reading history.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Added,
    Started,
    /// Pages read in one sitting. Updates within an hour of each other are
    /// merged into one entry.
    Progress {
        from_page: u32,
        to_page: u32,
    },
    Finished,
    /// Moved back to Want to Read or Reading from another status.
    StatusChanged {
        status: ReadingStatus,
    },
}

#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActivityEntry {
    pub id: u64,
    pub book_id: u64,
    /// The title when it happened, so deleted books still read sensibly.
    pub book_title: String,
    pub at_ms: i64,
    pub kind: ActivityKind,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct LibraryStats {
    pub total: u32,
    pub want_to_read: u32,
    pub reading: u32,
    pub finished: u32,
    /// Finished since `year_start_ms` (passed by the host, which knows the
    /// user's calendar and time zone).
    pub finished_this_year: u32,
    /// Pages read in the 7 days before now.
    pub pages_last_7_days: u32,
    /// Mean of the rated books, 1–5.
    pub average_rating: Option<f64>,
}

#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LibrarySettings {
    /// The order the library opens with.
    pub default_sort: LibrarySort,
    /// Off: the app never contacts the online catalogue.
    pub catalogue_enabled: bool,
}

impl Default for LibrarySettings {
    fn default() -> Self {
        Self {
            default_sort: LibrarySort::RecentlyAdded,
            catalogue_enabled: true,
        }
    }
}

/// What a CSV import did.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct CsvImportSummary {
    pub added: u32,
    /// One line per row left out, saying where and why ("Line 4: ...").
    pub skipped: Vec<String>,
}

/// A search result from the online catalogue.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct CatalogueBook {
    /// The catalogue's id for the work (Open Library: `/works/OL…W`).
    pub key: String,
    pub title: String,
    pub author: String,
    pub first_published: Option<i32>,
    pub total_pages: Option<u32>,
    pub cover_url: Option<String>,
    /// The id of the book already imported from this result, if any.
    pub library_book_id: Option<u64>,
}

/// The result of importing a catalogue book. The book is saved even when the
/// extras could not be fetched; `warnings` says what is missing.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ImportedBook {
    pub book: Book,
    pub warnings: Vec<String>,
}

/// What `Settings` shows about local data.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct StorageInfo {
    pub directory: String,
    pub books: u32,
    pub activity_entries: u32,
    pub saved_covers: u32,
    /// EPUBs kept for reading.
    pub saved_books: u32,
    /// Library file plus saved covers and EPUBs.
    pub bytes: u64,
}

// MARK: Reading EPUBs

/// A place in a book: a chapter (an index into `ReaderContents::chapters`)
/// and how far through it, 0.0–1.0.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct ReadingPosition {
    pub chapter: u32,
    pub fraction: f64,
}

/// One chapter (one file of the EPUB's reading order).
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChapterSummary {
    pub index: u32,
    /// From the book's table of contents, else the chapter's first heading,
    /// else "Section N".
    pub title: String,
    /// Whether the book's own table of contents lists it. Files a chapter
    /// is split across, and title pages, usually are not.
    pub in_contents: bool,
    pub words: u32,
}

/// What the reader needs to open a book.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ReaderContents {
    pub book_id: u64,
    pub title: String,
    pub chapters: Vec<ChapterSummary>,
    pub total_words: u64,
    /// Where to open: the saved position, or the place matching the book's
    /// current page if that was changed by hand since.
    pub start: ReadingPosition,
}

/// A run of text in one style.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TextSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub monospace: bool,
}

/// What a block of a chapter is. The host chooses how each looks.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum TextBlockKind {
    /// `level` 1–6, as `<h1>`–`<h6>`.
    Heading {
        level: u8,
    },
    Paragraph,
    /// Inside a `<blockquote>`.
    Quote,
    /// `marker` is "•" or "3." and so on.
    ListItem {
        marker: String,
    },
    /// Keep line breaks and spacing; show in a fixed-width font.
    Preformatted,
    /// A scene break: `<hr>`, or a paragraph of only "* * *" and the like.
    Separator,
    /// An image saved for this launch at `file` (absolute path).
    Image {
        file: String,
        alt: String,
    },
}

/// One paragraph, heading, image… of a chapter, in reading order.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TextBlock {
    pub kind: TextBlockKind,
    /// Empty for separators and images. Line breaks are `\n`.
    pub spans: Vec<TextSpan>,
}

/// A chapter's text, ready to lay out.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ReaderChapter {
    pub index: u32,
    pub title: String,
    pub blocks: Vec<TextBlock>,
}
