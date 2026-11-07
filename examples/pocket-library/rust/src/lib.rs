//! Pocket Library's Rust core: an offline-first reading tracker.
//!
//! Everything the SwiftUI host calls is in this file's `#[uniffi::export]`
//! blocks; `forge build` turns them into Swift (`import ForgeFFI`). The rules
//! are in [`domain`], the files in [`store`], the online catalogue in
//! [`catalogue`] and EPUB reading in [`epub`].
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
pub mod store;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use forge_net::UreqClient;
use forge_runtime::{background, Delivery, EventHub, Subscription};

pub use catalogue::{BookCatalogue, CatalogueHit, OpenLibrary};
use domain::{fold, validate, LibraryData};
pub use error::LibraryError;
pub use model::*;
use store::Store;

uniffi::setup_scaffolding!("forge_ffi");

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
            .map(|b| b.to_book(&covers))
            .collect()
    }

    pub fn book(&self, id: u64) -> Result<Book, LibraryError> {
        let state = lock(&self.state);
        Ok(state.data.book(id)?.to_book(&self.store.covers_dir()))
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

    pub fn settings(&self) -> Settings {
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

    /// Deletes a book and its saved cover. Its history stays.
    pub async fn delete_book(&self, id: u64) -> Result<(), LibraryError> {
        self.ensure_running()?;
        let (state, store, events) = self.parts();
        background("delete-book", move || {
            let mut state = lock(&state);
            let mut next = state.data.clone();
            let removed = next.delete(id)?;
            store.save(&next)?;
            if let Some(cover) = &removed.cover_file {
                store.delete_cover(cover);
            }
            apply(&mut state, next, &events);
            Ok(())
        })
        .await
    }

    // MARK: Online catalogue

    /// Searches the online catalogue. Nothing in the library changes; on any
    /// failure the library is exactly as it was.
    pub async fn search_catalogue(
        &self,
        query: String,
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
        background("search-catalogue", move || {
            let hits = catalogue.search(&query)?;
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
    /// still added, without them, and `warnings` says so.
    pub async fn import_from_catalogue(&self, key: String) -> Result<ImportedBook, LibraryError> {
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
        background("import-catalogue", move || {
            let mut warnings = Vec::new();
            let description = match catalogue.description(&hit.key) {
                Ok(text) => text.unwrap_or_default(),
                Err(err) => {
                    warnings.push(format!("The description could not be loaded ({err})."));
                    String::new()
                }
            };
            let cover_file = match &hit.large_cover_url {
                None => None,
                Some(url) => match catalogue.cover(url).and_then(|(bytes, ext)| {
                    store.save_cover(&format!("ol-{}", hit.key), ext, &bytes)
                }) {
                    Ok(name) => Some(name),
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
            let book = add_and_save(&state, &store, &events, input, source, cover_file)?;
            Ok(ImportedBook { book, warnings })
        })
        .await
    }

    // MARK: EPUB import

    /// Adds the book in the EPUB at `path` as Want to Read: title, authors,
    /// description and cover from the file, and a page count estimated from
    /// its length. Only the metadata and cover are kept, never the text. The
    /// host may delete the file afterwards.
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
            let meta = epub::read_metadata(bytes)?;
            {
                let state = lock(&state);
                let (title, author) = (fold(&meta.title), fold(&meta.author));
                if let Some(existing) = state
                    .data
                    .books
                    .iter()
                    .find(|b| fold(&b.title) == title && fold(&b.author) == author)
                {
                    return Err(LibraryError::AlreadyInLibrary { id: existing.id });
                }
            }
            let cover_file = match &meta.cover {
                Some(cover) => Some(store.save_cover(
                    &format!("epub-{}", now_ms()),
                    cover.extension,
                    &cover.bytes,
                )?),
                None => None,
            };
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
            )
        })
        .await
    }

    // MARK: Settings and data

    pub async fn update_settings(&self, settings: Settings) -> Result<Settings, LibraryError> {
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
        background("erase", move || {
            let mut state = lock(&state);
            let next = LibraryData {
                settings: state.data.settings.clone(),
                ..LibraryData::default()
            };
            store.save(&next)?;
            store.clear_files();
            apply(&mut state, next, &events);
            Ok(())
        })
        .await
    }

    pub fn storage_info(&self) -> StorageInfo {
        let state = lock(&self.state);
        let (saved_covers, bytes) = self.store.usage();
        StorageInfo {
            directory: self.store.dir().to_string_lossy().into_owned(),
            books: state.data.books.len() as u32,
            activity_entries: state.data.activity.len() as u32,
            saved_covers,
            bytes,
        }
    }

    // MARK: Events and lifecycle

    /// A new event stream. Each consumer takes its own.
    pub fn events(&self) -> Result<Arc<LibraryEvents>, LibraryError> {
        self.events
            .subscribe()
            .map(|subscription| Arc::new(LibraryEvents { subscription }))
            .ok_or(LibraryError::ShutDown)
    }

    /// Refuses new commands with `ShutDown` and ends every event stream.
    /// Commands already running finish and save. Idempotent.
    pub fn shutdown(&self) {
        self.shut_down.store(true, Ordering::SeqCst);
        self.events.close();
    }
}

impl Library {
    /// Opens with a given catalogue: tests pass one over canned responses.
    pub fn open(
        storage_directory: String,
        catalogue: Arc<dyn BookCatalogue>,
    ) -> Result<Arc<Self>, LibraryError> {
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
        }))
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
            Ok(state.data.book(id)?.to_book(&store.covers_dir()))
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

/// Adds an imported book, deleting its saved cover again if the library
/// cannot be saved, so a failure leaves nothing behind.
fn add_and_save(
    state: &Mutex<State>,
    store: &Store,
    events: &EventHub<LibraryEvent>,
    input: domain::ValidInput,
    source: BookSource,
    cover_file: Option<String>,
) -> Result<Book, LibraryError> {
    let mut state = lock(state);
    let result = (|| {
        if let Some(existing) = state.data.book_with_source(&source) {
            return Err(LibraryError::AlreadyInLibrary { id: existing.id });
        }
        let mut next = state.data.clone();
        let id = next.add(input, source, cover_file.clone(), now_ms())?;
        store.save(&next)?;
        Ok((id, next))
    })();
    match result {
        Ok((id, next)) => {
            apply(&mut state, next, events);
            Ok(state.data.book(id)?.to_book(&store.covers_dir()))
        }
        Err(err) => {
            if let Some(cover) = &cover_file {
                store.delete_cover(cover);
            }
            Err(err)
        }
    }
}

/// One consumer's events. Await `next()` in a loop; `None` means it ended.
#[derive(uniffi::Object)]
pub struct LibraryEvents {
    subscription: Subscription<LibraryEvent>,
}

#[uniffi::export]
impl LibraryEvents {
    /// The next event, or `None` once this stream has ended.
    pub async fn next(&self) -> Option<LibraryEvent> {
        match self.subscription.next().await? {
            Delivery::Event(event) => Some(event),
            Delivery::Dropped(count) => Some(LibraryEvent::EventsDropped { count }),
        }
    }

    /// Ends this stream; a pending `next()` returns `None`.
    pub fn cancel(&self) {
        self.subscription.cancel();
    }
}

/// The library's default query for a fresh screen: everything, in the
/// user's preferred order.
#[uniffi::export]
pub fn default_query(settings: Settings) -> LibraryQuery {
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
