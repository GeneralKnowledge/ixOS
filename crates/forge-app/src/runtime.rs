//! The runtime an app's core embeds.

use std::fs::{File, OpenOptions, TryLockError};
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use forge_runtime::{
    background_cancellable, net::AbortRegistration, CancelHandle, EventHub, Lifecycle, Phase,
};

use crate::contract::{
    AppPhase, Cancellation, ErrorKind, LaunchReport, RuntimeEvent, RuntimeEvents, Stopped,
};
use crate::executor::{reply, Executor};

const LOCK_FILE: &str = ".forge-app.lock";
/// Present while the app is in the background with work to finish. Found at
/// open, it means the last run was ended (by iOS or a crash) before that
/// work was done.
const UNFINISHED_FILE: &str = ".forge-app.unfinished";

/// An app's private data directory, held for the life of the core. Only one
/// core (in this process or another) can hold it, so two instances never
/// write over each other.
///
/// This is for the app's *private* container only. Never hold it on a
/// directory in an App Group container: iOS terminates a suspended process
/// that holds a file lock there (docs/app-model-review.md, Persistence).
#[derive(Debug)]
pub struct DataDirectory {
    path: PathBuf,
    _lock: File,
}

/// Why a data directory could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// Another core holds it.
    InUse { path: String },
    /// It could not be created or locked.
    Io { reason: String },
}

impl OpenError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            OpenError::InUse { .. } => ErrorKind::Unavailable,
            OpenError::Io { .. } => ErrorKind::Internal,
        }
    }
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::InUse { path } => write!(f, "{path} is already open in another instance"),
            OpenError::Io { reason } => write!(f, "could not open the data directory: {reason}"),
        }
    }
}

impl std::error::Error for OpenError {}

impl DataDirectory {
    /// Creates `path` if needed and takes its lock.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, OpenError> {
        let path = path.as_ref().to_path_buf();
        let io = |e: std::io::Error| OpenError::Io {
            reason: e.to_string(),
        };
        std::fs::create_dir_all(&path).map_err(io)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.join(LOCK_FILE))
            .map_err(io)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { path, _lock: file }),
            Err(TryLockError::WouldBlock) => Err(OpenError::InUse {
                path: path.display().to_string(),
            }),
            Err(TryLockError::Error(e)) => Err(io(e)),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The runtime an app's core object embeds. `S` is whatever the app keeps
/// (a struct, a store, a database handle); the runtime never looks inside.
pub struct Runtime<S> {
    executor: Executor<S>,
    revision: Arc<AtomicU64>,
    lifecycle: Lifecycle,
    events: EventHub<RuntimeEvent>,
    shut_down: AtomicBool,
    finishing: Arc<Finishing>,
    launch: LaunchReport,
    directory: Option<DataDirectory>,
}

/// A unit of non-command work and the scope that can stop it.
pub struct Work {
    cancel: CancelHandle,
    links: Vec<Box<dyn std::any::Any + Send>>,
    lifetime: CancelHandle,
    /// The scope whose cancellation means "interrupted": the foreground
    /// scope, or the finishing scope when background time ran out.
    interrupts: Option<CancelHandle>,
    _pending: Option<Pending>,
}

/// The work iOS is asked to wait for while the app is in the background:
/// accepted commands that haven't finished, and `finishing` work. It also
/// owns the finishing scope, which the host cancels when that time runs out.
struct Finishing {
    state: Mutex<FinishingState>,
    lifetime: CancelHandle,
    lifecycle: Lifecycle,
    events: EventHub<RuntimeEvent>,
    marker: Option<PathBuf>,
}

struct FinishingState {
    pending: usize,
    scope: CancelHandle,
    _link: AbortRegistration,
}

impl Finishing {
    fn new(
        lifecycle: &Lifecycle,
        events: &EventHub<RuntimeEvent>,
        marker: Option<PathBuf>,
    ) -> Self {
        let lifetime = lifecycle.lifetime();
        let (scope, link) = lifetime.child();
        Self {
            state: Mutex::new(FinishingState {
                pending: 0,
                scope,
                _link: link,
            }),
            lifetime,
            lifecycle: lifecycle.clone(),
            events: events.clone(),
            marker,
        }
    }

    fn lock(&self) -> MutexGuard<'_, FinishingState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn in_background(&self) -> bool {
        self.lifecycle.phase() == Phase::Background
    }

    /// Counts one piece of pending work until the returned guard drops.
    fn begin(self: &Arc<Self>) -> Pending {
        let mut state = self.lock();
        state.pending += 1;
        if state.pending == 1 && self.in_background() {
            self.announce(true);
        }
        Pending(Arc::clone(self))
    }

    fn end(&self) {
        let mut state = self.lock();
        state.pending -= 1;
        if state.pending == 0 && self.in_background() {
            self.announce(false);
        }
    }

    fn pending(&self) -> bool {
        self.lock().pending > 0
    }

    /// The app entered the background (`true`) or came back (`false`).
    fn phase_changed(&self, background: bool) {
        let state = self.lock();
        if background && state.pending > 0 {
            self.mark(true);
        } else if !background {
            self.mark(false);
        }
    }

    /// Called with the state locked, so announcements keep their order.
    fn announce(&self, pending: bool) {
        self.mark(pending);
        self.events
            .publish(RuntimeEvent::BackgroundWork { pending });
    }

    /// Creates or removes the marker. A create without a sync: it only has
    /// to survive the process being killed, not a power cut, and it runs on
    /// the host's lifecycle callback.
    fn mark(&self, pending: bool) {
        let Some(path) = &self.marker else { return };
        let result = if pending {
            std::fs::File::create(path).map(drop)
        } else {
            match std::fs::remove_file(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        };
        if let Err(err) = result {
            forge_runtime::logging::warn!("unfinished-work marker: {err}");
        }
    }

    fn scope(&self) -> CancelHandle {
        self.lock().scope.clone()
    }

    /// Background time ran out: stop finishing work, and start a new scope
    /// for work started afterwards.
    fn expire(&self) {
        let stopped = {
            let mut state = self.lock();
            let (scope, link) = self.lifetime.child();
            let old = std::mem::replace(&mut state.scope, scope);
            state._link = link;
            old
        };
        stopped.cancel();
    }
}

/// One piece of pending work, counted until dropped.
struct Pending(Arc<Finishing>);

impl Drop for Pending {
    fn drop(&mut self) {
        self.0.end();
    }
}

impl Work {
    /// The handle the work checks or passes to I/O (`send_cancellable`).
    pub fn handle(&self) -> &CancelHandle {
        &self.cancel
    }

    /// Why it stopped, once its handle is cancelled.
    pub fn stop_kind(&self) -> ErrorKind {
        if self.lifetime.is_cancelled() {
            ErrorKind::ShutDown
        } else if self.interrupts.as_ref().is_some_and(|f| f.is_cancelled()) {
            ErrorKind::Interrupted
        } else {
            ErrorKind::Cancelled
        }
    }
}

impl<S: Send + 'static> Runtime<S> {
    /// A runtime over `state`, with no data directory (tests, apps that
    /// store nothing).
    pub fn new(name: &str, state: S) -> Self {
        Self::build(name, state, None)
    }

    /// A runtime that holds `directory` until it is dropped.
    pub fn with_directory(name: &str, directory: DataDirectory, state: S) -> Self {
        Self::build(name, state, Some(directory))
    }

    fn build(name: &str, state: S, directory: Option<DataDirectory>) -> Self {
        let lifecycle = Lifecycle::new();
        let events = EventHub::new();
        let marker = directory.as_ref().map(|d| d.path().join(UNFINISHED_FILE));
        let launch = LaunchReport {
            unfinished_background_work: marker.as_ref().is_some_and(|m| m.exists()),
        };
        let finishing = Arc::new(Finishing::new(&lifecycle, &events, marker));
        // The report keeps what the marker said; the marker itself starts
        // over with this run.
        finishing.mark(false);
        Self {
            executor: Executor::start(name, state),
            revision: Arc::new(AtomicU64::new(0)),
            lifecycle,
            events,
            shut_down: AtomicBool::new(false),
            finishing,
            launch,
            directory,
        }
    }

    /// How the previous run ended, as far as the runtime can tell. Read it
    /// once at launch: `unfinished_background_work` means the app was ended
    /// while it still had work to finish in the background (iOS ran out of
    /// patience, or it crashed), so tell the person if that work mattered.
    /// Needs a data directory; without one the report is always clean.
    pub fn launch_report(&self) -> LaunchReport {
        self.launch.clone()
    }

    /// The data directory, if the runtime holds one.
    pub fn directory(&self) -> Option<&Path> {
        self.directory.as_ref().map(DataDirectory::path)
    }

    /// Runs `command` on the core's command thread, after every command
    /// accepted before it and before every one accepted after it. When it
    /// returns `Ok`, the revision increases and `Changed` is published, in
    /// that order and only then: whatever the command saved is on disk
    /// before anyone hears about it. An `Err` changes the revision not at
    /// all, so a command must leave the state as it found it when it fails
    /// (change a copy, save, then swap, as the starter does).
    ///
    /// Not cancellable once accepted. After `shutdown`, refused with
    /// `Stopped { kind: ShutDown }`.
    ///
    /// If iOS suspends the process mid-command, the command finishes on
    /// resume. If the process is killed first, the command did not happen
    /// (the caller never saw `Ok`), so the save inside it must be atomic
    /// (`JsonFile`, `write_atomic`, an SQLite transaction): the file then
    /// holds the previous state, never part of the new one. See
    /// docs/app-model-review.md, "What the runtime guarantees when iOS
    /// suspends or ends the process".
    pub fn command<T, E, F>(
        &self,
        name: &'static str,
        command: F,
    ) -> impl Future<Output = Result<T, E>>
    where
        T: Send + 'static,
        E: From<Stopped> + Send + 'static,
        F: FnOnce(&mut S) -> Result<T, E> + Send + 'static,
    {
        let refused = || {
            E::from(Stopped {
                kind: ErrorKind::ShutDown,
            })
        };
        let pending = if self.shut_down.load(Ordering::SeqCst) {
            None
        } else {
            let (tx, rx) = reply::<Result<T, E>>();
            let revision = Arc::clone(&self.revision);
            let events = self.events.clone();
            // Counted as pending until it has run, so a command accepted
            // just before the app leaves the screen gets time to save.
            let pending = self.finishing.begin();
            let job = Box::new(move |state: &mut S| {
                let result = catch_unwind(AssertUnwindSafe(|| command(state)));
                if matches!(result, Ok(Ok(_))) {
                    let revision = revision.fetch_add(1, Ordering::SeqCst) + 1;
                    events.publish(RuntimeEvent::Changed { revision });
                }
                if result.is_err() {
                    forge_runtime::logging::error!("command {name} panicked");
                }
                // No longer pending by the time the caller hears back.
                drop(pending);
                tx.send(result);
            });
            self.executor.submit(job).then_some(rx)
        };
        async move {
            match pending {
                Some(reply) => reply.await,
                None => Err(refused()),
            }
        }
    }

    /// Work that only makes sense on screen (a search, a refresh): stopped
    /// when the app goes to the background, by `host`, or by shutdown.
    pub fn foreground(&self, host: Option<&Cancellation>) -> Work {
        let foreground = self.lifecycle.foreground();
        self.work(&foreground, Some(foreground.clone()), host, None)
    }

    /// Work that should finish even if the app leaves the screen (saving
    /// an export, sending what was just written). While it runs in the
    /// background the host asks iOS for time (ForgeKit uses
    /// `beginBackgroundTask`); that time is best effort, and when it runs
    /// out the work is stopped with `Stopped { Interrupted }`. Also stopped
    /// by `host` and by shutdown. It must be safe to abandon at any point:
    /// whatever has to survive goes through a command.
    pub fn finishing(&self, host: Option<&Cancellation>) -> Work {
        let scope = self.finishing.scope();
        let pending = self.finishing.begin();
        self.work(&scope, Some(scope.clone()), host, Some(pending))
    }

    /// Work that may continue in the background until iOS suspends the
    /// process (it then freezes): stopped by `host` or by shutdown. Asks
    /// for no background time; use [`Self::finishing`] for work that should.
    pub fn lifetime(&self, host: Option<&Cancellation>) -> Work {
        let lifetime = self.lifecycle.lifetime();
        self.work(&lifetime, None, host, None)
    }

    fn work(
        &self,
        parent: &CancelHandle,
        interrupts: Option<CancelHandle>,
        host: Option<&Cancellation>,
        pending: Option<Pending>,
    ) -> Work {
        let (cancel, link) = parent.child();
        let mut links: Vec<Box<dyn std::any::Any + Send>> = vec![Box::new(link)];
        if let Some(host) = host {
            let cancel = cancel.clone();
            links.push(Box::new(host.handle().on_cancel(move || cancel.cancel())));
        }
        Work {
            cancel,
            links,
            lifetime: self.lifecycle.lifetime(),
            interrupts,
            _pending: pending,
        }
    }

    /// Whether accepted commands or finishing work are still running. The
    /// host asks when the app enters the background, and holds background
    /// time while it is true; afterwards `BackgroundWork` events say when
    /// it changes.
    pub fn background_work_pending(&self) -> bool {
        self.finishing.pending()
    }

    /// The host's background time ran out: finishing work is stopped
    /// (`Stopped { Interrupted }`). Commands can't be stopped; one still
    /// running finishes when the app next runs, or never happened if iOS
    /// ends the app first.
    pub fn background_time_expired(&self) {
        self.finishing.expire();
    }

    /// Runs `work` on its own thread (not the command thread, so commands
    /// keep flowing). If its scope was cancelled when it fails, the error
    /// becomes `Stopped` with the reason ([`Work::stop_kind`]), so every
    /// app reports interruption, cancellation and shutdown the same way.
    /// Dropping the future cancels the work. Never use it to save: commands
    /// do that.
    pub fn run<T, E, F>(
        &self,
        name: &str,
        work: Work,
        task: F,
    ) -> impl Future<Output = Result<T, E>>
    where
        T: Send + 'static,
        E: From<Stopped> + Send + 'static,
        F: FnOnce(&CancelHandle) -> Result<T, E> + Send + 'static,
    {
        let refused = self.shut_down.load(Ordering::SeqCst);
        let cancel = work.cancel.clone();
        let future = background_cancellable(name, cancel, move |cancel| {
            let result = task(cancel);
            let stopped = cancel.is_cancelled();
            let kind = work.stop_kind();
            drop(work.links);
            drop(work._pending);
            match result {
                Err(_) if stopped => Err(E::from(Stopped { kind })),
                other => other,
            }
        });
        async move {
            if refused {
                drop(future);
                return Err(E::from(Stopped {
                    kind: ErrorKind::ShutDown,
                }));
            }
            future.await
        }
    }

    /// The lifecycle the runtime follows, for framework parts that watch it
    /// (`forge_devtools::DevTools::start`). Apps report phases with
    /// `set_phase`, not through this.
    pub fn lifecycle(&self) -> &Lifecycle {
        &self.lifecycle
    }

    /// The last committed revision.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    /// A new event stream, or `None` after shutdown.
    pub fn events(&self) -> Option<Arc<RuntimeEvents>> {
        self.events.subscribe().map(RuntimeEvents::new)
    }

    pub fn phase(&self) -> AppPhase {
        self.lifecycle.phase().into()
    }

    /// The host reports a phase. Repeats are ignored; `Background` stops
    /// foreground work; coming back publishes `PhaseChanged { resumed:
    /// true }`; `Terminated` is `shutdown`.
    pub fn set_phase(&self, phase: AppPhase) {
        if phase == AppPhase::Terminated {
            return self.shutdown();
        }
        if let Some(change) = self.lifecycle.enter(phase.into()) {
            if phase == AppPhase::Background {
                self.finishing.phase_changed(true);
            } else if change.from == Phase::Background {
                self.finishing.phase_changed(false);
            }
            self.events.publish(RuntimeEvent::PhaseChanged {
                phase,
                resumed: change.resumed,
            });
        }
    }

    pub fn is_shut_down(&self) -> bool {
        self.shut_down.load(Ordering::SeqCst)
    }

    /// Refuses new commands and work, stops work in flight (it fails with
    /// `ShutDown`), and ends every event stream after a last
    /// `PhaseChanged(Terminated)`. Commands already accepted still run and
    /// save. Never blocks; idempotent.
    pub fn shutdown(&self) {
        if self.shut_down.swap(true, Ordering::SeqCst) {
            return;
        }
        self.executor.close();
        if self.lifecycle.enter(Phase::Terminated).is_some() {
            self.events.publish(RuntimeEvent::PhaseChanged {
                phase: AppPhase::Terminated,
                resumed: false,
            });
        }
        self.events.close();
    }

    /// Blocks until every accepted command has finished. For tests, and for
    /// a host that must know saves are done; never on the main thread.
    pub fn wait_idle(&self) {
        self.executor.wait_idle();
    }
}

impl<S> Drop for Runtime<S> {
    fn drop(&mut self) {
        // The command thread drains what was accepted, then exits and drops
        // the state. Don't wait for it here: the last reference may be
        // released on the main thread.
        self.executor.close();
    }
}

impl<S> Runtime<S> {
    /// Waits for the command thread to exit after `shutdown`. Tests only.
    #[doc(hidden)]
    pub fn join(&self) {
        self.executor.join();
    }
}
