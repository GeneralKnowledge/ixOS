//! Pocket Library's Rust core: an offline-first reading tracker.
//!
//! Everything the SwiftUI host calls is in this file's `#[uniffi::export]`
//! blocks; `forge build` turns them into Swift (`import ForgeFFI`). The rules
//! are in [`domain`], the files in [`store`], the online catalogue in
//! [`catalogue`], EPUB import in [`epub`] and the reader's text in
//! [`reader`].
//!
//! Threading: every method that touches the disk or the network is `async`
//! and runs on its own thread ([`forge_runtime::background`]), so Swift can
//! await it from the main actor. The plain methods (`books`, `stats`, …) only
//! read memory and are cheap enough for the UI thread.
//!
//! Each change is saved before the call returns, so the app can be suspended
//! or killed at any moment without losing anything.

pub mod catalogue;
pub mod domain;
pub mod epub;
mod error;
mod model;
pub mod reader;
pub mod store;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use forge_net::UreqClient;
use forge_runtime::{background, background_cancellable, lock, now_ms, CancelHandle, EventHub};

pub use catalogue::{BookCatalogue, CatalogueHit, OpenLibrary};
use domain::{fold, validate, LibraryData};
pub use error::LibraryError;
use forge_storage::Staged;
pub use model::*;
use reader::EpubText;
use store::Store;

// No namespace argument: UniFFI takes the library's name (pocket_library_core), so this
// crate keeps its own bindings when another app links it.
uniffi::setup_scaffolding!();

// A per-request `Cancellation` and the event stream object
// (`LibraryEvents::subscribe(&hub)`). See `forge_runtime::app_shell!`.
forge_runtime::app_shell! {
    cancellation: Cancellation,
    events: LibraryEvents(LibraryEvent::EventsDropped),
}

/// Sent with every catalogue request. Open Library asks clients to identify
/// themselves.
pub const USER_AGENT: &str = concat!(
    "PocketLibrary/",
    env!("CARGO_PKG_VERSION"),
    " (Forge example app; https://github.com/GeneralKnowledge/ixOS)"
);

struct State {
    data: LibraryData,
    revision: u64,
    /// The latest catalogue results, so an import needs only the key.
    last_search: HashMap<String, CatalogueHit>,
}

/// The library the host opens once and shares. Thread-safe.
#[derive(uniffi::Object)]
pub struct Library {
    state: Arc<Mutex<State>>,
    store: Arc<Store>,
    events: EventHub<LibraryEvent>,
    catalogue: Arc<dyn BookCatalogue>,
    shut_down: Arc<AtomicBool>,
    /// Cancelled by `shutdown`; every catalogue request runs under a child.
    lifetime: CancelHandle,
    /// The book open in the reader, so turning a page reads one chapter
    /// instead of the whole EPUB again.
    open_book: Arc<Mutex<Option<OpenBook>>>,
}

struct OpenBook {
    id: u64,
    epub_file: String,
    text: EpubText,
}

#[uniffi::export]
impl Library {
    /// Opens the library in `storage_directory` (created if missing). Fails
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
        let store = Store::new(non_empty(&storage_directory)?);
        if let Err(err @ LibraryError::NewerLibrary { .. }) = store.load() {
            return Err(err);
        }
        store.set_aside(now_ms())?;
        Self::open(storage_directory, default_catalogue())
    }

    // MARK: Reading (memory only)

    /// The books matching `query`, in its order.
    pub fn books(&self, query: LibraryQuery) -> Vec<Book> {
        let state = lock(&self.state);
        let covers = self.store.covers_dir();
        state
            .data
            .query(&query)
            .into_iter()
            .map(|b| b.to_book(covers))
            .collect()
    }

    pub fn book(&self, id: u64) -> Result<Book, LibraryError> {
        let state = lock(&self.state);
        Ok(state.data.book(id)?.to_book(self.store.covers_dir()))
    }

    /// Counts and reading totals. `year_start_ms` is the start of the
    /// current year in the user's time zone.
    pub fn stats(&self, year_start_ms: i64) -> LibraryStats {
        lock(&self.state).data.stats(year_start_ms, now_ms())
    }

    /// Reading history, newest first; `book_id` limits it to one book.
    pub fn history(&self, book_id: Option<u64>, limit: u32) -> Vec<ActivityEntry> {
        lock(&self.state).data.history(book_id, limit as usize)
    }

    pub fn settings(&self) -> LibrarySettings {
        lock(&self.state).data.settings.clone()
    }

    /// Increases with every saved change.
    pub fn revision(&self) -> u64 {
        lock(&self.state).revision
    }

    // MARK: Changing the library

    /// Adds a book typed in by the user.
    pub async fn add_book(&self, input: BookInput) -> Result<Book, LibraryError> {
        let input = validate(input)?;
        self.commit("add-book", move |data, _, now| {
            data.add(input, BookSource::Manual, None, now)
        })
        .await
    }

    /// Saves the edit form. Status and page follow the same rules as
    /// `update_progress` (the last page finishes a book, and so on).
    pub async fn update_book(&self, id: u64, input: BookInput) -> Result<Book, LibraryError> {
        let input = validate(input)?;
        self.commit("update-book", move |data, _, now| {
            data.update(id, input, now)?;
            Ok(id)
        })
        .await
    }

    /// Records the page the reader is on.
    pub async fn update_progress(&self, id: u64, page: u32) -> Result<Book, LibraryError> {
        self.commit("update-progress", move |data, _, now| {
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
        self.commit("set-status", move |data, _, now| {
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
        self.ensure_running()?;
        let (state, store, events) = self.parts();
        let open_book = Arc::clone(&self.open_book);
        background("delete-book", move || {
            let mut state = lock(&state);
            let mut next = state.data.clone();
            let removed = next.delete(id)?;
            store.save(&next)?;
            if let Some(cover) = &removed.cover_file {
                store.delete_cover(cover);
            }
            if let Some(epub) = &removed.epub_file {
                let mut open = lock(&open_book);
                if open.as_ref().is_some_and(|o| o.id == id) {
                    *open = None;
                }
                store.delete_book_file(epub);
            }
            apply(&mut state, next, &events);
            Ok(())
        })
        .await
    }

    // MARK: Online catalogue

    /// Searches the online catalogue. Nothing in the library changes; on any
    /// failure the library is exactly as it was.
    /// `cancellation` stops it part way with `Cancelled`.
    pub async fn search_catalogue(
        &self,
        query: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Vec<CatalogueBook>, LibraryError> {
        self.ensure_running()?;
        self.ensure_catalogue_enabled()?;
        let query = query.trim().to_string();
        if query.chars().count() < 2 {
            return Err(LibraryError::invalid(
                "type at least two characters to search",
            ));
        }
        let catalogue = Arc::clone(&self.catalogue);
        let state = Arc::clone(&self.state);
        self.cancellable("search-catalogue", cancellation, move |cancel| {
            let hits = catalogue.search(&query, cancel)?;
            let mut state = lock(&state);
            state.last_search = hits.iter().map(|h| (h.key.clone(), h.clone())).collect();
            Ok(hits
                .into_iter()
                .map(|hit| CatalogueBook {
                    library_book_id: state
                        .data
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
    /// still added, without them, and `warnings` says so. `cancellation`
    /// stops it part way with `Cancelled`, and then nothing is added.
    pub async fn import_from_catalogue(
        &self,
        key: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<ImportedBook, LibraryError> {
        self.ensure_running()?;
        self.ensure_catalogue_enabled()?;
        let source = BookSource::OpenLibrary {
            work_key: key.clone(),
        };
        let hit = {
            let state = lock(&self.state);
            if let Some(existing) = state.data.book_with_source(&source) {
                return Err(LibraryError::AlreadyInLibrary { id: existing.id });
            }
            state.last_search.get(&key).cloned().ok_or_else(|| {
                LibraryError::invalid("that result is no longer available; search again")
            })?
        };
        let catalogue = Arc::clone(&self.catalogue);
        let (state, store, events) = self.parts();
        self.cancellable("import-catalogue", cancellation, move |cancel| {
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
            let book = add_and_save(&state, &store, &events, input, source, cover_file, None)?;
            Ok(ImportedBook { book, warnings })
        })
        .await
    }

    // MARK: EPUB import

    /// Adds the book in the EPUB at `path` as Want to Read: title, authors,
    /// description and cover from the file, and a page count estimated from
    /// its length. A copy of the file is kept so the book can be read in the
    /// app (`open_reader`); the host may delete its own copy afterwards.
    ///
    /// If the book is already in the library without a saved EPUB (typed
    /// in, found in the catalogue, or imported before EPUBs were kept), the
    /// file is attached to that book instead, keeping its progress, and that
    /// book is returned. Otherwise a second import fails with
    /// `AlreadyInLibrary`.
    pub async fn import_epub(&self, path: String) -> Result<Book, LibraryError> {
        self.ensure_running()?;
        let (state, store, events) = self.parts();
        background("import-epub", move || {
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
                let state = lock(&state);
                let (title, author) = (fold(&meta.title), fold(&meta.author));
                state
                    .data
                    .books
                    .iter()
                    .find(|b| fold(&b.title) == title && fold(&b.author) == author)
                    .map(|b| (b.id, b.epub_file.is_some()))
            };
            let stem = format!("epub-{}", now_ms());
            match existing {
                Some((id, true)) => Err(LibraryError::AlreadyInLibrary { id }),
                Some((id, false)) => {
                    let epub_file = store.stage_book_file(&stem, &bytes)?;
                    attach_and_save(&state, &store, &events, id, epub_file)
                }
                None => {
                    // Staged files are deleted again if anything below fails.
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
                    add_and_save(
                        &state,
                        &store,
                        &events,
                        input,
                        BookSource::Epub { file_name },
                        cover_file,
                        Some(epub_file),
                    )
                }
            }
        })
        .await
    }

    // MARK: Reading

    /// Opens a book's saved EPUB for reading: its chapters, and where to
    /// start (see `ReaderContents::start`). Fails with `NotReadable` for a
    /// book without one. Images of earlier chapters are deleted.
    pub async fn open_reader(&self, id: u64) -> Result<ReaderContents, LibraryError> {
        self.ensure_running()?;
        let (state, store, _) = self.parts();
        let open_book = Arc::clone(&self.open_book);
        background("open-reader", move || {
            let (title, epub_file) = {
                let state = lock(&state);
                let book = state.data.book(id)?;
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
            // Lock order is state, then open_book (as in delete_book).
            drop(open);
            let start = lock(&state).data.reader_start(id, &words)?;
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
        self.ensure_running()?;
        let store = Arc::clone(&self.store);
        let open_book = Arc::clone(&self.open_book);
        background("reader-chapter", move || {
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
        self.commit("save-reading-position", move |data, _, now| {
            data.save_reading_position(id, position, &words, now)?;
            Ok(id)
        })
        .await
    }

    // MARK: LibrarySettings and data

    pub async fn update_settings(
        &self,
        settings: LibrarySettings,
    ) -> Result<LibrarySettings, LibraryError> {
        self.ensure_running()?;
        let (state, store, events) = self.parts();
        background("update-settings", move || {
            let mut state = lock(&state);
            let mut next = state.data.clone();
            next.settings = settings.clone();
            store.save(&next)?;
            state.data = next;
            state.revision += 1;
            events.publish(LibraryEvent::SettingsChanged {
                settings: settings.clone(),
            });
            Ok(settings)
        })
        .await
    }

    /// Writes the whole library (books, history, settings) as JSON for the
    /// share sheet and returns the file's path.
    pub async fn export_library(&self) -> Result<String, LibraryError> {
        self.ensure_running()?;
        let (state, store, _) = self.parts();
        background("export", move || {
            let data = lock(&state).data.clone();
            let json = serde_json::to_vec_pretty(&serde_json::json!({
                "format": "pocket-library-export",
                "schema_version": store::STORE_VERSION,
                "exported_at_ms": now_ms(),
                "library": data,
            }))
            .map_err(|e| LibraryError::Storage {
                reason: e.to_string(),
            })?;
            let path = store.write_export("PocketLibrary.json", &json)?;
            Ok(path.to_string_lossy().into_owned())
        })
        .await
    }

    /// Deletes every book, the history and saved covers. Settings stay.
    pub async fn erase_library(&self) -> Result<(), LibraryError> {
        self.ensure_running()?;
        let (state, store, events) = self.parts();
        let open_book = Arc::clone(&self.open_book);
        background("erase", move || {
            let mut state = lock(&state);
            let next = LibraryData {
                settings: state.data.settings.clone(),
                ..LibraryData::default()
            };
            store.save(&next)?;
            *lock(&open_book) = None;
            store.clear_files();
            apply(&mut state, next, &events);
            Ok(())
        })
        .await
    }

    pub fn storage_info(&self) -> StorageInfo {
        let state = lock(&self.state);
        let (saved_covers, saved_books, bytes) = self.store.usage();
        StorageInfo {
            directory: self.store.dir().to_string_lossy().into_owned(),
            books: state.data.books.len() as u32,
            activity_entries: state.data.activity.len() as u32,
            saved_covers,
            saved_books,
            bytes,
        }
    }

    // MARK: Events and lifecycle

    /// A new event stream. Each consumer takes its own.
    pub fn events(&self) -> Result<Arc<LibraryEvents>, LibraryError> {
        LibraryEvents::subscribe(&self.events).ok_or(LibraryError::ShutDown)
    }

    /// Refuses new commands with `ShutDown`, stops catalogue requests in
    /// flight (they fail with `ShutDown`) and ends every event stream. Saves
    /// already running finish, so no write is cut short. Idempotent.
    pub fn shutdown(&self) {
        self.shut_down.store(true, Ordering::SeqCst);
        self.lifetime.cancel();
        self.events.close();
    }
}

impl Library {
    /// Opens with a given catalogue: tests pass one over canned responses.
    pub fn open(
        storage_directory: String,
        catalogue: Arc<dyn BookCatalogue>,
    ) -> Result<Arc<Self>, LibraryError> {
        forge_runtime::logging::init();
        let store = Store::new(non_empty(&storage_directory)?);
        let data = store.load()?;
        store.remove_orphan_covers(&data);
        Ok(Arc::new(Self {
            state: Arc::new(Mutex::new(State {
                data,
                revision: 0,
                last_search: HashMap::new(),
            })),
            store: Arc::new(store),
            events: EventHub::new(),
            catalogue,
            shut_down: Arc::new(AtomicBool::new(false)),
            lifetime: CancelHandle::new(),
            open_book: Arc::new(Mutex::new(None)),
        }))
    }

    /// Runs catalogue work on its own thread under a handle that is
    /// cancelled by `cancellation`, by `shutdown`, or by dropping the future.
    /// A request stopped by `shutdown` fails with `ShutDown`.
    async fn cancellable<T, F>(
        &self,
        name: &str,
        cancellation: Option<Arc<Cancellation>>,
        work: F,
    ) -> Result<T, LibraryError>
    where
        T: Send + 'static,
        F: FnOnce(&CancelHandle) -> Result<T, LibraryError> + Send + 'static,
    {
        let lifetime = self.lifetime.clone();
        let (cancel, under_lifetime) = self.lifetime.child();
        let from_host = cancellation.map(|c| {
            let cancel = cancel.clone();
            c.handle.on_cancel(move || cancel.cancel())
        });
        background_cancellable(name, cancel, move |cancel| {
            let _links = (under_lifetime, from_host); // released when the work ends
            match work(cancel) {
                Err(LibraryError::Cancelled) if lifetime.is_cancelled() => {
                    Err(LibraryError::ShutDown)
                }
                result => result,
            }
        })
        .await
    }

    fn ensure_running(&self) -> Result<(), LibraryError> {
        if self.shut_down.load(Ordering::SeqCst) {
            Err(LibraryError::ShutDown)
        } else {
            Ok(())
        }
    }

    fn ensure_catalogue_enabled(&self) -> Result<(), LibraryError> {
        if lock(&self.state).data.settings.catalogue_enabled {
            Ok(())
        } else {
            Err(LibraryError::CatalogueDisabled)
        }
    }

    fn parts(&self) -> (Arc<Mutex<State>>, Arc<Store>, EventHub<LibraryEvent>) {
        (
            Arc::clone(&self.state),
            Arc::clone(&self.store),
            self.events.clone(),
        )
    }

    /// Applies `change` to a copy of the library, saves it, and only then
    /// makes it current and announces it. A failed save changes nothing.
    async fn commit<F>(&self, name: &'static str, change: F) -> Result<Book, LibraryError>
    where
        F: FnOnce(&mut LibraryData, &Store, i64) -> Result<u64, LibraryError> + Send + 'static,
    {
        self.ensure_running()?;
        let (state, store, events) = self.parts();
        background(name, move || {
            let mut state = lock(&state);
            let mut next = state.data.clone();
            let id = change(&mut next, &store, now_ms())?;
            store.save(&next)?;
            apply(&mut state, next, &events);
            Ok(state.data.book(id)?.to_book(store.covers_dir()))
        })
        .await
    }
}

fn apply(state: &mut State, next: LibraryData, events: &EventHub<LibraryEvent>) {
    state.data = next;
    state.revision += 1;
    events.publish(LibraryEvent::Changed {
        revision: state.revision,
    });
}

/// Adds an imported book with its staged cover and EPUB. The files are
/// committed once the library naming them is saved; on any failure they are
/// dropped, which deletes them, so a failure leaves nothing behind.
fn add_and_save(
    state: &Mutex<State>,
    store: &Store,
    events: &EventHub<LibraryEvent>,
    input: domain::ValidInput,
    source: BookSource,
    cover_file: Option<Staged>,
    epub_file: Option<Staged>,
) -> Result<Book, LibraryError> {
    let mut state = lock(state);
    if let Some(existing) = state.data.book_with_source(&source) {
        return Err(LibraryError::AlreadyInLibrary { id: existing.id });
    }
    let mut next = state.data.clone();
    let name = |file: &Option<Staged>| file.as_ref().map(|f| f.name().to_string());
    let id = next.add(input, source, name(&cover_file), now_ms())?;
    next.attach_epub(id, name(&epub_file))?;
    store.save(&next)?;
    for file in [cover_file, epub_file].into_iter().flatten() {
        file.commit();
    }
    apply(&mut state, next, events);
    Ok(state.data.book(id)?.to_book(store.covers_dir()))
}

/// Gives an existing book a staged EPUB, committed once the library is
/// saved (and deleted if it cannot be).
fn attach_and_save(
    state: &Mutex<State>,
    store: &Store,
    events: &EventHub<LibraryEvent>,
    id: u64,
    epub_file: Staged,
) -> Result<Book, LibraryError> {
    let mut state = lock(state);
    let mut next = state.data.clone();
    next.attach_epub(id, Some(epub_file.name().to_string()))?;
    store.save(&next)?;
    epub_file.commit();
    apply(&mut state, next, events);
    Ok(state.data.book(id)?.to_book(store.covers_dir()))
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
