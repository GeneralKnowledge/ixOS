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
    /// Library file plus saved covers.
    pub bytes: u64,
}

/// Something the host may want to react to.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum LibraryEvent {
    /// The library changed and was saved. Re-run the current query.
    Changed { revision: u64 },
    /// Settings changed.
    SettingsChanged { settings: LibrarySettings },
    /// This stream fell behind and missed `count` events; re-read everything.
    EventsDropped { count: u64 },
}
