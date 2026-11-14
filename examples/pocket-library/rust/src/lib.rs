//! Pocket Library's Rust core: an offline-first reading tracker.
//!
//! Everything the SwiftUI host calls is in this file's `#[uniffi::export]`
//! blocks; `forge build` turns them into Swift (`import PocketLibraryFFI`).
//! The rules are in [`domain`], the files in [`store`], the online catalogue
//! in [`catalogue`], EPUB import in [`epub`] and the reader's text in
//! [`reader`].
//!
//! Built on the application runtime (`forge_app`), like Checklist:
//!
//! - **Commands** (adding, editing and deleting books, progress, settings,
//!   the last step of every import) run one at a time on the runtime's
//!   command thread. Each saves before it returns; the runtime then raises
//!   the revision and announces `Changed`, and the host re-reads.
//! - **Queries** (`books`, `stats`, …) read the last saved library
//!   ([`Published`]) and never wait for a command, so they are cheap
//!   enough for the UI thread.
//! - **Work** that is slow or touches the network runs on its own thread
//!   in a runtime scope: catalogue search is on-screen work (stopped by the
//!   background), imports and exports are finishing work (the host asks iOS
//!   for time to finish them), and the reader's file reads may simply pause
//!   while the app is suspended.
//! - **The runtime contract** (`events`, `set_phase`, `revision`, background
//!   work, the launch report, `shutdown`) comes from
//!   `forge_app::export_runtime!`; ForgeKit's `ForgeHost` drives it.

pub mod catalogue;
mod csv_rows;
pub mod domain;
pub mod epub;
mod error;
mod model;
pub mod reader;
pub mod store;
pub mod summary;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use forge_app::{Cancellation, DataDirectory, ErrorKind, Published, Runtime};
use forge_net::UreqClient;
use forge_runtime::{lock, now_ms};

pub use catalogue::{BookCatalogue, CatalogueHit, OpenLibrary};
use domain::{fold, validate, LibraryData, StoredBookRun, StoredBookSynopsis, StoredSynopsis};
pub use error::LibraryError;
use forge_storage::Staged;
pub use model::*;
use reader::EpubText;
use store::Store;
pub use summary::{
    BookRunProgress, BookSynopsis, CastMember, ChapterSynopsis, SummaryModel, SummaryModelError,
    SummaryModelInfo, SummaryProgress, SummaryRequest, SummarySink, SummaryStage,
};

// No namespace argument: UniFFI takes the library's name (pocket_library_core), so this
// crate keeps its own bindings when another app links it.
uniffi::setup_scaffolding!();

/// Sent with every catalogue request. Open Library asks clients to identify
/// themselves.
pub const USER_AGENT: &str = concat!(
    "PocketLibrary/",
    env!("CARGO_PKG_VERSION"),
    " (Forge example app; https://github.com/GeneralKnowledge/ixOS)"
);

/// What the command thread owns: the saved library and its files.
struct State {
    data: LibraryData,
    store: Arc<Store>,
    published: Arc<Published<LibraryData>>,
}

impl State {
    /// Applies `change` to a copy of the library, saves it, and only then
    /// makes it current. A failed change or save changes nothing.
    fn commit<T>(
        &mut self,
        change: impl FnOnce(&mut LibraryData, &Store, i64) -> Result<T, LibraryError>,
    ) -> Result<T, LibraryError> {
        let mut next = self.data.clone();
        let out = change(&mut next, &self.store, now_ms())?;
        self.store.save(&next)?;
        self.published.set(next.clone());
        self.data = next;
        Ok(out)
    }

    /// A book as the host sees it.
    fn book(&self, id: u64) -> Result<Book, LibraryError> {
        Ok(self.data.book(id)?.to_book(self.store.covers_dir()))
    }
}

/// The library the host opens once and shares. Thread-safe.
#[derive(uniffi::Object)]
pub struct Library {
    rt: Runtime<State>,
    /// The last saved library, for queries.
    data: Arc<Published<LibraryData>>,
    store: Arc<Store>,
    catalogue: Arc<dyn BookCatalogue>,
    /// The latest catalogue results, so an import needs only the key.
    last_search: Arc<Mutex<HashMap<String, CatalogueHit>>>,
    /// The book open in the reader, so turning a page reads one chapter
    /// instead of the whole EPUB again.
    open_book: Arc<Mutex<Option<OpenBook>>>,
}

struct OpenBook {
    id: u64,
    epub_file: String,
    text: EpubText,
}

/// What an EPUB import does once the file has been read and staged.
enum EpubPlan {
    /// Give a book already in the library its EPUB.
    Attach { id: u64, epub_file: Staged },
    /// Add a new book.
    Add {
        input: Box<domain::ValidInput>,
        file_name: String,
        cover_file: Option<Staged>,
        epub_file: Staged,
    },
}

#[uniffi::export]
impl Library {
    /// Opens the library in `storage_directory` (created if missing),
    /// holding the directory so no second instance can write to it. Fails
    /// with `Storage` or `NewerLibrary` if the saved library cannot be used;
    /// the file is left untouched (see `open_fresh`).
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, LibraryError> {
        Self::open(storage_directory, default_catalogue())
    }

    /// Recovery for an unreadable library: moves `library.json` aside (it is
    /// kept, renamed `library.unreadable-<time>.json`) and opens an empty
    /// library. Refuses to touch a library from a newer app version.
    #[uniffi::constructor]
    pub fn open_fresh(storage_directory: String) -> Result<Arc<Self>, LibraryError> {
        Self::open_with(storage_directory, default_catalogue(), true)
    }

    // MARK: Reading (memory only)

    /// The books matching `query`, in its order.
    pub fn books(&self, query: LibraryQuery) -> Vec<Book> {
        let data = self.data.get();
        let covers = self.store.covers_dir();
        data.query(&query)
            .into_iter()
            .map(|b| b.to_book(covers))
            .collect()
    }

    pub fn book(&self, id: u64) -> Result<Book, LibraryError> {
        Ok(self.data.get().book(id)?.to_book(self.store.covers_dir()))
    }

    /// Counts and reading totals. `year_start_ms` is the start of the
    /// current year in the user's time zone.
    pub fn stats(&self, year_start_ms: i64) -> LibraryStats {
        self.data.get().stats(year_start_ms, now_ms())
    }

    /// Reading history, newest first; `book_id` limits it to one book.
    pub fn history(&self, book_id: Option<u64>, limit: u32) -> Vec<ActivityEntry> {
        self.data.get().history(book_id, limit as usize)
    }

    pub fn settings(&self) -> LibrarySettings {
        self.data.get().settings.clone()
    }

    // MARK: Changing the library

    /// Adds a book typed in by the user.
    pub async fn add_book(&self, input: BookInput) -> Result<Book, LibraryError> {
        let input = validate(input)?;
        self.commit_book("add-book", move |data, _, now| {
            data.add(input, BookSource::Manual, None, now)
        })
        .await
    }

    /// Saves the edit form. Status and page follow the same rules as
    /// `update_progress` (the last page finishes a book, and so on).
    pub async fn update_book(&self, id: u64, input: BookInput) -> Result<Book, LibraryError> {
        let input = validate(input)?;
        self.commit_book("update-book", move |data, _, now| {
            data.update(id, input, now)?;
            Ok(id)
        })
        .await
    }

    /// Records the page the reader is on.
    pub async fn update_progress(&self, id: u64, page: u32) -> Result<Book, LibraryError> {
        self.commit_book("update-progress", move |data, _, now| {
            let status = data.book(id)?.status;
            let status = if status == ReadingStatus::Finished {
                ReadingStatus::Reading
            } else {
                status
            };
            data.set_reading_state(id, status, page, now)?;
            Ok(id)
        })
        .await
    }

    /// Moves a book to `status`: Want to Read resets the page, Finished
    /// jumps to the last page.
    pub async fn set_status(&self, id: u64, status: ReadingStatus) -> Result<Book, LibraryError> {
        self.commit_book("set-status", move |data, _, now| {
            let page = match status {
                ReadingStatus::WantToRead => 0,
                _ => data.book(id)?.current_page,
            };
            data.set_reading_state(id, status, page, now)?;
            Ok(id)
        })
        .await
    }

    /// Deletes a book, its saved cover and its EPUB. Its history stays.
    pub async fn delete_book(&self, id: u64) -> Result<(), LibraryError> {
        let open_book = Arc::clone(&self.open_book);
        self.rt
            .command("delete-book", move |state| {
                let removed = state.commit(|data, _, _| data.delete(id))?;
                // The library no longer names them: a crash before this
                // leaves files the next launch sweeps.
                if let Some(cover) = &removed.cover_file {
                    state.store.delete_cover(cover);
                }
                if let Some(epub) = &removed.epub_file {
                    let mut open = lock(&open_book);
                    if open.as_ref().is_some_and(|o| o.id == id) {
                        *open = None;
                    }
                    state.store.delete_book_file(epub);
                }
                Ok(())
            })
            .await
    }

    // MARK: Online catalogue

    /// Searches the online catalogue. Nothing in the library changes; on any
    /// failure the library is exactly as it was. On-screen work:
    /// `cancellation` stops it part way with `Cancelled`, and leaving the app
    /// with `Interrupted`.
    pub async fn search_catalogue(
        &self,
        query: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Vec<CatalogueBook>, LibraryError> {
        self.ensure_catalogue_enabled()?;
        let query = query.trim().to_string();
        if query.chars().count() < 2 {
            return Err(LibraryError::invalid(
                "type at least two characters to search",
            ));
        }
        let catalogue = Arc::clone(&self.catalogue);
        let last_search = Arc::clone(&self.last_search);
        let data = Arc::clone(&self.data);
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("search-catalogue", work, move |cancel| {
                let hits = catalogue.search(&query, cancel)?;
                *lock(&last_search) = hits.iter().map(|h| (h.key.clone(), h.clone())).collect();
                let data = data.get();
                Ok(hits
                    .into_iter()
                    .map(|hit| CatalogueBook {
                        library_book_id: data
                            .book_with_source(&BookSource::OpenLibrary {
                                work_key: hit.key.clone(),
                            })
                            .map(|b| b.id),
                        key: hit.key,
                        title: hit.title,
                        author: hit.author,
                        first_published: hit.first_published,
                        total_pages: hit.total_pages,
                        cover_url: hit.cover_url,
                    })
                    .collect())
            })
            .await
    }

    /// Adds a result of the latest search as a Want to Read book. Its
    /// description and a cover are fetched too; if they fail the book is
    /// still added, without them, and `warnings` says so. Finishing work:
    /// it may complete after the person leaves the app. `cancellation`
    /// stops it part way with `Cancelled`, and then nothing is added.
    pub async fn import_from_catalogue(
        &self,
        key: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<ImportedBook, LibraryError> {
        self.ensure_catalogue_enabled()?;
        let source = BookSource::OpenLibrary {
            work_key: key.clone(),
        };
        if let Some(existing) = self.data.get().book_with_source(&source) {
            return Err(LibraryError::AlreadyInLibrary { id: existing.id });
        }
        let hit = lock(&self.last_search).get(&key).cloned().ok_or_else(|| {
            LibraryError::invalid("that result is no longer available; search again")
        })?;
        let catalogue = Arc::clone(&self.catalogue);
        let store = Arc::clone(&self.store);
        let work = self.rt.finishing(cancellation.as_deref());
        let (input, cover_file, warnings) = self
            .rt
            .run("import-catalogue", work, move |cancel| {
                let mut warnings = Vec::new();
                let description = match catalogue.description(&hit.key, cancel) {
                    Ok(text) => text.unwrap_or_default(),
                    Err(LibraryError::Cancelled) => return Err(LibraryError::Cancelled),
                    Err(err) => {
                        warnings.push(format!("The description could not be loaded ({err})."));
                        String::new()
                    }
                };
                let cover_file = match &hit.large_cover_url {
                    None => None,
                    Some(url) => match catalogue.cover(url, cancel).and_then(|(bytes, ext)| {
                        store.stage_cover(&format!("ol-{}", hit.key), ext, &bytes)
                    }) {
                        Ok(staged) => Some(staged),
                        Err(LibraryError::Cancelled) => return Err(LibraryError::Cancelled),
                        Err(err) => {
                            warnings.push(format!(
                                "The cover could not be saved for offline use ({err})."
                            ));
                            None
                        }
                    },
                };
                let input = validate(BookInput {
                    title: hit.title.clone(),
                    author: hit.author.clone(),
                    description,
                    status: ReadingStatus::WantToRead,
                    current_page: 0,
                    total_pages: hit.total_pages,
                    rating: None,
                    notes: String::new(),
                    cover_url: hit.cover_url.clone(),
                })?;
                Ok((input, cover_file, warnings))
            })
            .await?;
        let book = self
            .rt
            .command("import-catalogue", move |state| {
                add_book_with_files(state, input, source, cover_file, None)
            })
            .await?;
        Ok(ImportedBook { book, warnings })
    }

    // MARK: EPUB import

    /// Adds the book in the EPUB at `path` as Want to Read: title, authors,
    /// description and cover from the file, and a page count estimated from
    /// its length. A copy of the file is kept so the book can be read in the
    /// app (`open_reader`); the host may delete its own copy afterwards.
    /// Finishing work: it may complete after the person leaves the app.
    ///
    /// If the book is already in the library without a saved EPUB (typed
    /// in, found in the catalogue, or imported before EPUBs were kept), the
    /// file is attached to that book instead, keeping its progress, and that
    /// book is returned. Otherwise a second import fails with
    /// `AlreadyInLibrary`.
    pub async fn import_epub(&self, path: String) -> Result<Book, LibraryError> {
        let store = Arc::clone(&self.store);
        let data = Arc::clone(&self.data);
        let work = self.rt.finishing(None);
        let plan = self
            .rt
            .run("import-epub", work, move |_| {
                let path = std::path::PathBuf::from(path);
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "book.epub".into());
                let size = std::fs::metadata(&path)
                    .map_err(|e| LibraryError::UnsupportedFile {
                        reason: format!("cannot read {file_name}: {e}"),
                    })?
                    .len();
                if size > epub::MAX_EPUB_BYTES as u64 {
                    return Err(LibraryError::UnsupportedFile {
                        reason: "the file is larger than 100 MB".into(),
                    });
                }
                let bytes = std::fs::read(&path).map_err(|e| LibraryError::UnsupportedFile {
                    reason: format!("cannot read {file_name}: {e}"),
                })?;
                let meta = epub::read_metadata(bytes.clone())?;
                // Check now that the reader can open it, so an import never
                // promises a book that cannot be read.
                EpubText::open(std::io::Cursor::new(&bytes))?;
                let existing = {
                    let (title, author) = (fold(&meta.title), fold(&meta.author));
                    data.get()
                        .books
                        .iter()
                        .find(|b| fold(&b.title) == title && fold(&b.author) == author)
                        .map(|b| (b.id, b.epub_file.is_some()))
                };
                let stem = format!("epub-{}", now_ms());
                match existing {
                    Some((id, true)) => Err(LibraryError::AlreadyInLibrary { id }),
                    Some((id, false)) => Ok(EpubPlan::Attach {
                        id,
                        epub_file: store.stage_book_file(&stem, &bytes)?,
                    }),
                    None => {
                        // Staged files are deleted again if anything below,
                        // or the command that saves the book, fails.
                        let cover_file = match &meta.cover {
                            Some(cover) => {
                                Some(store.stage_cover(&stem, cover.extension, &cover.bytes)?)
                            }
                            None => None,
                        };
                        let epub_file = store.stage_book_file(&stem, &bytes)?;
                        let input = validate(BookInput {
                            title: meta.title,
                            author: meta.author,
                            description: meta.description,
                            status: ReadingStatus::WantToRead,
                            current_page: 0,
                            total_pages: meta.estimated_pages,
                            rating: None,
                            notes: String::new(),
                            cover_url: None,
                        })?;
                        Ok(EpubPlan::Add {
                            input: Box::new(input),
                            file_name,
                            cover_file,
                            epub_file,
                        })
                    }
                }
            })
            .await?;
        self.rt
            .command("import-epub", move |state| match plan {
                EpubPlan::Attach { id, epub_file } => {
                    // Another import may have given it one meanwhile.
                    if state.data.book(id)?.epub_file.is_some() {
                        return Err(LibraryError::AlreadyInLibrary { id });
                    }
                    state
                        .commit(|data, _, _| data.attach_epub(id, Some(epub_file.name().into())))?;
                    epub_file.commit();
                    state.book(id)
                }
                EpubPlan::Add {
                    input,
                    file_name,
                    cover_file,
                    epub_file,
                } => add_book_with_files(
                    state,
                    *input,
                    BookSource::Epub { file_name },
                    cover_file,
                    Some(epub_file),
                ),
            })
            .await
    }

    // MARK: Reading

    /// Opens a book's saved EPUB for reading: its chapters, and where to
    /// start (see `ReaderContents::start`). Fails with `NotReadable` for a
    /// book without one. Images of earlier chapters are deleted.
    pub async fn open_reader(&self, id: u64) -> Result<ReaderContents, LibraryError> {
        let (data, store) = (Arc::clone(&self.data), Arc::clone(&self.store));
        let open_book = Arc::clone(&self.open_book);
        let work = self.rt.lifetime(None);
        self.rt
            .run("open-reader", work, move |_| {
                let (title, epub_file) = {
                    let data = data.get();
                    let book = data.book(id)?;
                    (book.title.clone(), book.epub_file.clone())
                };
                let epub_file = epub_file.ok_or_else(|| LibraryError::NotReadable {
                    reason: "there is no EPUB for this book; import its EPUB file to read it here"
                        .into(),
                })?;
                let mut open = lock(&open_book);
                let reuse = open
                    .as_ref()
                    .is_some_and(|o| o.id == id && o.epub_file == epub_file);
                if !reuse {
                    let path =
                        store
                            .book_file(&epub_file)
                            .ok_or_else(|| LibraryError::NotReadable {
                                reason: "the saved EPUB's name is not valid".into(),
                            })?;
                    *open = Some(OpenBook {
                        id,
                        epub_file,
                        text: EpubText::open_file(&path)?,
                    });
                }
                store.reset_reader();
                let book = open.as_ref().expect("opened above");
                let (words, chapters) = (book.text.chapter_words(), book.text.chapters().to_vec());
                drop(open);
                let start = data.get().reader_start(id, &words)?;
                Ok(ReaderContents {
                    book_id: id,
                    title,
                    chapters,
                    total_words: words.iter().map(|&w| u64::from(w)).sum(),
                    start,
                })
            })
            .await
    }

    /// The text of chapter `index` of a book opened with `open_reader`.
    pub async fn reader_chapter(&self, id: u64, index: u32) -> Result<ReaderChapter, LibraryError> {
        let store = Arc::clone(&self.store);
        let open_book = Arc::clone(&self.open_book);
        let work = self.rt.lifetime(None);
        self.rt
            .run("reader-chapter", work, move |_| {
                let mut open = lock(&open_book);
                let book = open
                    .as_mut()
                    .filter(|o| o.id == id)
                    .ok_or_else(|| LibraryError::invalid("open the book in the reader first"))?;
                book.text.chapter(index, &store.reader_images(id))
            })
            .await
    }

    /// Saves where the reader is, and moves the book's current page to
    /// match: reading in the app starts the book and counts as pages read,
    /// like typing the page in. It never finishes a book; the reader offers
    /// "Mark as Finished" at the end. A finished book keeps its page.
    pub async fn save_reading_position(
        &self,
        id: u64,
        position: ReadingPosition,
    ) -> Result<Book, LibraryError> {
        let words = {
            let open = lock(&self.open_book);
            open.as_ref()
                .filter(|o| o.id == id)
                .map(|o| o.text.chapter_words())
                .ok_or_else(|| LibraryError::invalid("open the book in the reader first"))?
        };
        self.commit_book("save-reading-position", move |data, _, now| {
            data.save_reading_position(id, position, &words, now)?;
            Ok(id)
        })
        .await
    }

    // MARK: Chapter summaries

    /// The saved summary of the chapter that reader chapter `index` is part
    /// of (a chapter can span several files; see `summary::section`), for
    /// a book opened with `open_reader`. None if there is none for the
    /// book's current EPUB.
    pub async fn chapter_synopsis(
        &self,
        id: u64,
        index: u32,
    ) -> Result<Option<ChapterSynopsis>, LibraryError> {
        let (data, open_book) = (Arc::clone(&self.data), Arc::clone(&self.open_book));
        let work = self.rt.lifetime(None);
        self.rt
            .run("chapter-synopsis", work, move |_| {
                let start = {
                    let open = lock(&open_book);
                    let book = open.as_ref().filter(|o| o.id == id).ok_or_else(|| {
                        LibraryError::invalid("open the book in the reader first")
                    })?;
                    summary::section(book.text.chapters(), index).start
                };
                Ok(data
                    .get()
                    .synopsis(id, start)
                    .map(StoredSynopsis::to_synopsis))
            })
            .await
    }

    /// Writes a summary of the chapter that reader chapter `index` is part
    /// of, with `model` (Apple's on-device model, from Swift), saves it in
    /// place of any earlier one, and returns it. Long chapters are read in
    /// parts that fit the model's small window (see `summary`); `progress`
    /// hears each stage and the summary as it is written. On-screen work:
    /// `cancellation` stops it with `Cancelled`, leaving the app with
    /// `Interrupted`, and either way nothing is saved.
    pub async fn summarise_chapter(
        &self,
        id: u64,
        index: u32,
        model: Arc<dyn SummaryModel>,
        progress: Option<Arc<dyn SummaryProgress>>,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<ChapterSynopsis, LibraryError> {
        let info = model.describe();
        if !info.available {
            return Err(LibraryError::SummaryUnavailable {
                reason: info
                    .unavailable_reason
                    .unwrap_or_else(|| format!("{} is not available", info.name)),
            });
        }
        let (data, open_book) = (Arc::clone(&self.data), Arc::clone(&self.open_book));
        let work = self.rt.foreground(cancellation.as_deref());
        let synopsis = self
            .rt
            .run("summarise-chapter", work, move |cancel| {
                if let Some(progress) = &progress {
                    progress.changed(SummaryStage::Opening);
                }
                let (start, title, epub_file, paragraphs) = {
                    let mut open = lock(&open_book);
                    let book = open.as_mut().filter(|o| o.id == id).ok_or_else(|| {
                        LibraryError::invalid("open the book in the reader first")
                    })?;
                    let range = summary::section(book.text.chapters(), index);
                    let chapters = &book.text.chapters()[range.start as usize..range.end as usize];
                    let words: u32 = chapters.iter().map(|c| c.words).sum();
                    if words < summary::MIN_WORDS {
                        return Err(LibraryError::invalid(
                            "this section is too short to summarise",
                        ));
                    }
                    if words > summary::MAX_WORDS {
                        return Err(LibraryError::invalid(format!(
                            "this chapter is too long to summarise on the phone ({words} words; \
                             the most is {})",
                            summary::MAX_WORDS
                        )));
                    }
                    let title = chapters[0].title.clone();
                    let mut paragraphs = Vec::new();
                    for i in range.clone() {
                        paragraphs.extend(book.text.chapter_text(i)?);
                    }
                    (range.start, title, book.epub_file.clone(), paragraphs)
                };
                let (book_title, author) = {
                    let data = data.get();
                    let book = data.book(id)?;
                    (book.title.clone(), book.author.clone())
                };
                let written = summary::Summariser {
                    model: model.as_ref(),
                    cancel,
                    progress,
                    requests: Default::default(),
                    source: summary::Source {
                        book_title,
                        author,
                        chapter_title: title.clone(),
                    },
                }
                .run(&paragraphs)?;
                Ok(StoredSynopsis {
                    chapter: start,
                    title,
                    epub_file,
                    text: written.text,
                    parts: written.parts,
                    skipped_passages: written.skipped_passages,
                    model_name: info.name,
                    created_at_ms: now_ms(),
                })
            })
            .await?;
        self.rt
            .command("save-summary", move |state| {
                let out = synopsis.to_synopsis();
                state.commit(|data, _, _| data.save_synopsis(id, synopsis))?;
                Ok(out)
            })
            .await
    }

    /// The saved whole-book summary, with every saved chapter summary.
    pub fn book_synopsis(&self, id: u64) -> Option<BookSynopsis> {
        self.data.get().book_synopsis(id)
    }

    /// Where an unfinished whole-book run got to, if one did: the next
    /// `summarise_book` carries on from there.
    pub fn book_run(&self, id: u64) -> Option<BookRunProgress> {
        self.data.get().book_run(id).map(|r| BookRunProgress {
            chapters_done: r.sections_done,
            chapters: r.sections,
            seconds: r.seconds,
            model_requests: r.model_requests,
        })
    }

    /// The whole-book "swarm": every chapter is read (summaries already
    /// saved are reused, and each new one is saved as soon as it is done,
    /// so a stopped run resumes where it left off), a cast keeper updates
    /// the list of people after each chapter, and the overview is written
    /// from the chapter summaries and the cast. A chapter the model
    /// declines is skipped and counted. Takes minutes. It keeps going when
    /// the app leaves the screen (the host asks iOS for background time;
    /// iOS may still suspend it, or the model may refuse to work in the
    /// background, which stops it with `Interrupted`). After every chapter
    /// it saves where it got to (`book_run`), so the next call carries on
    /// from the next chapter with the cast and counts so far.
    pub async fn summarise_book(
        &self,
        id: u64,
        model: Arc<dyn SummaryModel>,
        progress: Option<Arc<dyn SummaryProgress>>,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<BookSynopsis, LibraryError> {
        let info = model.describe();
        if !info.available {
            return Err(LibraryError::SummaryUnavailable {
                reason: info
                    .unavailable_reason
                    .unwrap_or_else(|| format!("{} is not available", info.name)),
            });
        }
        let started = std::time::Instant::now();
        let (book_title, author, epub_file) = {
            let data = self.data.get();
            let book = data.book(id)?;
            let epub = book
                .epub_file
                .clone()
                .ok_or_else(|| LibraryError::NotReadable {
                    reason: "there is no EPUB for this book; import its EPUB file to summarise it"
                        .into(),
                })?;
            (book.title.clone(), book.author.clone(), epub)
        };
        let store = Arc::clone(&self.store);
        let path = store
            .book_file(&epub_file)
            .ok_or_else(|| LibraryError::NotReadable {
                reason: "the saved EPUB's name is not valid".into(),
            })?;
        let text = self
            .rt
            .run(
                "open-book-text",
                self.rt.lifetime(cancellation.as_deref()),
                move |_| EpubText::open_file(&path).map(|t| Arc::new(Mutex::new(t))),
            )
            .await?;
        let chapters = lock(&text).chapters().to_vec();
        let sections = summary::book_sections(&chapters);
        if sections.is_empty() {
            return Err(LibraryError::invalid(
                "this book has no chapters to summarise",
            ));
        }
        let total = sections.len() as u32;
        // Carry on from an unfinished run of this EPUB with as many sections.
        let resumed = self
            .data
            .get()
            .book_run(id)
            .filter(|r| r.sections == total)
            .cloned();
        let source = |chapter_title: &str| summary::Source {
            book_title: book_title.clone(),
            author: author.clone(),
            chapter_title: chapter_title.to_string(),
        };
        let mut cast: Vec<CastMember> = Vec::new();
        let (mut requests, mut skipped, mut earlier_seconds, mut done) = (0u32, 0u32, 0u32, 0u32);
        if let Some(run) = &resumed {
            cast = run.cast.clone();
            requests = run.model_requests;
            skipped = run.chapters_skipped;
            earlier_seconds = run.seconds;
            done = run.sections_done;
        }
        let seconds = |started: &std::time::Instant| {
            earlier_seconds
                .saturating_add(started.elapsed().as_secs().min(u64::from(u32::MAX)) as u32)
        };
        // The summaries of sections already done, for the overview.
        let mut summaries: Vec<String> = {
            let data = self.data.get();
            sections[..done as usize]
                .iter()
                .filter_map(|range| {
                    data.synopsis(id, range.start)
                        .map(|s| format!("{}: {}", s.title, s.text))
                })
                .collect()
        };
        for (n, range) in sections.into_iter().enumerate().skip(done as usize) {
            let title = chapters[range.start as usize].title.clone();
            // A summary saved before the checker existed may be the model
            // declining in words: read that chapter again.
            let saved = self
                .data
                .get()
                .synopsis(id, range.start)
                .filter(|s| !s.text.split("\n\n").any(summary::declines))
                .cloned();
            if let Some(progress) = &progress {
                progress.changed(SummaryStage::Chapter {
                    chapter: n as u32 + 1,
                    chapters: total,
                    title: title.clone(),
                    saved: saved.is_some(),
                });
            }
            let chapter_text = match saved {
                Some(saved) => saved.text,
                None => {
                    let (model, progress, text) =
                        (Arc::clone(&model), progress.clone(), Arc::clone(&text));
                    let (source, range2) = (source(&title), range.clone());
                    let work = self.rt.lifetime(cancellation.as_deref());
                    let result = self
                        .rt
                        .run("summarise-book-chapter", work, move |cancel| {
                            let mut paragraphs = Vec::new();
                            {
                                let mut text = lock(&text);
                                for i in range2 {
                                    paragraphs.extend(text.chapter_text(i)?);
                                }
                            }
                            let summariser = summary::Summariser {
                                model: model.as_ref(),
                                cancel,
                                progress,
                                requests: Default::default(),
                                source,
                            };
                            let written = summariser.run(&paragraphs);
                            Ok::<_, LibraryError>((written, summariser.requests.get()))
                        })
                        .await?;
                    requests += result.1;
                    match result.0 {
                        Ok(written) => {
                            let synopsis = StoredSynopsis {
                                chapter: range.start,
                                title: title.clone(),
                                epub_file: epub_file.clone(),
                                text: written.text.clone(),
                                parts: written.parts,
                                skipped_passages: written.skipped_passages,
                                model_name: info.name.clone(),
                                created_at_ms: now_ms(),
                            };
                            self.rt
                                .command("save-summary", move |state| {
                                    state.commit(|data, _, _| data.save_synopsis(id, synopsis))
                                })
                                .await?;
                            written.text
                        }
                        Err(
                            LibraryError::SummaryRefused { .. } | LibraryError::InvalidInput { .. },
                        ) => {
                            skipped += 1;
                            continue;
                        }
                        Err(err) => return Err(err),
                    }
                }
            };
            // The cast keeper's turn.
            let (model, progress, current) = (Arc::clone(&model), progress.clone(), cast.clone());
            let (source2, title2, text2) = (source(&title), title.clone(), chapter_text.clone());
            let work = self.rt.lifetime(cancellation.as_deref());
            let (updated, used) = self
                .rt
                .run("update-cast", work, move |cancel| {
                    let summariser = summary::Summariser {
                        model: model.as_ref(),
                        cancel,
                        progress,
                        requests: Default::default(),
                        source: source2,
                    };
                    let cast = summariser.update_cast(&current, &title2, &text2)?;
                    Ok::<_, LibraryError>((cast, summariser.requests.get()))
                })
                .await?;
            cast = updated;
            requests += used;
            summaries.push(format!("{title}: {chapter_text}"));
            self.save_book_run(
                id,
                &epub_file,
                n as u32 + 1,
                total,
                &cast,
                summaries.len() as u32,
                skipped,
                requests,
                seconds(&started),
            )
            .await?;
        }
        if summaries.is_empty() {
            return Err(LibraryError::SummaryRefused {
                reason: "the model declined every chapter of this book".into(),
            });
        }
        let (model2, progress2, cast2, summaries2) = (
            Arc::clone(&model),
            progress.clone(),
            cast.clone(),
            summaries.clone(),
        );
        let source2 = source("");
        let work = self.rt.lifetime(cancellation.as_deref());
        let (overview, used) = self
            .rt
            .run("book-overview", work, move |cancel| {
                let summariser = summary::Summariser {
                    model: model2.as_ref(),
                    cancel,
                    progress: progress2,
                    requests: Default::default(),
                    source: source2,
                };
                let text = summariser.overview(&cast2, &summaries2)?;
                Ok::<_, LibraryError>((text, summariser.requests.get()))
            })
            .await?;
        requests += used;
        let stored = StoredBookSynopsis {
            epub_file,
            text: overview,
            cast,
            chapters_read: summaries.len() as u32,
            chapters_skipped: skipped,
            model_requests: requests,
            seconds: seconds(&started),
            model_name: info.name,
            created_at_ms: now_ms(),
        };
        self.rt
            .command("save-book-summary", move |state| {
                state.commit(|data, _, _| data.save_book_synopsis(id, stored))?;
                state
                    .data
                    .book_synopsis(id)
                    .ok_or_else(|| LibraryError::invalid("the summary was not saved"))
            })
            .await
    }

    // MARK: LibrarySettings and data

    pub async fn update_settings(
        &self,
        settings: LibrarySettings,
    ) -> Result<LibrarySettings, LibraryError> {
        self.rt
            .command("update-settings", move |state| {
                state.commit(|data, _, _| {
                    data.settings = settings.clone();
                    Ok(settings)
                })
            })
            .await
    }

    /// Writes a backup of the whole library (books, history, settings,
    /// covers and EPUBs) as one archive for the share sheet, and returns its
    /// path. `restore_library` reads it back.
    pub async fn backup_library(&self) -> Result<String, LibraryError> {
        // On the command thread, so no change lands while the files are
        // copied.
        self.rt
            .command("backup", move |state| {
                let path = state.store.write_backup(now_ms())?;
                Ok(path.to_string_lossy().into_owned())
            })
            .await
    }

    /// Replaces the library with the backup at `path` (from
    /// `backup_library`, on this phone or another). The archive is checked
    /// in full first: another app's file, a newer version's, or a damaged
    /// one is refused and the library is left as it was. Returns how many
    /// books the restored library has.
    pub async fn restore_library(&self, path: String) -> Result<u32, LibraryError> {
        let store = Arc::clone(&self.store);
        let work = self.rt.finishing(None);
        let (restoring, data) = self
            .rt
            .run("unpack-backup", work, move |_| {
                store.unpack_backup(std::path::Path::new(&path))
            })
            .await?;
        let open_book = Arc::clone(&self.open_book);
        self.rt
            .command("restore", move |state| {
                state.store.adopt_backup(&restoring, &data)?;
                *lock(&open_book) = None;
                let books = data.books.len() as u32;
                state.published.set(data.clone());
                state.data = data;
                Ok(books)
            })
            .await
    }

    /// Writes every book as a spreadsheet (CSV) for the share sheet, with
    /// the columns other reading apps and spreadsheets understand, and
    /// returns the file's path.
    pub async fn export_books_csv(&self) -> Result<String, LibraryError> {
        let (data, store) = (self.data.get(), Arc::clone(&self.store));
        let work = self.rt.finishing(None);
        self.rt
            .run("export-csv", work, move |_| {
                let rows: Vec<csv_rows::BookRow> =
                    data.books.iter().map(csv_rows::BookRow::from).collect();
                let bytes = forge_storage::csv_file::to_csv(&rows)?;
                let path = store.write_export("PocketLibrary.csv", &bytes)?;
                Ok(path.to_string_lossy().into_owned())
            })
            .await
    }

    /// Adds the books in a CSV file (one this app exported, or a
    /// spreadsheet with at least a Title column). Rows that can't be read
    /// are skipped and listed by line; the rest are added in one save.
    pub async fn import_books_csv(&self, path: String) -> Result<CsvImportSummary, LibraryError> {
        let work = self.rt.finishing(None);
        let read = self
            .rt
            .run("read-csv", work, move |_| {
                forge_storage::csv_file::import_csv::<csv_rows::BookRow>(std::path::Path::new(
                    &path,
                ))
                .map_err(LibraryError::from)
            })
            .await?;
        self.rt
            .command("import-csv", move |state| {
                let mut skipped: Vec<String> = read
                    .problems
                    .iter()
                    .map(|p| format!("Line {}: {}", p.line, p.reason))
                    .collect();
                let mut added = 0u32;
                let mut inputs = Vec::new();
                for (index, row) in read.rows.into_iter().enumerate() {
                    match row.into_input().and_then(validate) {
                        Ok(input) => inputs.push(input),
                        Err(e) => skipped.push(format!("Row {}: {e}", index + 1)),
                    }
                }
                if !inputs.is_empty() {
                    state.commit(|data, _, now| {
                        for input in inputs {
                            data.add(input, BookSource::Manual, None, now)?;
                            added += 1;
                        }
                        Ok(())
                    })?;
                }
                Ok(CsvImportSummary { added, skipped })
            })
            .await
    }

    /// Deletes every book, the history and saved covers. Settings stay.
    pub async fn erase_library(&self) -> Result<(), LibraryError> {
        let open_book = Arc::clone(&self.open_book);
        self.rt
            .command("erase", move |state| {
                state.commit(|data, _, _| {
                    *data = LibraryData {
                        settings: data.settings.clone(),
                        ..LibraryData::default()
                    };
                    Ok(())
                })?;
                *lock(&open_book) = None;
                state.store.clear_files();
                Ok(())
            })
            .await
    }

    pub fn storage_info(&self) -> StorageInfo {
        let data = self.data.get();
        let (saved_covers, saved_books, bytes) = self.store.usage();
        StorageInfo {
            directory: self.store.dir().to_string_lossy().into_owned(),
            books: data.books.len() as u32,
            activity_entries: data.activity.len() as u32,
            saved_covers,
            saved_books,
            bytes,
        }
    }
}

// The runtime half of the API: `events`, `set_phase`, `phase`, `revision`,
// background work, the launch report and `shutdown`.
forge_app::export_runtime!(Library, rt);

/// The framework kind of each error, so ForgeKit presents it like any
/// app's (`LibraryError: ForgeKindedError` in Swift).
#[uniffi::export]
pub fn library_error_kind(error: LibraryError) -> ErrorKind {
    error.kind()
}

impl Library {
    /// Opens with a given catalogue: tests pass one over canned responses.
    pub fn open(
        storage_directory: String,
        catalogue: Arc<dyn BookCatalogue>,
    ) -> Result<Arc<Self>, LibraryError> {
        Self::open_with(storage_directory, catalogue, false)
    }

    fn open_with(
        storage_directory: String,
        catalogue: Arc<dyn BookCatalogue>,
        fresh: bool,
    ) -> Result<Arc<Self>, LibraryError> {
        forge_runtime::logging::init();
        let directory = DataDirectory::open(non_empty(&storage_directory)?)?;
        let store = Store::new(directory.path());
        if fresh {
            if let Err(err @ LibraryError::NewerLibrary { .. }) = store.load() {
                return Err(err);
            }
            store.set_aside(now_ms())?;
        }
        let data = store.load()?;
        store.remove_orphan_covers(&data);
        let store = Arc::new(store);
        let published = Arc::new(Published::new(data.clone()));
        let state = State {
            data,
            store: Arc::clone(&store),
            published: Arc::clone(&published),
        };
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("pocket-library", directory, state),
            data: published,
            store,
            catalogue,
            last_search: Arc::new(Mutex::new(HashMap::new())),
            open_book: Arc::new(Mutex::new(None)),
        }))
    }

    /// Saves where a whole-book run got to, after each chapter.
    #[allow(clippy::too_many_arguments)]
    async fn save_book_run(
        &self,
        id: u64,
        epub_file: &str,
        sections_done: u32,
        sections: u32,
        cast: &[CastMember],
        chapters_read: u32,
        chapters_skipped: u32,
        model_requests: u32,
        seconds: u32,
    ) -> Result<(), LibraryError> {
        let run = StoredBookRun {
            epub_file: epub_file.to_string(),
            sections_done,
            sections,
            cast: cast.to_vec(),
            chapters_read,
            chapters_skipped,
            model_requests,
            seconds,
        };
        self.rt
            .command("save-book-run", move |state| {
                state.commit(|data, _, _| data.save_book_run(id, run))
            })
            .await
    }

    fn ensure_catalogue_enabled(&self) -> Result<(), LibraryError> {
        if self.data.get().settings.catalogue_enabled {
            Ok(())
        } else {
            Err(LibraryError::CatalogueDisabled)
        }
    }

    /// A command that changes the library and returns the book it names.
    async fn commit_book<F>(&self, name: &'static str, change: F) -> Result<Book, LibraryError>
    where
        F: FnOnce(&mut LibraryData, &Store, i64) -> Result<u64, LibraryError> + Send + 'static,
    {
        self.rt
            .command(name, move |state| {
                let id = state.commit(change)?;
                state.book(id)
            })
            .await
    }
}

/// Adds an imported book with its staged cover and EPUB, on the command
/// thread. The files are committed once the library naming them is saved;
/// on any failure they are dropped, which deletes them, so a failure leaves
/// nothing behind.
fn add_book_with_files(
    state: &mut State,
    input: domain::ValidInput,
    source: BookSource,
    cover_file: Option<Staged>,
    epub_file: Option<Staged>,
) -> Result<Book, LibraryError> {
    if let Some(existing) = state.data.book_with_source(&source) {
        return Err(LibraryError::AlreadyInLibrary { id: existing.id });
    }
    let name = |file: &Option<Staged>| file.as_ref().map(|f| f.name().to_string());
    let (cover, epub) = (name(&cover_file), name(&epub_file));
    let id = state.commit(|data, _, now| {
        let id = data.add(input, source, cover, now)?;
        data.attach_epub(id, epub)?;
        Ok(id)
    })?;
    for file in [cover_file, epub_file].into_iter().flatten() {
        file.commit();
    }
    state.book(id)
}

/// The library's default query for a fresh screen: everything, in the
/// user's preferred order.
#[uniffi::export]
pub fn default_query(settings: LibrarySettings) -> LibraryQuery {
    LibraryQuery {
        text: String::new(),
        status: None,
        sort: settings.default_sort,
    }
}

/// Words per page used for EPUB page estimates, for the import screen's
/// explanation.
#[uniffi::export]
pub fn epub_words_per_page() -> u32 {
    epub::WORDS_PER_PAGE
}

fn default_catalogue() -> Arc<dyn BookCatalogue> {
    Arc::new(OpenLibrary::new(
        UreqClient::with_user_agent(USER_AGENT).max_body_bytes(8 * 1024 * 1024),
    ))
}

fn non_empty(dir: &str) -> Result<&str, LibraryError> {
    if dir.trim().is_empty() {
        Err(LibraryError::invalid("storage directory must not be empty"))
    } else {
        Ok(dir)
    }
}

#[cfg(test)]
mod tests;
