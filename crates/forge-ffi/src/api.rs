//! The UniFFI surface: commands, queries, events and lifecycle.
//!
//! | Kind      | Exports |
//! |-----------|---------|
//! | Open      | [`open_counter_app`] (async); `ForgeCounterApp::new` and [`recover_counter_app`] (sync, kept for existing callers) |
//! | Commands  | `apply_action` (async), `start_bulk_add` (long-running, returns a [`CounterOperation`]); `apply` (sync, kept) |
//! | Queries   | `snapshot`, `startup_kind`, `active_operation` (never wait for disk) |
//! | Events    | `subscribe` → [`CounterEventSubscription`](crate::CounterEventSubscription) |
//! | Lifecycle | `shutdown` (async), `is_shut_down` |
//!
//! See `app.rs` for the concurrency contract and `docs/ffi-policy.md` for how
//! hosts should use it.

use std::sync::Arc;
use std::time::Duration;

use forge_core::CounterAction;
use forge_storage::JsonSnapshotStore;

use crate::app::{self, AppCore, Operation};
use crate::errors::ForgeAppError;
use crate::events::CounterEventSubscription;
use crate::session::StartupKind as InternalStartupKind;
use crate::signal::block_on;

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct CounterStateSnapshot {
    pub count: i64,
}

#[derive(uniffi::Enum, Clone, Debug)]
pub enum ForgeActionKind {
    Increment,
    Decrement,
    Reset,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupKind {
    FreshInstall,
    Restored,
}

/// How [`open_counter_app`] treats existing data.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    /// Restore valid data; report corrupt data as `CorruptPersistence`.
    Normal,
    /// Like [`recover_counter_app`]: replace the data with a fresh counter
    /// **only** if it is corrupt. Newer-schema or unreadable data is never
    /// overwritten.
    RecoverCorruptData,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct ActionOutcome {
    pub snapshot: CounterStateSnapshot,
}

/// Point-in-time status of a long-running operation.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct OperationStatus {
    pub operation_id: u64,
    pub completed: u32,
    pub total: u32,
    pub cancel_requested: bool,
    pub finished: bool,
}

fn to_public_startup(kind: InternalStartupKind) -> StartupKind {
    match kind {
        InternalStartupKind::FreshInstall => StartupKind::FreshInstall,
        InternalStartupKind::Restored => StartupKind::Restored,
    }
}

fn to_core_action(action: ForgeActionKind) -> CounterAction {
    match action {
        ForgeActionKind::Increment => CounterAction::Increment,
        ForgeActionKind::Decrement => CounterAction::Decrement,
        ForgeActionKind::Reset => CounterAction::Reset,
    }
}

fn to_internal_mode(mode: OpenMode) -> app::OpenMode {
    match mode {
        OpenMode::Normal => app::OpenMode::Normal,
        OpenMode::RecoverCorruptData => app::OpenMode::RecoverCorruptData,
    }
}

/// One Rust app instance. Thread-safe; share it freely between host
/// components. Each instance owns its storage directory: do not open two
/// instances on the same directory at once.
#[derive(uniffi::Object)]
pub struct ForgeCounterApp {
    core: AppCore,
}

impl ForgeCounterApp {
    async fn open_core(storage_directory: String, mode: OpenMode) -> Result<Self, ForgeAppError> {
        let store = Box::new(JsonSnapshotStore::new(storage_directory));
        let core = AppCore::open(store, to_internal_mode(mode)).await?;
        Ok(Self { core })
    }
}

#[uniffi::export]
impl ForgeCounterApp {
    /// Opens the app synchronously, blocking the caller while state loads.
    /// Prefer [`open_counter_app`] from UI code.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Self, ForgeAppError> {
        block_on(Self::open_core(storage_directory, OpenMode::Normal))
    }

    // ---- Queries (never wait for disk) ----

    pub fn startup_kind(&self) -> Result<StartupKind, ForgeAppError> {
        Ok(to_public_startup(self.core.startup_kind()))
    }

    /// The last committed state. Answers immediately, even while a save is in
    /// progress.
    pub fn snapshot(&self) -> Result<CounterStateSnapshot, ForgeAppError> {
        Ok(app::to_ffi(&self.core.snapshot()))
    }

    /// The running long-running operation, if any.
    pub fn active_operation(&self) -> Option<OperationStatus> {
        self.core.active_operation().map(|op| status(&op))
    }

    // ---- Commands ----

    /// Applies an action on the instance's command thread. Resolves after the
    /// change has been saved (or rejected). Once sent it always runs to
    /// completion; dropping or cancelling the caller does not undo it.
    pub async fn apply_action(
        &self,
        action: ForgeActionKind,
    ) -> Result<ActionOutcome, ForgeAppError> {
        let snapshot = self.core.apply(to_core_action(action)).await?;
        Ok(ActionOutcome {
            snapshot: app::to_ffi(&snapshot),
        })
    }

    /// Synchronous form of [`Self::apply_action`]; blocks the caller until the
    /// change is saved. Kept for existing callers such as the Android host.
    pub fn apply(&self, action: ForgeActionKind) -> Result<ActionOutcome, ForgeAppError> {
        block_on(self.apply_action(action))
    }

    /// Starts adding `amount` as a long-running, cancellable operation (it
    /// stands in for work such as an import). Progress arrives as
    /// `OperationProgress` events; the whole amount is committed at the end or
    /// not at all. `step_delay_ms` is the simulated work per progress step
    /// (up to 20 steps).
    pub fn start_bulk_add(
        &self,
        amount: u32,
        step_delay_ms: u32,
    ) -> Result<Arc<CounterOperation>, ForgeAppError> {
        let operation = self
            .core
            .start_bulk_add(amount, Duration::from_millis(u64::from(step_delay_ms)))?;
        Ok(Arc::new(CounterOperation { operation }))
    }

    // ---- Events ----

    /// A new, independent event subscription. See
    /// [`CounterEventSubscription`] for its lifetime rules.
    pub fn subscribe(&self) -> Result<Arc<CounterEventSubscription>, ForgeAppError> {
        self.core.subscribe().map(Arc::new)
    }

    /// Number of live subscriptions (diagnostics and tests).
    pub fn subscription_count(&self) -> u32 {
        u32::try_from(self.core.subscriber_count()).unwrap_or(u32::MAX)
    }

    // ---- Lifecycle ----

    /// Shuts the instance down: new commands fail with `ShutDown`, the running
    /// operation is cancelled, commands already accepted finish, every event
    /// subscription ends. Resolves when all of that is done. Idempotent.
    /// Queries keep answering with the last committed state.
    pub async fn shutdown(&self) {
        self.core.shutdown().await;
    }

    pub fn is_shut_down(&self) -> bool {
        self.core.is_shut_down()
    }
}

fn status(operation: &Operation) -> OperationStatus {
    OperationStatus {
        operation_id: operation.id,
        completed: operation.completed(),
        total: operation.total,
        cancel_requested: operation.is_cancel_requested(),
        finished: operation.is_finished(),
    }
}

/// Handle to a long-running operation. Releasing it does not cancel the
/// operation; its outcome is also delivered as an `OperationFinished` event.
#[derive(uniffi::Object)]
pub struct CounterOperation {
    operation: Arc<Operation>,
}

#[uniffi::export]
impl CounterOperation {
    pub fn id(&self) -> u64 {
        self.operation.id
    }

    pub fn status(&self) -> OperationStatus {
        status(&self.operation)
    }

    /// Requests cancellation. Before the commit point the operation stops and
    /// `result()` fails with `Cancelled`; after it, this does nothing and
    /// `result()` returns the committed state.
    pub fn cancel(&self) {
        self.operation.cancel();
    }

    /// Resolves when the operation ends. Can be awaited any number of times.
    pub async fn result(&self) -> Result<CounterStateSnapshot, ForgeAppError> {
        self.operation.result().await
    }
}

/// Opens the app without blocking the caller: state is loaded on the
/// instance's command thread.
#[uniffi::export]
pub async fn open_counter_app(
    storage_directory: String,
    mode: OpenMode,
) -> Result<Arc<ForgeCounterApp>, ForgeAppError> {
    ForgeCounterApp::open_core(storage_directory, mode)
        .await
        .map(Arc::new)
}

/// Opens the app, replacing persisted data with a fresh counter **only** if it is
/// corrupt. Valid data is restored; data from a newer build
/// (`UnsupportedSchemaVersion`) or unreadable data is never overwritten.
/// Synchronous; prefer `open_counter_app(…, OpenMode::RecoverCorruptData)`.
#[uniffi::export]
pub fn recover_counter_app(storage_directory: String) -> Result<ForgeCounterApp, ForgeAppError> {
    block_on(ForgeCounterApp::open_core(
        storage_directory,
        OpenMode::RecoverCorruptData,
    ))
}

/// Spike / diagnostics: fixed greeting from Rust (used in toolchain validation).
#[uniffi::export]
pub fn forge_greeting() -> String {
    "Forge Mobile (Rust core)".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_and_async_apis_share_one_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let app = block_on(open_counter_app(path.clone(), OpenMode::Normal)).unwrap();
        app.apply(ForgeActionKind::Increment).unwrap();
        let outcome = block_on(app.apply_action(ForgeActionKind::Increment)).unwrap();
        assert_eq!(outcome.snapshot.count, 2);
        assert_eq!(app.snapshot().unwrap().count, 2);
        block_on(app.shutdown());
        assert!(app.is_shut_down());
        assert!(matches!(
            app.apply(ForgeActionKind::Increment),
            Err(ForgeAppError::ShutDown)
        ));
        let reopened = ForgeCounterApp::new(path).unwrap();
        assert_eq!(reopened.startup_kind().unwrap(), StartupKind::Restored);
        assert_eq!(reopened.snapshot().unwrap().count, 2);
    }

    #[test]
    fn operation_handle_reports_status_and_result() {
        let dir = tempfile::tempdir().unwrap();
        let app = ForgeCounterApp::new(dir.path().to_string_lossy().into_owned()).unwrap();
        let op = app.start_bulk_add(7, 0).unwrap();
        assert_eq!(block_on(op.result()).unwrap().count, 7);
        let status = op.status();
        assert!(status.finished);
        assert_eq!(status.completed, 7);
        assert_eq!(status.total, 7);
        assert!(app.active_operation().is_none());
    }

    #[test]
    fn subscriptions_are_counted_and_released() {
        let dir = tempfile::tempdir().unwrap();
        let app = ForgeCounterApp::new(dir.path().to_string_lossy().into_owned()).unwrap();
        let a = app.subscribe().unwrap();
        let b = app.subscribe().unwrap();
        assert_eq!(app.subscription_count(), 2);
        a.cancel();
        drop(b);
        assert_eq!(app.subscription_count(), 0);
    }
}
