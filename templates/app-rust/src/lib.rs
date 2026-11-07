//! The app's Rust core. Everything the hosts can call is in this file's
//! `#[uniffi::export]` blocks; `forge build` turns them into Swift
//! (`import ForgeFFI`) and Kotlin (`uniffi.forge_ffi`) APIs.
//!
//! The starter is a small notes app that shows every pattern a real app
//! needs, so you can replace it piece by piece:
//!
//! - state owned by Rust, persisted to a JSON file that survives restarts
//!   ([`forge_storage::JsonFile`]);
//! - `async` commands that do blocking work off the UI thread
//!   ([`forge_runtime::background`]);
//! - typed errors ([`AppError`]) that arrive in Swift and Kotlin as thrown
//!   errors with the same cases;
//! - events the host subscribes to ([`AppCore::events`]);
//! - HTTPS with the platform's certificate checks ([`AppCore::check_url`]),
//!   cancellable while in flight ([`Cancellation`]);
//! - the app lifecycle ([`AppCore::set_phase`]): the host reports the
//!   phase, Rust stops on-screen work in the background and announces resume
//!   ([`forge_runtime::Lifecycle`]);
//! - shutdown that refuses new work and stops requests in flight.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forge_net::{HttpClient, NetError, Request, UreqClient};
use forge_runtime::{
    background, background_cancellable, CancelHandle, Delivery, EventHub, Lifecycle, Phase,
    Subscription,
};
use forge_storage::{JsonFile, StorageError};
use serde::{Deserialize, Serialize};

uniffi::setup_scaffolding!("forge_ffi");

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

/// Something the host may want to react to.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum AppEvent {
    /// The notes changed (and were saved). Carries the full list, newest first.
    NotesChanged { notes: Vec<Note> },
    /// This subscriber fell behind and missed `count` events; re-read `notes()`.
    EventsDropped { count: u64 },
    /// The app changed phase (reported by the host with `set_phase`).
    /// `resumed` is true on the way back from `Background`: the moment to
    /// refresh anything that may have changed while the app was away.
    PhaseChanged { phase: AppPhase, resumed: bool },
}

/// Where the app is in its lifecycle. The host reports it with
/// [`AppCore::set_phase`]: iOS from SwiftUI's `scenePhase` (and
/// `willTerminate`), Android from the activity's `onStart`, `onResume`,
/// `onPause` and `onStop`.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppPhase {
    /// Created, not on screen yet. The phase before the first report.
    Launching,
    /// On screen and receiving input.
    Active,
    /// On screen without input: an alert, the app switcher, a dialog.
    Inactive,
    /// Not visible; the OS may suspend or kill the process from here.
    /// Requests started on screen are stopped with `Interrupted`.
    Background,
    /// Going away: same as `shutdown()`. iOS does not always report it.
    Terminated,
}

impl From<AppPhase> for Phase {
    fn from(phase: AppPhase) -> Self {
        match phase {
            AppPhase::Launching => Phase::Launching,
            AppPhase::Active => Phase::Active,
            AppPhase::Inactive => Phase::Inactive,
            AppPhase::Background => Phase::Background,
            AppPhase::Terminated => Phase::Terminated,
        }
    }
}

impl From<Phase> for AppPhase {
    fn from(phase: Phase) -> Self {
        match phase {
            Phase::Launching => AppPhase::Launching,
            Phase::Active => AppPhase::Active,
            Phase::Inactive => AppPhase::Inactive,
            Phase::Background => AppPhase::Background,
            Phase::Terminated => AppPhase::Terminated,
        }
    }
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
    /// The request was cancelled with its [`Cancellation`] (or, from
    /// Kotlin, by cancelling the calling coroutine).
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

impl From<StorageError> for AppError {
    fn from(err: StorageError) -> Self {
        Self::Storage {
            reason: err.to_string(),
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

#[derive(Serialize, Deserialize, Default)]
struct StoredNotes {
    next_id: u64,
    notes: Vec<Note>,
}

struct State {
    stored: StoredNotes,
}

/// The app instance the host creates once and shares. Thread-safe.
#[derive(uniffi::Object)]
pub struct AppCore {
    state: Arc<Mutex<State>>,
    file: Arc<JsonFile<StoredNotes>>,
    events: EventHub<AppEvent>,
    http: UreqClient,
    shut_down: Arc<AtomicBool>,
    /// The phase the host reported, and the cancellation scopes that follow
    /// it: requests run under its foreground scope, which the background and
    /// `shutdown` cancel.
    lifecycle: Lifecycle,
}

/// Lets the host stop one request part way. Swift: create one per call and
/// cancel it from `withTaskCancellationHandler`, because cancelling a Swift
/// `Task` does not reach Rust on its own. Kotlin: cancelling the coroutine
/// is enough; pass `null`.
#[derive(uniffi::Object, Default)]
pub struct Cancellation {
    handle: CancelHandle,
}

#[uniffi::export]
impl Cancellation {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Stops the request: it fails with `Cancelled` at once, even while
    /// waiting for a slow server. Idempotent.
    pub fn cancel(&self) {
        self.handle.cancel();
    }
}

#[uniffi::export]
impl AppCore {
    /// Opens the app, loading saved notes from `storage_directory` (created
    /// if missing). Fails with `Storage` if the saved data cannot be used;
    /// it is left untouched.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, AppError> {
        if storage_directory.is_empty() {
            return Err(AppError::InvalidInput {
                reason: "storage directory must not be empty".into(),
            });
        }
        let file = JsonFile::new(
            PathBuf::from(storage_directory).join("notes.json"),
            STORE_VERSION,
        );
        let stored = file.load()?.unwrap_or_default();
        Ok(Arc::new(Self {
            state: Arc::new(Mutex::new(State { stored })),
            file: Arc::new(file),
            events: EventHub::new(),
            http: UreqClient::new(),
            shut_down: Arc::new(AtomicBool::new(false)),
            lifecycle: Lifecycle::new(),
        }))
    }

    /// All notes, newest first.
    pub fn notes(&self) -> Vec<Note> {
        newest_first(&lock(&self.state).stored.notes)
    }

    /// Notes containing `query` (ignoring case and surrounding spaces),
    /// newest first; every note when `query` is blank. A quick in-memory
    /// read, so the host may call it on each keystroke.
    pub fn search_notes(&self, query: String) -> Vec<Note> {
        let query = query.trim().to_lowercase();
        let state = lock(&self.state);
        let matches: Vec<Note> = state
            .stored
            .notes
            .iter()
            .filter(|note| query.is_empty() || note.text.to_lowercase().contains(&query))
            .cloned()
            .collect();
        newest_first(&matches)
    }

    /// Adds a note and saves. Emits `NotesChanged` after the save.
    pub async fn add_note(&self, text: String) -> Result<Note, AppError> {
        self.ensure_running()?;
        let text = note_text(text)?;
        let (state, file, events) = self.parts();
        background("add-note", move || {
            let mut state = lock(&state);
            let note = Note {
                id: state.stored.next_id + 1,
                text,
                created_at_ms: now_ms(),
            };
            let mut next = StoredNotes {
                next_id: note.id,
                notes: state.stored.notes.clone(),
            };
            next.notes.push(note.clone());
            file.save(&next)?; // nothing changes in memory if the save fails
            state.stored = next;
            events.publish(AppEvent::NotesChanged {
                notes: newest_first(&state.stored.notes),
            });
            Ok(note)
        })
        .await
    }

    /// Replaces a note's text and saves. `NotFound` if there is no such
    /// note, `InvalidInput` if `text` is blank. Emits `NotesChanged`.
    pub async fn update_note(&self, id: u64, text: String) -> Result<Note, AppError> {
        self.ensure_running()?;
        let text = note_text(text)?;
        let (state, file, events) = self.parts();
        background("update-note", move || {
            let mut state = lock(&state);
            let mut next = StoredNotes {
                next_id: state.stored.next_id,
                notes: state.stored.notes.clone(),
            };
            let note = next
                .notes
                .iter_mut()
                .find(|n| n.id == id)
                .ok_or(AppError::NotFound { id })?;
            note.text = text;
            let note = note.clone();
            file.save(&next)?;
            state.stored = next;
            events.publish(AppEvent::NotesChanged {
                notes: newest_first(&state.stored.notes),
            });
            Ok(note)
        })
        .await
    }

    /// Deletes a note and saves. `NotFound` if there is no such note.
    pub async fn delete_note(&self, id: u64) -> Result<(), AppError> {
        self.ensure_running()?;
        let (state, file, events) = self.parts();
        background("delete-note", move || {
            let mut state = lock(&state);
            if !state.stored.notes.iter().any(|n| n.id == id) {
                return Err(AppError::NotFound { id });
            }
            let next = StoredNotes {
                next_id: state.stored.next_id,
                notes: state
                    .stored
                    .notes
                    .iter()
                    .filter(|n| n.id != id)
                    .cloned()
                    .collect(),
            };
            file.save(&next)?;
            state.stored = next;
            events.publish(AppEvent::NotesChanged {
                notes: newest_first(&state.stored.notes),
            });
            Ok(())
        })
        .await
    }

    /// Fetches `url` (HTTPS or HTTP) and returns the HTTP status code. The
    /// certificate is checked with the platform's trust store; a bad one is
    /// `Certificate`, no connection is `Offline`. `cancellation`, the app
    /// going to the background, or `shutdown` stop it part way with
    /// `Cancelled`, `Interrupted` or `ShutDown`.
    pub async fn check_url(
        &self,
        url: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<u16, AppError> {
        self.ensure_running()?;
        let http = self.http.clone();
        let lifetime = self.lifecycle.lifetime();
        let foreground = self.lifecycle.foreground();
        // Cancelled by the background, shutdown, `cancellation`, or by
        // dropping the future. Started in the background, it stops at once.
        let (cancel, under_foreground) = foreground.child();
        let from_host = cancellation.map(|c| {
            let cancel = cancel.clone();
            c.handle.on_cancel(move || cancel.cancel())
        });
        background_cancellable("check-url", cancel, move |cancel| {
            let _links = (under_foreground, from_host); // released when the request ends
            let request = Request::get(url).timeout(Duration::from_secs(15));
            match http.send_cancellable(&request, cancel) {
                Ok(response) => Ok(response.status),
                Err(NetError::Cancelled) if lifetime.is_cancelled() => Err(AppError::ShutDown),
                Err(NetError::Cancelled) if foreground.is_cancelled() => Err(AppError::Interrupted),
                Err(err) => Err(err.into()),
            }
        })
        .await
    }

    /// A new event stream. Each consumer takes its own.
    pub fn events(&self) -> Result<Arc<AppEvents>, AppError> {
        self.events
            .subscribe()
            .map(|subscription| Arc::new(AppEvents { subscription }))
            .ok_or(AppError::ShutDown)
    }

    /// The phase last reported with `set_phase`.
    pub fn phase(&self) -> AppPhase {
        self.lifecycle.phase().into()
    }

    /// The host reports a lifecycle change. Cheap and safe to call from the
    /// UI thread, and to repeat. Going to `Background` stops requests
    /// started on screen (`Interrupted`); coming back emits `PhaseChanged`
    /// with `resumed: true`; `Terminated` is `shutdown()`. Notes need
    /// nothing here: every change is saved before its command returns.
    pub fn set_phase(&self, phase: AppPhase) {
        if phase == AppPhase::Terminated {
            return self.shutdown();
        }
        if let Some(change) = self.lifecycle.enter(phase.into()) {
            self.events.publish(AppEvent::PhaseChanged {
                phase,
                resumed: change.resumed,
            });
        }
    }

    /// Refuses new commands with `ShutDown`, stops requests in flight (they
    /// fail with `ShutDown`) and ends every event stream after a last
    /// `PhaseChanged(Terminated)`. Saves already running finish, so no write
    /// is cut short. Idempotent.
    pub fn shutdown(&self) {
        self.shut_down.store(true, Ordering::SeqCst);
        if self.lifecycle.enter(Phase::Terminated).is_some() {
            self.events.publish(AppEvent::PhaseChanged {
                phase: AppPhase::Terminated,
                resumed: false,
            });
        }
        self.events.close();
    }
}

impl AppCore {
    fn ensure_running(&self) -> Result<(), AppError> {
        if self.shut_down.load(Ordering::SeqCst) {
            Err(AppError::ShutDown)
        } else {
            Ok(())
        }
    }

    fn parts(
        &self,
    ) -> (
        Arc<Mutex<State>>,
        Arc<JsonFile<StoredNotes>>,
        EventHub<AppEvent>,
    ) {
        (
            Arc::clone(&self.state),
            Arc::clone(&self.file),
            self.events.clone(),
        )
    }
}

/// One consumer's events. Await `next()` in a loop; `None` means it ended.
#[derive(uniffi::Object)]
pub struct AppEvents {
    subscription: Subscription<AppEvent>,
}

#[uniffi::export]
impl AppEvents {
    /// The next event, or `None` once this stream has ended.
    pub async fn next(&self) -> Option<AppEvent> {
        match self.subscription.next().await? {
            Delivery::Event(event) => Some(event),
            Delivery::Dropped(count) => Some(AppEvent::EventsDropped { count }),
        }
    }

    /// Ends this stream; a pending `next()` returns `None`.
    pub fn cancel(&self) {
        self.subscription.cancel();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let reopened = open(dir.path());
        assert_eq!(reopened.notes(), vec![second.clone(), first.clone()]);
        block_on(reopened.delete_note(first.id)).unwrap();
        assert_eq!(open(dir.path()).notes(), vec![second]);
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
        assert_eq!(
            block_on(events.next()),
            Some(AppEvent::NotesChanged {
                notes: vec![call.clone(), edited.clone()]
            })
        );
        assert_eq!(open(dir.path()).notes()[1].text, "Buy oat milk");

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
    fn subscribers_see_changes_and_streams_end_on_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let app = open(dir.path());
        let events = app.events().unwrap();
        let note = block_on(app.add_note("hello".into())).unwrap();
        assert_eq!(
            block_on(events.next()),
            Some(AppEvent::NotesChanged { notes: vec![note] })
        );
        app.shutdown();
        assert_eq!(
            block_on(events.next()),
            Some(AppEvent::PhaseChanged {
                phase: AppPhase::Terminated,
                resumed: false
            })
        );
        assert_eq!(block_on(events.next()), None);
        assert_eq!(
            block_on(app.add_note("late".into())),
            Err(AppError::ShutDown)
        );
        assert!(app.events().is_err());
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

    fn cancel_after(ms: u64, what: impl FnOnce() + Send + 'static) {
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
        cancel_after(150, {
            let cancellation = Arc::clone(&cancellation);
            move || cancellation.cancel()
        });
        let result = block_on(app.check_url(url.clone(), Some(cancellation)));
        assert_eq!(result, Err(AppError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(5));

        // The app still works after a cancelled request; shutdown stops the next.
        cancel_after(150, {
            let app = Arc::clone(&app);
            move || app.shutdown()
        });
        assert_eq!(block_on(app.check_url(url, None)), Err(AppError::ShutDown));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn the_background_interrupts_requests_and_resume_is_announced() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        let dir = tempfile::tempdir().unwrap();
        let app = open(dir.path());
        let events = app.events().unwrap();
        let (url, _server) = silent_server();
        assert_eq!(app.phase(), AppPhase::Launching);
        app.set_phase(AppPhase::Active);
        app.set_phase(AppPhase::Active); // repeats are ignored

        let started = std::time::Instant::now();
        cancel_after(150, {
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
        // Notes still work in the background: saving is not on-screen work.
        block_on(app.add_note("from the background".into())).unwrap();

        app.set_phase(AppPhase::Inactive);
        app.set_phase(AppPhase::Active);
        let phases: Vec<_> = std::iter::from_fn(|| match block_on(events.next()) {
            Some(AppEvent::PhaseChanged { phase, resumed }) => Some(Some((phase, resumed))),
            Some(_) => Some(None),
            None => None,
        })
        .take(5)
        .flatten()
        .collect();
        assert_eq!(
            phases,
            vec![
                (AppPhase::Active, false),
                (AppPhase::Background, false),
                (AppPhase::Inactive, true),
                (AppPhase::Active, false),
            ]
        );
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
        app.set_phase(AppPhase::Active); // ignored once terminated
        assert_eq!(app.phase(), AppPhase::Terminated);
        app.shutdown(); // still idempotent
    }

    #[test]
    fn two_instances_in_different_directories_are_independent() {
        let (a_dir, b_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (a, b) = (open(a_dir.path()), open(b_dir.path()));
        block_on(a.add_note("in a".into())).unwrap();
        assert!(b.notes().is_empty());
        a.shutdown();
        block_on(b.add_note("in b".into())).unwrap();
        assert_eq!(open(a_dir.path()).notes().len(), 1);
    }
}
