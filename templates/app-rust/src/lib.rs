//! The app's Rust core. Everything the SwiftUI app can call is in this
//! file's `#[uniffi::export]` blocks; `forge build` turns them into Swift
//! (`import ForgeFFI`).
//!
//! The starter is a small notes app on the ixOS application runtime
//! (`forge_app`), showing every pattern a real app needs, so you can replace
//! it piece by piece:
//!
//! - **commands** (`add_note`, `update_note`, `delete_note`) run one at a
//!   time on the runtime's command thread, in the order they were sent. Each
//!   saves before it returns; only then does the runtime raise the revision
//!   and tell the host (`RuntimeEvent::Changed`), which re-reads;
//! - **queries** (`notes`, `search_notes`) read a [`Published`] snapshot and
//!   never wait for a command;
//! - **work** that is not a command (`check_url`, HTTPS with the platform's
//!   certificate checks) runs off the command thread, in the foreground
//!   scope: the app going to the background, the host's [`Cancellation`] or
//!   shutdown stop it;
//! - typed errors ([`AppError`]) that arrive in Swift as thrown errors with
//!   the same cases, each with a framework kind (`app_error_kind`);
//! - the lifecycle, events and shutdown come from the runtime
//!   (`forge_app::export_runtime!`); ForgeKit's `ForgeHost` drives them;
//! - native services (Keychain, notifications, deep links) and logging to
//!   the platform log ([`forge_runtime::logging`]).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use forge_app::{Cancellation, DataDirectory, ErrorKind, OpenError, Published, Runtime, Stopped};
use forge_net::{HttpClient, NetError, Request, UreqClient};
use forge_runtime::now_ms;
use forge_storage::{JsonFile, StorageError};
use serde::{Deserialize, Serialize};

uniffi::setup_scaffolding!("forge_ffi");

// Native services the host implements (Keychain, app directories, local
// notifications, permissions, deep links), as `crate::native`. The iOS host
// installs them with `ForgeServices.install()`; Rust code uses
// `native::NativeServices::installed()`. See docs/native-services.md.
forge_services::native_services!();

// `forge_log` for the host: Swift's `ForgeLog("Category").warn(...)` writes
// into the same log as Rust (system log, recent records, `forge device`).
forge_runtime::log_export!();

/// Schema version of `notes.json`. Bump it (and add a migration) when
/// [`StoredNotes`] changes shape.
const STORE_VERSION: u32 = 1;

/// One note.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: u64,
    pub text: String,
    /// Unix time in milliseconds.
    pub created_at_ms: i64,
}

/// Every error the API returns. `reason` fields are diagnostics, not UI copy.
/// Field names avoid `message`, which clashes with Kotlin's `Throwable.message`.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum AppError {
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error("no note with id {id}")]
    NotFound { id: u64 },
    /// The notes file could not be read or written.
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// Another copy of the app holds the notes (one core per data folder).
    #[error("the notes are open in another copy of the app")]
    InUse,
    /// No connection, DNS failure, or connection refused.
    #[error("offline: {reason}")]
    Offline { reason: String },
    /// The server's TLS certificate was rejected (expired, untrusted, wrong host).
    #[error("certificate rejected: {reason}")]
    Certificate { reason: String },
    #[error("request timed out")]
    Timeout,
    /// Any other network failure.
    #[error("network error: {reason}")]
    Network { reason: String },
    /// The request was cancelled with its [`Cancellation`].
    #[error("cancelled")]
    Cancelled,
    /// The app went to the background while the request was running, which
    /// stops it. Safe to retry once the app is back on screen.
    #[error("interrupted: the app went to the background")]
    Interrupted,
    /// `shutdown()` was called; the instance accepts no more work.
    #[error("the app has been shut down")]
    ShutDown,
}

/// How the runtime reports work it stopped or refused.
impl From<Stopped> for AppError {
    fn from(stopped: Stopped) -> Self {
        match stopped.kind {
            ErrorKind::Interrupted => Self::Interrupted,
            ErrorKind::ShutDown => Self::ShutDown,
            _ => Self::Cancelled,
        }
    }
}

impl From<StorageError> for AppError {
    fn from(err: StorageError) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

impl From<OpenError> for AppError {
    fn from(err: OpenError) -> Self {
        match err {
            OpenError::InUse { .. } => Self::InUse,
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

impl From<NetError> for AppError {
    fn from(err: NetError) -> Self {
        match err {
            NetError::InvalidRequest { reason } => Self::InvalidInput { reason },
            NetError::Offline { reason } => Self::Offline { reason },
            NetError::Certificate { reason } => Self::Certificate { reason },
            NetError::Timeout => Self::Timeout,
            NetError::Cancelled => Self::Cancelled,
            other => Self::Network {
                reason: other.to_string(),
            },
        }
    }
}

/// The framework kind of an error, so ForgeKit (`ForgeErrors.describe`)
/// can present any of them sensibly.
#[uniffi::export]
pub fn app_error_kind(error: AppError) -> ErrorKind {
    match error {
        AppError::InvalidInput { .. } => ErrorKind::Invalid,
        AppError::NotFound { .. } => ErrorKind::NotFound,
        AppError::Storage { .. } => ErrorKind::Internal,
        AppError::InUse
        | AppError::Offline { .. }
        | AppError::Certificate { .. }
        | AppError::Timeout
        | AppError::Network { .. } => ErrorKind::Unavailable,
        AppError::Cancelled => ErrorKind::Cancelled,
        AppError::Interrupted => ErrorKind::Interrupted,
        AppError::ShutDown => ErrorKind::ShutDown,
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct StoredNotes {
    next_id: u64,
    notes: Vec<Note>,
}

/// What the command thread owns: the notes and the file they are saved in.
struct State {
    stored: StoredNotes,
    file: JsonFile<StoredNotes>,
    published: Arc<Published<Vec<Note>>>,
}

impl State {
    /// Changes a copy, saves it, then keeps it and publishes the new list:
    /// a failed save leaves everything as it was, which the runtime requires
    /// of a failed command.
    fn change<T>(
        &mut self,
        edit: impl FnOnce(&mut StoredNotes) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut next = self.stored.clone();
        let value = edit(&mut next)?;
        self.file.save(&next)?;
        self.stored = next;
        self.published.set(newest_first(&self.stored.notes));
        Ok(value)
    }
}

/// The app instance. ForgeKit's `ForgeHost` opens one per process and
/// shares it. Thread-safe.
#[derive(uniffi::Object)]
pub struct AppCore {
    rt: Runtime<State>,
    notes: Arc<Published<Vec<Note>>>,
    http: UreqClient,
    /// The live connection to `forge device`, in development builds only.
    #[cfg(feature = "devtools")]
    _devtools: Option<forge_devtools::DevTools>,
}

#[uniffi::export]
impl AppCore {
    /// Opens the app, loading saved notes from `storage_directory` (created
    /// if missing) and holding it, so no second core can write there. Fails
    /// with `Storage` if the saved data cannot be used (it is left
    /// untouched), or `InUse` if another core holds it.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, AppError> {
        forge_runtime::logging::init();
        if storage_directory.is_empty() {
            return Err(AppError::InvalidInput {
                reason: "storage directory must not be empty".into(),
            });
        }
        let storage_directory = PathBuf::from(storage_directory);
        let directory = DataDirectory::open(&storage_directory)?;
        let file = JsonFile::new(directory.path().join("notes.json"), STORE_VERSION);
        let stored: StoredNotes = file.load()?.unwrap_or_default();
        // Logs go to the system log on iOS (see forge_runtime::logging).
        // Never log note text or other user data.
        forge_runtime::logging::info!("opened with {} notes", stored.notes.len());
        let notes = Arc::new(Published::new(newest_first(&stored.notes)));
        let state = State {
            stored,
            file,
            published: Arc::clone(&notes),
        };
        let rt = Runtime::with_directory("app-core", directory, state);
        Ok(Arc::new(Self {
            #[cfg(feature = "devtools")]
            _devtools: devtools::start(&storage_directory, &notes, &rt),
            rt,
            notes,
            http: UreqClient::new(),
        }))
    }

    /// All notes, newest first. Never waits for a command.
    pub fn notes(&self) -> Vec<Note> {
        self.notes.get().as_ref().clone()
    }

    /// Notes containing `query` (ignoring case and surrounding spaces),
    /// newest first; every note when `query` is blank. A quick in-memory
    /// read, so the host may call it on each keystroke.
    pub fn search_notes(&self, query: String) -> Vec<Note> {
        let query = query.trim().to_lowercase();
        self.notes
            .get()
            .iter()
            .filter(|note| query.is_empty() || note.text.to_lowercase().contains(&query))
            .cloned()
            .collect()
    }

    /// Adds a note and saves it.
    pub async fn add_note(&self, text: String) -> Result<Note, AppError> {
        let text = note_text(text)?;
        self.rt
            .command("add-note", move |state| {
                state.change(|stored| {
                    let note = Note {
                        id: stored.next_id + 1,
                        text,
                        created_at_ms: now_ms(),
                    };
                    stored.next_id = note.id;
                    stored.notes.push(note.clone());
                    Ok(note)
                })
            })
            .await
    }

    /// Replaces a note's text and saves. `NotFound` if there is no such
    /// note, `InvalidInput` if `text` is blank.
    pub async fn update_note(&self, id: u64, text: String) -> Result<Note, AppError> {
        let text = note_text(text)?;
        self.rt
            .command("update-note", move |state| {
                state.change(|stored| {
                    let note = stored
                        .notes
                        .iter_mut()
                        .find(|n| n.id == id)
                        .ok_or(AppError::NotFound { id })?;
                    note.text = text;
                    Ok(note.clone())
                })
            })
            .await
    }

    /// Deletes a note and saves. `NotFound` if there is no such note.
    pub async fn delete_note(&self, id: u64) -> Result<(), AppError> {
        self.rt
            .command("delete-note", move |state| {
                state.change(|stored| {
                    let before = stored.notes.len();
                    stored.notes.retain(|n| n.id != id);
                    if stored.notes.len() == before {
                        return Err(AppError::NotFound { id });
                    }
                    Ok(())
                })
            })
            .await
    }

    /// Fetches `url` (HTTPS or HTTP) and returns the HTTP status code. The
    /// certificate is checked with the platform's trust store; a bad one is
    /// `Certificate`, no connection is `Offline`. On-screen work:
    /// `cancellation`, the app going to the background, or `shutdown` stop
    /// it part way with `Cancelled`, `Interrupted` or `ShutDown`.
    pub async fn check_url(
        &self,
        url: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<u16, AppError> {
        let http = self.http.clone();
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("check-url", work, move |cancel| {
                let request = Request::get(url).timeout(Duration::from_secs(15));
                Ok(http.send_cancellable(&request, cancel)?.status)
            })
            .await
    }
}

// The runtime half of the API: `events()`, `set_phase(phase)`, `phase()`,
// `revision()` and `shutdown()`. ForgeKit's `ForgeHost` calls them.
forge_app::export_runtime!(AppCore, rt);

/// The validation rule for note text, shared by add and update.
fn note_text(text: String) -> Result<String, AppError> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(AppError::InvalidInput {
            reason: "a note needs some text".into(),
        });
    }
    Ok(text)
}

fn newest_first(notes: &[Note]) -> Vec<Note> {
    let mut notes = notes.to_vec();
    notes.sort_by_key(|note| std::cmp::Reverse(note.id));
    notes
}

/// What this app shows `forge device` in development builds: probes for
/// `forge device snapshot` and commands for `forge device cmd`. Add your own;
/// never expose secrets or user content (note text, tokens).
#[cfg(feature = "devtools")]
mod devtools {
    use super::*;
    use forge_devtools::{Config, DevTools};

    pub(crate) fn start(
        storage_directory: &std::path::Path,
        notes: &Arc<Published<Vec<Note>>>,
        rt: &Runtime<State>,
    ) -> Option<DevTools> {
        // `forge run ios` passes the bundle ID; the pairing lives with the
        // app's data, so deleting the app forgets it.
        let app = option_env!("FORGE_APP_ID").unwrap_or(env!("CARGO_PKG_NAME"));
        let config = Config::new(
            app,
            env!("CARGO_PKG_VERSION"),
            storage_directory.join(".forge-devtools-pairing"),
        );
        let tools = match DevTools::start(config, rt.lifecycle().clone()) {
            Ok(tools) => tools,
            Err(e) => {
                forge_runtime::logging::warn!("devtools did not start: {e}");
                return None;
            }
        };
        let notes = Arc::clone(notes);
        tools.probe("notes", move || notes.get().len().to_string());
        tools.command("log", "Writes its arguments to the app's log", |args| {
            forge_runtime::logging::info!(target: "devtools", "{}", args.join(" "));
            Ok(String::new())
        });
        Some(tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_app::{AppPhase, RuntimeEvent};
    use forge_runtime::block_on;

    fn open(dir: &std::path::Path) -> Arc<AppCore> {
        AppCore::new(dir.to_string_lossy().into_owned()).unwrap()
    }

    #[test]
    fn notes_persist_across_instances() {
        let dir = tempfile::tempdir().unwrap();
        let app = open(dir.path());
        let first = block_on(app.add_note("first".into())).unwrap();
        let second = block_on(app.add_note("  second  ".into())).unwrap();
        assert_eq!(second.text, "second");
        app.shutdown();
        drop(app);

        let reopened = open(dir.path());
        assert_eq!(reopened.notes(), vec![second.clone(), first.clone()]);
        block_on(reopened.delete_note(first.id)).unwrap();
        drop(reopened);
        assert_eq!(open(dir.path()).notes(), vec![second]);
    }

    #[test]
    fn one_core_per_data_directory() {
        let dir = tempfile::tempdir().unwrap();
        let _app = open(dir.path());
        assert_eq!(
            AppCore::new(dir.path().to_string_lossy().into_owned()).err(),
            Some(AppError::InUse)
        );
    }

    #[test]
    fn notes_can_be_edited_and_searched() {
        let dir = tempfile::tempdir().unwrap();
        let app = open(dir.path());
        let milk = block_on(app.add_note("Buy milk".into())).unwrap();
        let call = block_on(app.add_note("Call Sam".into())).unwrap();
        let events = app.events().unwrap();

        let edited = block_on(app.update_note(milk.id, "  Buy oat milk ".into())).unwrap();
        assert_eq!(edited.text, "Buy oat milk");
        assert_eq!(edited.created_at_ms, milk.created_at_ms);
        // Saved, then announced.
        assert_eq!(
            block_on(events.next()),
            Some(RuntimeEvent::Changed { revision: 3 })
        );
        assert_eq!(app.notes(), vec![call.clone(), edited.clone()]);

        assert_eq!(app.search_notes("MILK ".into()), vec![edited.clone()]);
        assert_eq!(app.search_notes("  ".into()).len(), 2);
        assert!(app.search_notes("eggs".into()).is_empty());

        assert!(matches!(
            block_on(app.update_note(milk.id, " ".into())),
            Err(AppError::InvalidInput { .. })
        ));
        assert_eq!(
            block_on(app.update_note(99, "x".into())),
            Err(AppError::NotFound { id: 99 })
        );
        // Failed commands change nothing and announce nothing.
        assert_eq!(app.revision(), 3);
    }

    #[test]
    fn errors_are_typed() {
        let dir = tempfile::tempdir().unwrap();
        let app = open(dir.path());
        assert!(matches!(
            block_on(app.add_note("   ".into())),
            Err(AppError::InvalidInput { .. })
        ));
        assert_eq!(
            block_on(app.delete_note(42)),
            Err(AppError::NotFound { id: 42 })
        );
        assert!(matches!(
            AppCore::new(String::new()),
            Err(AppError::InvalidInput { .. })
        ));
        assert_eq!(
            app_error_kind(AppError::Interrupted),
            ErrorKind::Interrupted
        );
        drop(app);
        // A file from a newer build is reported, not overwritten.
        std::fs::write(
            dir.path().join("notes.json"),
            r#"{"schema_version": 99, "data": {}}"#,
        )
        .unwrap();
        assert!(matches!(
            AppCore::new(dir.path().to_string_lossy().into_owned()),
            Err(AppError::Storage { .. })
        ));
    }

    #[test]
    fn shutdown_refuses_new_work_and_ends_event_streams() {
        let dir = tempfile::tempdir().unwrap();
        let app = open(dir.path());
        let events = app.events().unwrap();
        block_on(app.add_note("hello".into())).unwrap();
        assert_eq!(
            block_on(events.next()),
            Some(RuntimeEvent::Changed { revision: 1 })
        );
        app.shutdown();
        assert_eq!(
            block_on(events.next()),
            Some(RuntimeEvent::PhaseChanged {
                phase: AppPhase::Terminated,
                resumed: false
            })
        );
        assert_eq!(block_on(events.next()), None);
        assert_eq!(
            block_on(app.add_note("late".into())),
            Err(AppError::ShutDown)
        );
        assert!(app.events().is_none());
    }

    #[test]
    fn check_url_reports_typed_network_errors() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        let dir = tempfile::tempdir().unwrap();
        let app = open(dir.path());
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(matches!(
            block_on(app.check_url(format!("http://127.0.0.1:{port}/"), None)),
            Err(AppError::Offline { .. })
        ));
        assert!(matches!(
            block_on(app.check_url("not a url".into(), None)),
            Err(AppError::InvalidInput { .. })
        ));
    }

    /// A local server that accepts and never answers, so a request to it
    /// only ends when cancelled (its timeout is 15 s).
    fn silent_server() -> (String, std::net::TcpListener) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        (url, listener)
    }

    fn after(ms: u64, what: impl FnOnce() + Send + 'static) {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            what();
        });
    }

    #[test]
    fn a_request_in_flight_can_be_cancelled_or_stopped_by_shutdown() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        let dir = tempfile::tempdir().unwrap();
        let app = open(dir.path());
        let (url, _server) = silent_server();

        let cancellation = Cancellation::new();
        let started = std::time::Instant::now();
        after(150, {
            let cancellation = Arc::clone(&cancellation);
            move || cancellation.cancel()
        });
        let result = block_on(app.check_url(url.clone(), Some(cancellation)));
        assert_eq!(result, Err(AppError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(5));

        // The app still works after a cancelled request; shutdown stops the next.
        after(150, {
            let app = Arc::clone(&app);
            move || app.shutdown()
        });
        assert_eq!(block_on(app.check_url(url, None)), Err(AppError::ShutDown));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn the_background_interrupts_requests_but_not_saving() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        let dir = tempfile::tempdir().unwrap();
        let app = open(dir.path());
        let (url, _server) = silent_server();
        assert_eq!(app.phase(), AppPhase::Launching);
        app.set_phase(AppPhase::Active);

        let started = std::time::Instant::now();
        after(150, {
            let app = Arc::clone(&app);
            move || app.set_phase(AppPhase::Background)
        });
        assert_eq!(
            block_on(app.check_url(url.clone(), None)),
            Err(AppError::Interrupted)
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        // Started while in the background, a request stops at once.
        assert_eq!(
            block_on(app.check_url(url, None)),
            Err(AppError::Interrupted)
        );
        // Notes still save in the background: a command is not on-screen work.
        block_on(app.add_note("from the background".into())).unwrap();
    }

    #[test]
    fn terminated_is_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let app = open(dir.path());
        app.set_phase(AppPhase::Active);
        app.set_phase(AppPhase::Terminated);
        assert_eq!(app.phase(), AppPhase::Terminated);
        assert_eq!(
            block_on(app.add_note("late".into())),
            Err(AppError::ShutDown)
        );
        app.shutdown(); // idempotent
    }
}
