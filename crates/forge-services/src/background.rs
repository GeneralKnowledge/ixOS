//! The `background` optional service: iOS background tasks
//! (`BGTaskScheduler`), with Swift registering them at launch and Rust doing
//! the work in the window iOS grants (docs/native-services.md, "Background
//! tasks").
//!
//! The split follows the contract in docs/native-capability-roadmap.md:
//!
//! - Swift registers every identifier listed in forge.toml
//!   (`[ios] background_tasks`, written to `BGTaskSchedulerPermittedIdentifiers`)
//!   during launch, as iOS requires, submits requests and receives each task.
//! - Rust registers a handler per identifier and decides when to ask again.
//! - Swift hands each granted window to Rust as a [`BackgroundTaskRun`]: its
//!   expiry becomes a cancellation, and the handler's `finish` becomes
//!   `setTaskCompleted`.
//!
//! The two jobs apps repeat are built in: a refresh that asks to run again
//! every so often (`register_refresh`), and heavy upkeep that waits for power
//! or the network (`register_processing`). Both submit their own next
//! request, so an app registers once when its core opens and nothing more.
//! Handlers are async (they usually call the core's own async methods); each
//! run gets its own thread.
//!
//! iOS may start a task before the core has opened (it launches the app in
//! the background for it). Such a run waits for its handler, and fails if
//! the window ends first.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Drives `future` to completion on the calling thread. Background runs
/// have their own thread, and the futures they await (a core's commands)
/// complete on other threads, so parking until woken is enough.
#[doc(hidden)]
pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! __native_background {
    () => {
        /// Background tasks: work iOS runs while the app is in the
        /// background, when it chooses. iOS: BackgroundTasks. Needs each
        /// identifier in forge.toml `[ios] background_tasks`. iOS decides
        /// whether and when a task runs (usage, battery, Low Power Mode); a
        /// task never runs after the person force-quits the app.
        pub mod background {
            use ::std::collections::HashMap;
            use ::std::sync::atomic::{AtomicBool, Ordering};
            use ::std::sync::{Arc, Mutex};

            /// The kind of window to ask iOS for.
            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
            pub enum BackgroundTaskKind {
                /// A short refresh (about 30 seconds): fetch new content.
                Refresh,
                /// A longer window (minutes), usually overnight: upkeep,
                /// indexing, uploads. iOS can wait for power or a network.
                Processing {
                    requires_external_power: bool,
                    requires_network: bool,
                },
            }

            /// One request to iOS.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq)]
            pub struct BackgroundTaskRequest {
                pub identifier: String,
                pub kind: BackgroundTaskKind,
                /// Not before this many seconds from now. iOS treats it as
                /// the earliest time, never a promise.
                pub earliest_begin_seconds: f64,
            }

            /// Implemented by the host (iOS: `BGTaskScheduler`).
            #[::uniffi::export(with_foreign)]
            pub trait BackgroundScheduler: Send + Sync {
                /// Submits `request`, replacing an earlier one with the same
                /// identifier.
                fn submit(&self, request: BackgroundTaskRequest) -> Result<(), super::NativeServiceError>;
                /// Withdraws the request with `identifier`, if any.
                fn cancel(&self, identifier: String);
                /// The identifiers the host registered at launch (iOS: the
                /// app's `BGTaskSchedulerPermittedIdentifiers`).
                fn permitted(&self) -> Vec<String>;
            }

            /// How the host learns that a run has finished. iOS:
            /// `BGTask.setTaskCompleted(success:)`.
            #[::uniffi::export(with_foreign)]
            pub trait BackgroundTaskCompletion: Send + Sync {
                fn finished(&self, success: bool);
            }

            /// One window iOS granted. The handler does its work, checks
            /// `is_cancelled` (or waits on `cancelled`) as it goes, and calls
            /// `finish` exactly once; later calls are ignored. A run dropped
            /// without `finish` reports failure.
            #[derive(::uniffi::Object)]
            pub struct BackgroundTaskRun {
                identifier: String,
                cancelled: AtomicBool,
                /// Set when a handler took the run.
                dispatched: AtomicBool,
                completion: Mutex<Option<Arc<dyn BackgroundTaskCompletion>>>,
                waiters: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
            }

            #[::uniffi::export]
            impl BackgroundTaskRun {
                /// Called by the host when the window runs out. The handler
                /// should stop soon after and `finish(false)`; if it has not
                /// finished when iOS insists, iOS ends the run anyway.
                pub fn expire(&self) {
                    self.cancelled.store(true, Ordering::SeqCst);
                    let waiters: Vec<_> = self.waiters.lock().unwrap().drain(..).collect();
                    for waiter in waiters {
                        waiter();
                    }
                    // Still waiting for its handler: nothing ran.
                    if !self.dispatched.load(Ordering::SeqCst) {
                        self.finish(false);
                    }
                }
            }

            impl BackgroundTaskRun {
                fn new(identifier: String, completion: Arc<dyn BackgroundTaskCompletion>) -> Self {
                    Self {
                        identifier,
                        cancelled: AtomicBool::new(false),
                        dispatched: AtomicBool::new(false),
                        completion: Mutex::new(Some(completion)),
                        waiters: Mutex::new(Vec::new()),
                    }
                }

                pub fn identifier(&self) -> &str {
                    &self.identifier
                }

                /// True once iOS has asked the run to stop.
                pub fn is_cancelled(&self) -> bool {
                    self.cancelled.load(Ordering::SeqCst)
                }

                /// Calls `f` once when iOS ends the window (at once if it
                /// already has): cancel the app's own work with it, such as a
                /// `forge_app::Cancellation`.
                pub fn on_cancel(&self, f: impl FnOnce() + Send + 'static) {
                    let mut waiters = self.waiters.lock().unwrap();
                    if self.is_cancelled() {
                        drop(waiters);
                        f();
                    } else {
                        waiters.push(Box::new(f));
                    }
                }

                /// A receiver that gets a message when the run is cancelled
                /// (at once if it already is), for work that waits.
                pub fn cancelled(&self) -> ::std::sync::mpsc::Receiver<()> {
                    let (tx, rx) = ::std::sync::mpsc::channel();
                    self.on_cancel(move || {
                        let _ = tx.send(());
                    });
                    rx
                }

                /// Reports the run's result to iOS. Only the first call counts.
                pub fn finish(&self, success: bool) {
                    let completion = self.completion.lock().unwrap().take();
                    if let Some(completion) = completion {
                        completion.finished(success);
                    }
                }

                /// Whether `finish` has been called.
                pub fn is_finished(&self) -> bool {
                    self.completion.lock().unwrap().is_none()
                }
            }

            impl Drop for BackgroundTaskRun {
                fn drop(&mut self) {
                    if let Some(completion) = self.completion.get_mut().unwrap().take() {
                        completion.finished(false);
                    }
                }
            }

            type BoxedRun = ::std::pin::Pin<Box<dyn ::std::future::Future<Output = bool> + Send>>;
            type BackgroundHandler = Arc<dyn Fn(Arc<BackgroundTaskRun>) -> BoxedRun + Send + Sync>;

            fn boxed<F, Fut>(handler: F) -> BackgroundHandler
            where
                F: Fn(Arc<BackgroundTaskRun>) -> Fut + Send + Sync + 'static,
                Fut: ::std::future::Future<Output = bool> + Send + 'static,
            {
                Arc::new(move |run| Box::pin(handler(run)))
            }

            #[derive(Clone)]
            struct Registration {
                handler: BackgroundHandler,
                /// Submitted again at the start of every run (and at
                /// registration), so the task keeps recurring.
                next: Option<BackgroundTaskRequest>,
            }

            /// Called by the host when iOS starts a task. Runs the handler
            /// registered for `identifier`, or finishes at once (failure) if
            /// there is none.
            #[::uniffi::export]
            pub fn run_native_background_task(
                identifier: String,
                completion: Arc<dyn BackgroundTaskCompletion>,
            ) -> Arc<BackgroundTaskRun> {
                let run = Arc::new(BackgroundTaskRun::new(identifier.clone(), completion));
                match BackgroundService::installed() {
                    Ok(service) => service.run(run.clone()),
                    Err(_) => run.finish(false),
                }
                run
            }

            #[::uniffi::export]
            pub fn install_native_background(scheduler: Arc<dyn BackgroundScheduler>) {
                BackgroundService::new(scheduler).install();
            }

            /// Background tasks for Rust code. Cheap to clone; clones share
            /// their registrations.
            #[derive(Clone)]
            pub struct BackgroundService {
                scheduler: Arc<dyn BackgroundScheduler>,
                registrations: Arc<Mutex<HashMap<String, Registration>>>,
                /// Runs iOS started before their handler was registered.
                waiting: Arc<Mutex<Vec<Arc<BackgroundTaskRun>>>>,
            }

            $crate::__native_slot!(BackgroundService, "background");

            impl BackgroundService {
                pub fn new(scheduler: Arc<dyn BackgroundScheduler>) -> Self {
                    Self {
                        scheduler,
                        registrations: Arc::new(Mutex::new(HashMap::new())),
                        waiting: Arc::new(Mutex::new(Vec::new())),
                    }
                }

                fn check(&self, identifier: &str) -> Result<(), super::NativeServiceError> {
                    if self.scheduler.permitted().iter().any(|p| p == identifier) {
                        Ok(())
                    } else {
                        Err(super::NativeServiceError::InvalidInput {
                            reason: format!(
                                "background task \"{identifier}\" is not in forge.toml [ios] background_tasks"
                            ),
                        })
                    }
                }

                /// A refresh that recurs: asks iOS to run `handler` no sooner
                /// than `every_seconds` from now, and again that long after
                /// each run starts. For fetching new content (feeds, mail,
                /// weather) so it is there when the app opens. The handler
                /// returns whether it succeeded; it should stop soon after the
                /// run `is_cancelled`, and a cancelled run reports failure.
                pub fn register_refresh<F, Fut>(
                    &self,
                    identifier: &str,
                    every_seconds: f64,
                    handler: F,
                ) -> Result<(), super::NativeServiceError>
                where
                    F: Fn(Arc<BackgroundTaskRun>) -> Fut + Send + Sync + 'static,
                    Fut: ::std::future::Future<Output = bool> + Send + 'static,
                {
                    self.register(
                        identifier,
                        Some(BackgroundTaskRequest {
                            identifier: identifier.into(),
                            kind: BackgroundTaskKind::Refresh,
                            earliest_begin_seconds: every_seconds,
                        }),
                        boxed(handler),
                    )
                }

                /// Upkeep that recurs in longer windows, which iOS usually
                /// gives overnight on power: compacting a database, rebuilding
                /// an index, a backup, uploads.
                pub fn register_processing<F, Fut>(
                    &self,
                    identifier: &str,
                    every_seconds: f64,
                    requires_external_power: bool,
                    requires_network: bool,
                    handler: F,
                ) -> Result<(), super::NativeServiceError>
                where
                    F: Fn(Arc<BackgroundTaskRun>) -> Fut + Send + Sync + 'static,
                    Fut: ::std::future::Future<Output = bool> + Send + 'static,
                {
                    self.register(
                        identifier,
                        Some(BackgroundTaskRequest {
                            identifier: identifier.into(),
                            kind: BackgroundTaskKind::Processing {
                                requires_external_power,
                                requires_network,
                            },
                            earliest_begin_seconds: every_seconds,
                        }),
                        boxed(handler),
                    )
                }

                /// A handler for one-off requests the app submits itself with
                /// `schedule` (finish a job that ran out of time, say).
                pub fn register_handler<F, Fut>(
                    &self,
                    identifier: &str,
                    handler: F,
                ) -> Result<(), super::NativeServiceError>
                where
                    F: Fn(Arc<BackgroundTaskRun>) -> Fut + Send + Sync + 'static,
                    Fut: ::std::future::Future<Output = bool> + Send + 'static,
                {
                    self.register(identifier, None, boxed(handler))
                }

                fn register(
                    &self,
                    identifier: &str,
                    next: Option<BackgroundTaskRequest>,
                    handler: BackgroundHandler,
                ) -> Result<(), super::NativeServiceError> {
                    self.check(identifier)?;
                    if let Some(request) = &next {
                        if !(request.earliest_begin_seconds >= 0.0) {
                            return Err(super::NativeServiceError::InvalidInput {
                                reason: "background task interval must be zero or more seconds".into(),
                            });
                        }
                    }
                    self.registrations
                        .lock()
                        .unwrap()
                        .insert(identifier.into(), Registration { handler, next: next.clone() });
                    let submitted = match next {
                        Some(request) => self.scheduler.submit(request),
                        None => Ok(()),
                    };
                    // Runs iOS started before this handler existed.
                    let ready: Vec<_> = {
                        let mut waiting = self.waiting.lock().unwrap();
                        let (ready, rest) = waiting.drain(..).partition(|r| r.identifier() == identifier);
                        *waiting = rest;
                        ready
                    };
                    for run in ready {
                        self.run(run);
                    }
                    submitted
                }

                /// Submits a one-off request (replacing any pending one with
                /// the same identifier).
                pub fn schedule(&self, request: BackgroundTaskRequest) -> Result<(), super::NativeServiceError> {
                    self.check(&request.identifier)?;
                    self.scheduler.submit(request)
                }

                /// Withdraws a pending request and forgets its handler, so a
                /// recurring task stops recurring.
                pub fn cancel(&self, identifier: &str) {
                    self.registrations.lock().unwrap().remove(identifier);
                    self.scheduler.cancel(identifier.into());
                }

                fn run(&self, run: Arc<BackgroundTaskRun>) {
                    if run.is_finished() {
                        return;
                    }
                    let registration = self.registrations.lock().unwrap().get(run.identifier()).cloned();
                    let Some(registration) = registration else {
                        // iOS launched the app for this task before the core
                        // opened and registered its handler: wait for it.
                        // The run fails if its window ends first.
                        let mut waiting = self.waiting.lock().unwrap();
                        waiting.retain(|r| !r.is_finished());
                        waiting.push(run);
                        return;
                    };
                    run.dispatched.store(true, Ordering::SeqCst);
                    // Ask for the next window first, as Apple recommends, so
                    // a run that is cut short still recurs.
                    if let Some(next) = registration.next {
                        let _ = self.scheduler.submit(next);
                    }
                    let work = (registration.handler)(run.clone());
                    let spawned = ::std::thread::Builder::new()
                        .name(format!("forge-background-{}", run.identifier()))
                        .spawn({
                            let run = run.clone();
                            move || {
                                let ok = $crate::background::block_on(work);
                                run.finish(ok && !run.is_cancelled());
                            }
                        });
                    if spawned.is_err() {
                        run.finish(false);
                    }
                }

                /// Runs waiting for a handler (for tests and diagnostics).
                pub fn waiting(&self) -> usize {
                    self.waiting.lock().unwrap().len()
                }

                pub fn for_tests(permitted: &[&str]) -> (Self, Arc<testing::FakeScheduler>) {
                    let fake = Arc::new(testing::FakeScheduler::new(permitted));
                    (Self::new(fake.clone()), fake)
                }
            }

            pub mod testing {
                use super::*;

                /// Records submitted requests, and starts runs the way iOS
                /// would.
                pub struct FakeScheduler {
                    permitted: Vec<String>,
                    pub submitted: Mutex<Vec<BackgroundTaskRequest>>,
                    pub cancelled: Mutex<Vec<String>>,
                }

                impl FakeScheduler {
                    pub fn new(permitted: &[&str]) -> Self {
                        Self {
                            permitted: permitted.iter().map(|p| p.to_string()).collect(),
                            submitted: Mutex::new(Vec::new()),
                            cancelled: Mutex::new(Vec::new()),
                        }
                    }

                    /// The completion results, in order, of runs started with
                    /// `start`.
                    pub fn start(
                        service: &BackgroundService,
                        identifier: &str,
                    ) -> (Arc<BackgroundTaskRun>, Arc<Mutex<Vec<bool>>>) {
                        let results = Arc::new(Mutex::new(Vec::new()));
                        let run = Arc::new(BackgroundTaskRun::new(
                            identifier.into(),
                            Arc::new(Recorder(results.clone())),
                        ));
                        service.run(run.clone());
                        (run, results)
                    }
                }

                struct Recorder(Arc<Mutex<Vec<bool>>>);

                impl BackgroundTaskCompletion for Recorder {
                    fn finished(&self, success: bool) {
                        self.0.lock().unwrap().push(success);
                    }
                }

                impl BackgroundScheduler for FakeScheduler {
                    fn submit(&self, request: BackgroundTaskRequest) -> Result<(), super::super::NativeServiceError> {
                        self.submitted.lock().unwrap().push(request);
                        Ok(())
                    }
                    fn cancel(&self, identifier: String) {
                        self.cancelled.lock().unwrap().push(identifier);
                    }
                    fn permitted(&self) -> Vec<String> {
                        self.permitted.clone()
                    }
                }
            }
        }
    };
}
