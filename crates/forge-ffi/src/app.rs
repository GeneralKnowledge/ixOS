//! The app engine behind [`ForgeCounterApp`](crate::ForgeCounterApp): one
//! serial command thread per instance.
//!
//! # Concurrency contract
//!
//! * **Commands** (anything that changes state or touches disk) are sent to the
//!   instance's command thread and run one at a time, in the order received.
//!   That thread alone owns the engine and the store, so two commands can never
//!   interleave and no lock is held across disk I/O.
//! * A command commits only after its save succeeds. The thread then publishes
//!   the committed snapshot and emits [`CounterEvent::StateChanged`]. A failed
//!   save changes nothing and emits nothing.
//! * **Queries** read the last *committed* snapshot from a short-lived lock.
//!   They never wait for disk, so a query during a write returns the state
//!   before that write.
//! * **Long-running operations** do their slow work on their own thread and
//!   then send one commit command, so ordinary commands keep flowing while they
//!   run. They are all-or-nothing and cancellable up to that commit.
//! * **Shutdown** refuses new commands, cancels the running operation, runs the
//!   commands already accepted, waits for the operation to finish, ends every
//!   event subscription, and stops the thread. Dropping the instance does the
//!   same without waiting.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use forge_core::{CounterAction, CounterEngine, CounterSnapshot};
use forge_storage::SnapshotStore;

use crate::api::CounterStateSnapshot;
use crate::errors::ForgeAppError;
use crate::events::{CounterEvent, CounterEventSubscription, EventHub, OperationOutcome};
use crate::session::{self, StartupKind};
use crate::signal::{lock, Completion, Responder};

pub(crate) type Store = dyn SnapshotStore<Snapshot = CounterSnapshot> + Send;

/// Progress is reported at most this many times per operation.
const MAX_PROGRESS_STEPS: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenMode {
    Normal,
    RecoverCorruptData,
}

type Job = Box<dyn FnOnce(&mut Worker) + Send>;

enum Message {
    Run(Job),
    Stop,
}

/// State shared between the instance, its command thread and its operations.
struct Shared {
    committed: Mutex<CounterSnapshot>,
    events: Arc<EventHub>,
    operations: Mutex<Operations>,
}

struct Operations {
    accepting: bool,
    next_id: u64,
    active: Option<Arc<Operation>>,
}

impl Shared {
    fn snapshot(&self) -> CounterSnapshot {
        lock(&self.committed).clone()
    }

    fn publish_commit(&self, snapshot: &CounterSnapshot) {
        *lock(&self.committed) = snapshot.clone();
        self.events.publish(CounterEvent::StateChanged {
            snapshot: to_ffi(snapshot),
        });
    }

    fn has_active_operation(&self) -> bool {
        lock(&self.operations).active.is_some()
    }
}

pub(crate) fn to_ffi(snapshot: &CounterSnapshot) -> CounterStateSnapshot {
    CounterStateSnapshot {
        count: snapshot.count,
    }
}

/// Runs on the command thread. Owns the authoritative engine and the store.
struct Worker {
    engine: CounterEngine,
    store: Box<Store>,
    shared: Arc<Shared>,
}

impl Worker {
    fn apply(&mut self, action: CounterAction) -> Result<CounterSnapshot, ForgeAppError> {
        let next = session::apply_action(&mut self.engine, self.store.as_ref(), action)?;
        self.shared.publish_commit(&next);
        Ok(next)
    }

    fn commit_operation(&mut self, operation: &Arc<Operation>, amount: u32) {
        // The commit point. A cancel that arrived before this line wins.
        let result = if operation.is_cancel_requested() {
            Err(ForgeAppError::Cancelled)
        } else {
            self.apply(CounterAction::Add(amount))
        };
        finish_operation(&self.shared, operation, result);
    }
}

/// Records an operation's result, clears it as the active operation, and
/// emits its `OperationFinished` event (after any `StateChanged`).
fn finish_operation(
    shared: &Shared,
    operation: &Arc<Operation>,
    result: Result<CounterSnapshot, ForgeAppError>,
) {
    let result = result.map(|s| to_ffi(&s));
    // Clear the active slot before waking waiters, so a caller that sees the
    // result can start the next operation straight away.
    {
        let mut operations = lock(&shared.operations);
        if operation.is_finished() {
            return;
        }
        if operations
            .active
            .as_ref()
            .is_some_and(|a| Arc::ptr_eq(a, operation))
        {
            operations.active = None;
        }
        operation.result.complete(result.clone());
    }
    let outcome = match result {
        Ok(snapshot) => OperationOutcome::Completed { snapshot },
        Err(ForgeAppError::Cancelled) => OperationOutcome::Cancelled,
        Err(error) => OperationOutcome::Failed { error },
    };
    shared.events.publish(CounterEvent::OperationFinished {
        operation_id: operation.id,
        outcome,
    });
}

fn run_worker(mut worker: Worker, receiver: Receiver<Message>, stopped: Arc<Completion<()>>) {
    let mut stopping = false;
    for message in receiver.iter() {
        match message {
            Message::Run(job) => job(&mut worker),
            Message::Stop => stopping = true,
        }
        // Keep serving until the running operation (already cancelled) has
        // sent its final message, so its result and event are not lost.
        if stopping && !worker.shared.has_active_operation() {
            break;
        }
    }
    worker.shared.events.close();
    stopped.complete(());
}

/// A long-running operation: its progress, cancellation flag and result.
pub(crate) struct Operation {
    pub(crate) id: u64,
    pub(crate) total: u32,
    completed: AtomicU32,
    cancel_requested: AtomicBool,
    thread: Mutex<Option<Thread>>,
    result: Arc<Completion<Result<CounterStateSnapshot, ForgeAppError>>>,
}

impl Operation {
    pub(crate) fn completed(&self) -> u32 {
        self.completed.load(Ordering::SeqCst)
    }

    pub(crate) fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.result.get().is_some()
    }

    /// Requests cancellation. Takes effect at the next step, or at the commit
    /// point at the latest. No effect once the operation has committed.
    pub(crate) fn cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
        if let Some(thread) = lock(&self.thread).as_ref() {
            thread.unpark();
        }
    }

    pub(crate) async fn result(&self) -> Result<CounterStateSnapshot, ForgeAppError> {
        self.result.wait().await
    }

    /// Sleeps up to `duration`, returning early (and `false`) on cancellation.
    fn pause(&self, duration: Duration) -> bool {
        let deadline = Instant::now() + duration;
        loop {
            if self.is_cancel_requested() {
                return false;
            }
            let now = Instant::now();
            if now >= deadline {
                return true;
            }
            thread::park_timeout(deadline - now);
        }
    }
}

/// The engine of one app instance.
pub(crate) struct AppCore {
    sender: Mutex<Option<Sender<Message>>>,
    shared: Arc<Shared>,
    startup_kind: StartupKind,
    stopped: Arc<Completion<()>>,
}

impl AppCore {
    /// Starts the command thread, which loads (or recovers) persisted state.
    /// Resolves once the state is loaded; nothing blocks the caller meanwhile.
    pub(crate) async fn open(store: Box<Store>, mode: OpenMode) -> Result<Self, ForgeAppError> {
        let (sender, receiver) = mpsc::channel();
        let (responder, loaded) = Responder::new();
        let stopped = Completion::new();
        let thread_stopped = Arc::clone(&stopped);
        let events = EventHub::new();
        let thread_events = Arc::clone(&events);
        thread::Builder::new()
            .name("forge-counter-commands".into())
            .spawn(move || {
                let opened = match mode {
                    OpenMode::Normal => session::load_engine(store.as_ref()),
                    OpenMode::RecoverCorruptData => {
                        session::recover_after_corruption(store.as_ref())
                    }
                };
                let (engine, kind) = match opened {
                    Ok(opened) => opened,
                    Err(err) => {
                        responder.respond(Err(err));
                        thread_stopped.complete(());
                        return;
                    }
                };
                let shared = Arc::new(Shared {
                    committed: Mutex::new(engine.snapshot().clone()),
                    events: thread_events,
                    operations: Mutex::new(Operations {
                        accepting: true,
                        next_id: 1,
                        active: None,
                    }),
                });
                responder.respond(Ok((Arc::clone(&shared), kind)));
                let worker = Worker {
                    engine,
                    store,
                    shared,
                };
                run_worker(worker, receiver, thread_stopped);
            })
            .map_err(|err| ForgeAppError::internal(format!("spawn command thread: {err}")))?;

        let (shared, startup_kind) = loaded.wait().await?;
        Ok(Self {
            sender: Mutex::new(Some(sender)),
            shared,
            startup_kind,
            stopped,
        })
    }

    pub(crate) fn startup_kind(&self) -> StartupKind {
        self.startup_kind
    }

    /// The last committed state. Never waits for disk.
    pub(crate) fn snapshot(&self) -> CounterSnapshot {
        self.shared.snapshot()
    }

    fn send(&self, job: Job) -> Result<(), ForgeAppError> {
        let sender = lock(&self.sender);
        let Some(sender) = sender.as_ref() else {
            return Err(ForgeAppError::ShutDown);
        };
        // If the thread has gone, the job (and its responder) is dropped,
        // which answers the caller with `ShutDown`.
        sender
            .send(Message::Run(job))
            .map_err(|_| ForgeAppError::ShutDown)
    }

    /// Runs a command on the command thread and resolves with its result.
    pub(crate) async fn apply(
        &self,
        action: CounterAction,
    ) -> Result<CounterSnapshot, ForgeAppError> {
        let (responder, reply) = Responder::new();
        self.send(Box::new(move |worker: &mut Worker| {
            responder.respond(worker.apply(action));
        }))?;
        reply.wait().await
    }

    /// Starts adding `amount` as a long-running operation. Its slow part runs
    /// in up to [`MAX_PROGRESS_STEPS`] steps of `step_delay`, reporting progress
    /// after each, then the whole amount is committed at once.
    pub(crate) fn start_bulk_add(
        &self,
        amount: u32,
        step_delay: Duration,
    ) -> Result<Arc<Operation>, ForgeAppError> {
        if amount == 0 {
            return Err(ForgeAppError::InvalidInput {
                reason: "amount must be at least 1".into(),
            });
        }
        // Early check for a clearer error; the commit checks again against the
        // state at that moment.
        CounterEngine::from_snapshot(self.snapshot())
            .and_then(|mut engine| engine.apply(CounterAction::Add(amount)))?;

        let sender = lock(&self.sender)
            .as_ref()
            .cloned()
            .ok_or(ForgeAppError::ShutDown)?;
        let operation = {
            let mut operations = lock(&self.shared.operations);
            if !operations.accepting {
                return Err(ForgeAppError::ShutDown);
            }
            if operations.active.is_some() {
                return Err(ForgeAppError::OperationInProgress);
            }
            let operation = Arc::new(Operation {
                id: operations.next_id,
                total: amount,
                completed: AtomicU32::new(0),
                cancel_requested: AtomicBool::new(false),
                thread: Mutex::new(None),
                result: Completion::new(),
            });
            operations.next_id += 1;
            operations.active = Some(Arc::clone(&operation));
            operation
        };

        let shared = Arc::clone(&self.shared);
        let op = Arc::clone(&operation);
        let spawned = thread::Builder::new()
            .name(format!("forge-counter-operation-{}", operation.id))
            .spawn(move || run_bulk_add(&shared, &op, sender, amount, step_delay));
        if let Err(err) = spawned {
            let error = ForgeAppError::internal(format!("spawn operation thread: {err}"));
            finish_operation(&self.shared, &operation, Err(error.clone()));
            return Err(error);
        }
        Ok(operation)
    }

    pub(crate) fn active_operation(&self) -> Option<Arc<Operation>> {
        lock(&self.shared.operations).active.clone()
    }

    pub(crate) fn subscribe(&self) -> Result<CounterEventSubscription, ForgeAppError> {
        self.shared.events.subscribe()
    }

    pub(crate) fn subscriber_count(&self) -> usize {
        self.shared.events.subscriber_count()
    }

    pub(crate) fn is_shut_down(&self) -> bool {
        lock(&self.sender).is_none()
    }

    /// Begins shutdown without waiting. Idempotent.
    fn begin_shutdown(&self) {
        let active = {
            let mut operations = lock(&self.shared.operations);
            operations.accepting = false;
            operations.active.clone()
        };
        if let Some(operation) = active {
            operation.cancel();
        }
        if let Some(sender) = lock(&self.sender).take() {
            // Fails only if the thread already stopped, which is the goal.
            let _ = sender.send(Message::Stop);
        }
    }

    /// Shuts down and resolves once the command thread has stopped: every
    /// accepted command has run, the operation has finished, and every event
    /// subscription has ended.
    pub(crate) async fn shutdown(&self) {
        self.begin_shutdown();
        self.stopped.wait().await;
    }
}

impl Drop for AppCore {
    fn drop(&mut self) {
        self.begin_shutdown();
    }
}

fn run_bulk_add(
    shared: &Shared,
    operation: &Arc<Operation>,
    sender: Sender<Message>,
    amount: u32,
    step_delay: Duration,
) {
    *lock(&operation.thread) = Some(thread::current());
    let steps = amount.min(MAX_PROGRESS_STEPS);
    let mut cancelled = false;
    for step in 1..=steps {
        // The slow part: stands in for parsing a file, a network call, etc.
        if !operation.pause(step_delay) {
            cancelled = true;
            break;
        }
        let completed = (u64::from(amount) * u64::from(step) / u64::from(steps)) as u32;
        operation.completed.store(completed, Ordering::SeqCst);
        shared.events.publish(CounterEvent::OperationProgress {
            operation_id: operation.id,
            completed,
            total: amount,
        });
    }

    let job: Job = if cancelled {
        let op = Arc::clone(operation);
        Box::new(move |worker: &mut Worker| {
            finish_operation(&worker.shared, &op, Err(ForgeAppError::Cancelled));
        })
    } else {
        let op = Arc::clone(operation);
        Box::new(move |worker: &mut Worker| worker.commit_operation(&op, amount))
    };
    if sender.send(Message::Run(job)).is_err() {
        // The command thread is gone, so nothing can commit.
        finish_operation(shared, operation, Err(ForgeAppError::Cancelled));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::block_on;
    use forge_storage::{JsonSnapshotStore, StorageError};
    use std::sync::mpsc::SyncSender;

    /// In-memory store whose saves can be made to fail or to pause.
    #[derive(Clone, Default)]
    struct TestStore {
        inner: Arc<Mutex<TestStoreState>>,
    }

    type SaveGate = (SyncSender<()>, Arc<Mutex<Receiver<()>>>);

    #[derive(Default)]
    struct TestStoreState {
        saved: Option<CounterSnapshot>,
        saves: usize,
        fail_saves: bool,
        /// When set, each save reports "started" and then waits for "go".
        gate: Option<SaveGate>,
    }

    impl TestStore {
        fn saved(&self) -> Option<i64> {
            lock(&self.inner).saved.as_ref().map(|s| s.count)
        }

        fn saves(&self) -> usize {
            lock(&self.inner).saves
        }

        fn fail_saves(&self, fail: bool) {
            lock(&self.inner).fail_saves = fail;
        }

        /// Pauses every later save. Returns (started, go).
        fn gate(&self) -> (Receiver<()>, SyncSender<()>) {
            let (started_tx, started_rx) = mpsc::sync_channel(8);
            let (go_tx, go_rx) = mpsc::sync_channel(8);
            lock(&self.inner).gate = Some((started_tx, Arc::new(Mutex::new(go_rx))));
            (started_rx, go_tx)
        }
    }

    impl SnapshotStore for TestStore {
        type Snapshot = CounterSnapshot;

        fn load(&self) -> Result<Option<CounterSnapshot>, StorageError> {
            Ok(lock(&self.inner).saved.clone())
        }

        fn save(&self, snapshot: &CounterSnapshot) -> Result<(), StorageError> {
            let gate = lock(&self.inner).gate.clone();
            if let Some((started, go)) = gate {
                started.send(()).unwrap();
                lock(&go).recv().unwrap();
            }
            let mut state = lock(&self.inner);
            if state.fail_saves {
                return Err(StorageError::WriteFailed {
                    reason: "test".into(),
                });
            }
            state.saved = Some(snapshot.clone());
            state.saves += 1;
            Ok(())
        }
    }

    fn open(store: &TestStore) -> AppCore {
        block_on(AppCore::open(Box::new(store.clone()), OpenMode::Normal)).unwrap()
    }

    fn changed(count: i64) -> CounterEvent {
        CounterEvent::StateChanged {
            snapshot: CounterStateSnapshot { count },
        }
    }

    fn drain(sub: &CounterEventSubscription) -> Vec<CounterEvent> {
        let mut events = Vec::new();
        while let Some(event) = block_on(sub.next()) {
            events.push(event);
        }
        events
    }

    #[test]
    fn async_apply_commits_and_persists() {
        let store = TestStore::default();
        let app = open(&store);
        assert_eq!(app.startup_kind(), StartupKind::FreshInstall);
        let snap = block_on(app.apply(CounterAction::Increment)).unwrap();
        assert_eq!(snap.count, 1);
        assert_eq!(app.snapshot().count, 1);
        assert_eq!(store.saved(), Some(1));
    }

    #[test]
    fn concurrent_commands_are_serialised_without_lost_updates() {
        let store = TestStore::default();
        let app = Arc::new(open(&store));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let app = Arc::clone(&app);
                thread::spawn(move || {
                    for _ in 0..25 {
                        block_on(app.apply(CounterAction::Increment)).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(app.snapshot().count, 200);
        assert_eq!(store.saved(), Some(200));
        assert_eq!(store.saves(), 200);
    }

    #[test]
    fn read_during_write_returns_last_committed_state() {
        let store = TestStore::default();
        let app = Arc::new(open(&store));
        block_on(app.apply(CounterAction::Increment)).unwrap();
        let (started, go) = store.gate();
        let writer = {
            let app = Arc::clone(&app);
            thread::spawn(move || block_on(app.apply(CounterAction::Increment)))
        };
        started.recv().unwrap();
        // The save is in progress: queries answer immediately with the old value.
        assert_eq!(app.snapshot().count, 1);
        go.send(()).unwrap();
        assert_eq!(writer.join().unwrap().unwrap().count, 2);
        assert_eq!(app.snapshot().count, 2);
    }

    #[test]
    fn failed_save_changes_nothing_and_emits_nothing() {
        let store = TestStore::default();
        let app = open(&store);
        block_on(app.apply(CounterAction::Increment)).unwrap();
        let sub = app.subscribe().unwrap();
        store.fail_saves(true);
        let err = block_on(app.apply(CounterAction::Increment)).unwrap_err();
        assert!(matches!(err, ForgeAppError::PersistenceFailed { .. }));
        assert_eq!(app.snapshot().count, 1);
        assert_eq!(store.saved(), Some(1));
        block_on(app.shutdown());
        assert!(drain(&sub).is_empty(), "no StateChanged for a failed save");
    }

    #[test]
    fn state_change_is_reported_only_after_the_save() {
        let store = TestStore::default();
        let app = Arc::new(open(&store));
        let sub = app.subscribe().unwrap();
        let (started, go) = store.gate();
        let writer = {
            let app = Arc::clone(&app);
            thread::spawn(move || block_on(app.apply(CounterAction::Increment)))
        };
        started.recv().unwrap();
        // While the save is blocked, there is no event to read.
        let probe = thread::spawn(move || {
            let event = block_on(sub.next());
            (event, sub)
        });
        thread::sleep(Duration::from_millis(50));
        assert!(!probe.is_finished());
        assert_eq!(store.saved(), None);
        go.send(()).unwrap();
        let (event, _sub) = probe.join().unwrap();
        assert_eq!(event, Some(changed(1)));
        assert_eq!(store.saved(), Some(1));
        writer.join().unwrap().unwrap();
    }

    #[test]
    fn domain_errors_propagate_typed() {
        let app = open(&TestStore::default());
        assert_eq!(
            block_on(app.apply(CounterAction::Decrement)),
            Err(ForgeAppError::WouldUnderflow)
        );
    }

    #[test]
    fn bulk_add_reports_progress_then_completes() {
        let store = TestStore::default();
        let app = open(&store);
        let sub = app.subscribe().unwrap();
        let op = app.start_bulk_add(40, Duration::from_millis(1)).unwrap();
        let result = block_on(op.result()).unwrap();
        assert_eq!(result.count, 40);
        assert_eq!(app.snapshot().count, 40);
        assert_eq!(store.saved(), Some(40));
        assert!(app.active_operation().is_none());
        block_on(app.shutdown());

        let events = drain(&sub);
        let progress: Vec<u32> = events
            .iter()
            .filter_map(|e| match e {
                CounterEvent::OperationProgress { completed, .. } => Some(*completed),
                _ => None,
            })
            .collect();
        assert_eq!(progress.len(), MAX_PROGRESS_STEPS as usize);
        assert!(progress.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(progress.last(), Some(&40));
        let tail = &events[events.len() - 2..];
        assert_eq!(tail[0], changed(40));
        assert_eq!(
            tail[1],
            CounterEvent::OperationFinished {
                operation_id: op.id,
                outcome: OperationOutcome::Completed {
                    snapshot: CounterStateSnapshot { count: 40 }
                },
            }
        );
    }

    #[test]
    fn cancelled_bulk_add_commits_nothing() {
        let store = TestStore::default();
        let app = open(&store);
        let sub = app.subscribe().unwrap();
        let op = app.start_bulk_add(100, Duration::from_secs(10)).unwrap();
        let started = Instant::now();
        op.cancel();
        assert_eq!(block_on(op.result()), Err(ForgeAppError::Cancelled));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cancel is prompt"
        );
        assert_eq!(app.snapshot().count, 0);
        assert_eq!(store.saves(), 0);
        block_on(app.shutdown());
        assert_eq!(
            drain(&sub),
            vec![CounterEvent::OperationFinished {
                operation_id: op.id,
                outcome: OperationOutcome::Cancelled,
            }]
        );
    }

    #[test]
    fn cancel_after_commit_is_a_no_op() {
        let app = open(&TestStore::default());
        let op = app.start_bulk_add(3, Duration::ZERO).unwrap();
        let result = block_on(op.result());
        op.cancel();
        assert_eq!(result.unwrap().count, 3);
        assert_eq!(block_on(op.result()).unwrap().count, 3);
        assert_eq!(app.snapshot().count, 3);
    }

    #[test]
    fn commands_keep_running_during_a_bulk_add() {
        let app = open(&TestStore::default());
        let op = app.start_bulk_add(10, Duration::from_millis(30)).unwrap();
        block_on(app.apply(CounterAction::Increment)).unwrap();
        block_on(app.apply(CounterAction::Increment)).unwrap();
        assert!(
            !op.is_finished(),
            "increments did not wait for the operation"
        );
        // The operation commits on top of the state at its commit point.
        assert_eq!(block_on(op.result()).unwrap().count, 12);
    }

    #[test]
    fn one_operation_at_a_time() {
        let app = open(&TestStore::default());
        let first = app.start_bulk_add(5, Duration::from_secs(10)).unwrap();
        assert!(matches!(
            app.start_bulk_add(5, Duration::ZERO),
            Err(ForgeAppError::OperationInProgress)
        ));
        first.cancel();
        let _ = block_on(first.result());
        assert!(app.start_bulk_add(5, Duration::ZERO).is_ok());
    }

    #[test]
    fn bulk_add_validates_input() {
        let app = open(&TestStore::default());
        assert!(matches!(
            app.start_bulk_add(0, Duration::ZERO),
            Err(ForgeAppError::InvalidInput { .. })
        ));
        assert!(matches!(
            app.start_bulk_add(u32::MAX, Duration::ZERO),
            Err(ForgeAppError::WouldOverflow)
        ));
    }

    #[test]
    fn bulk_add_save_failure_is_reported_and_commits_nothing() {
        let store = TestStore::default();
        let app = open(&store);
        store.fail_saves(true);
        let op = app.start_bulk_add(5, Duration::ZERO).unwrap();
        assert!(matches!(
            block_on(op.result()),
            Err(ForgeAppError::PersistenceFailed { .. })
        ));
        assert_eq!(app.snapshot().count, 0);
    }

    #[test]
    fn shutdown_cancels_the_operation_and_ends_subscriptions() {
        let store = TestStore::default();
        let app = open(&store);
        let sub = app.subscribe().unwrap();
        let op = app.start_bulk_add(100, Duration::from_secs(10)).unwrap();
        block_on(app.shutdown());
        assert!(app.is_shut_down());
        assert_eq!(block_on(op.result()), Err(ForgeAppError::Cancelled));
        assert_eq!(store.saves(), 0);
        // The final event is delivered before the stream ends.
        assert_eq!(
            drain(&sub),
            vec![CounterEvent::OperationFinished {
                operation_id: op.id,
                outcome: OperationOutcome::Cancelled,
            }]
        );
    }

    #[test]
    fn commands_accepted_before_shutdown_still_run() {
        let store = TestStore::default();
        let app = Arc::new(open(&store));
        let (started, go) = store.gate();
        let first = {
            let app = Arc::clone(&app);
            thread::spawn(move || block_on(app.apply(CounterAction::Increment)))
        };
        started.recv().unwrap();
        let second = {
            let app = Arc::clone(&app);
            thread::spawn(move || block_on(app.apply(CounterAction::Increment)))
        };
        // Give the second command time to be queued behind the first.
        thread::sleep(Duration::from_millis(50));
        let closer = {
            let app = Arc::clone(&app);
            thread::spawn(move || block_on(app.shutdown()))
        };
        thread::sleep(Duration::from_millis(50));
        go.send(()).unwrap();
        go.send(()).unwrap();
        assert_eq!(first.join().unwrap().unwrap().count, 1);
        assert_eq!(second.join().unwrap().unwrap().count, 2);
        closer.join().unwrap();
        assert_eq!(store.saved(), Some(2));
    }

    #[test]
    fn everything_after_shutdown_is_refused() {
        let app = open(&TestStore::default());
        block_on(app.shutdown());
        block_on(app.shutdown()); // idempotent
        assert_eq!(
            block_on(app.apply(CounterAction::Increment)),
            Err(ForgeAppError::ShutDown)
        );
        assert!(matches!(
            app.start_bulk_add(1, Duration::ZERO),
            Err(ForgeAppError::ShutDown)
        ));
        assert!(matches!(app.subscribe(), Err(ForgeAppError::ShutDown)));
        // Queries still answer with the last committed state.
        assert_eq!(app.snapshot().count, 0);
    }

    #[test]
    fn dropping_the_app_ends_subscriptions_and_operations() {
        let app = open(&TestStore::default());
        let sub = app.subscribe().unwrap();
        let op = app.start_bulk_add(100, Duration::from_secs(10)).unwrap();
        drop(app);
        assert_eq!(block_on(op.result()), Err(ForgeAppError::Cancelled));
        let events = drain(&sub);
        assert_eq!(
            events.last(),
            Some(&CounterEvent::OperationFinished {
                operation_id: op.id,
                outcome: OperationOutcome::Cancelled,
            })
        );
    }

    #[test]
    fn open_failure_is_reported_and_stops_the_thread() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSnapshotStore::new(dir.path());
        std::fs::write(store.path(), "{bad").unwrap();
        let result = block_on(AppCore::open(Box::new(store.clone()), OpenMode::Normal));
        assert!(matches!(
            result,
            Err(ForgeAppError::CorruptPersistence { .. })
        ));
        let recovered =
            block_on(AppCore::open(Box::new(store), OpenMode::RecoverCorruptData)).unwrap();
        assert_eq!(recovered.startup_kind(), StartupKind::FreshInstall);
        assert_eq!(recovered.snapshot().count, 0);
    }

    #[test]
    fn real_store_survives_reopen_after_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let open_json = || {
            block_on(AppCore::open(
                Box::new(JsonSnapshotStore::new(dir.path())),
                OpenMode::Normal,
            ))
            .unwrap()
        };
        let app = open_json();
        block_on(app.apply(CounterAction::Increment)).unwrap();
        let op = app.start_bulk_add(4, Duration::ZERO).unwrap();
        block_on(op.result()).unwrap();
        block_on(app.shutdown());
        let reopened = open_json();
        assert_eq!(reopened.startup_kind(), StartupKind::Restored);
        assert_eq!(reopened.snapshot().count, 5);
    }
}
