//! The runtime an app's core embeds.

use std::fs::{File, OpenOptions, TryLockError};
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use forge_runtime::{background_cancellable, CancelHandle, EventHub, Lifecycle, Phase};

use crate::contract::{AppPhase, Cancellation, ErrorKind, RuntimeEvent, RuntimeEvents, Stopped};
use crate::executor::{reply, Executor};

const LOCK_FILE: &str = ".forge-app.lock";

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
    directory: Option<DataDirectory>,
}

/// A unit of non-command work and the scope that can stop it.
pub struct Work {
    cancel: CancelHandle,
    links: Vec<Box<dyn std::any::Any + Send>>,
    lifetime: CancelHandle,
    foreground: Option<CancelHandle>,
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
        } else if self.foreground.as_ref().is_some_and(|f| f.is_cancelled()) {
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
        Self {
            executor: Executor::start(name, state),
            revision: Arc::new(AtomicU64::new(0)),
            lifecycle: Lifecycle::new(),
            events: EventHub::new(),
            shut_down: AtomicBool::new(false),
            directory,
        }
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
            let job = Box::new(move |state: &mut S| {
                let result = catch_unwind(AssertUnwindSafe(|| command(state)));
                if matches!(result, Ok(Ok(_))) {
                    let revision = revision.fetch_add(1, Ordering::SeqCst) + 1;
                    events.publish(RuntimeEvent::Changed { revision });
                }
                if result.is_err() {
                    forge_runtime::logging::error!("command {name} panicked");
                }
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
        self.work(&foreground, Some(foreground.clone()), host)
    }

    /// Work that may continue in the background until iOS suspends the
    /// process (it then freezes): stopped by `host` or by shutdown.
    pub fn lifetime(&self, host: Option<&Cancellation>) -> Work {
        let lifetime = self.lifecycle.lifetime();
        self.work(&lifetime, None, host)
    }

    fn work(
        &self,
        parent: &CancelHandle,
        foreground: Option<CancelHandle>,
        host: Option<&Cancellation>,
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
            foreground,
        }
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
