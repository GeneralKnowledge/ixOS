use std::sync::Mutex;

use forge_core::{CounterAction, CounterSnapshot};
use forge_storage::JsonSnapshotStore;

use crate::errors::ForgeAppError;
use crate::session::{self, StartupKind as InternalStartupKind};

#[derive(uniffi::Record, Clone, Debug)]
pub struct CounterStateSnapshot {
    pub count: i64,
}

#[derive(uniffi::Enum, Clone, Debug)]
pub enum ForgeActionKind {
    Increment,
    Decrement,
    Reset,
}

#[derive(uniffi::Enum, Clone, Copy, Debug)]
pub enum StartupKind {
    FreshInstall,
    Restored,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct ActionOutcome {
    pub snapshot: CounterStateSnapshot,
}

fn to_ffi(snapshot: &CounterSnapshot) -> CounterStateSnapshot {
    CounterStateSnapshot {
        count: snapshot.count,
    }
}

fn to_public_startup(kind: InternalStartupKind) -> StartupKind {
    match kind {
        InternalStartupKind::FreshInstall => StartupKind::FreshInstall,
        InternalStartupKind::Restored => StartupKind::Restored,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, ForgeAppError> {
    mutex.lock().map_err(|_| ForgeAppError::InternalFailure)
}

#[derive(uniffi::Object)]
pub struct ForgeCounterApp {
    engine: Mutex<forge_core::CounterEngine>,
    store: JsonSnapshotStore,
    startup_kind: StartupKind,
}

#[uniffi::export]
impl ForgeCounterApp {
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Self, ForgeAppError> {
        let store = JsonSnapshotStore::new(storage_directory);
        let (engine, kind) = session::load_engine(&store)?;
        Ok(Self {
            engine: Mutex::new(engine),
            store,
            startup_kind: to_public_startup(kind),
        })
    }

    pub fn startup_kind(&self) -> Result<StartupKind, ForgeAppError> {
        Ok(self.startup_kind)
    }

    pub fn snapshot(&self) -> Result<CounterStateSnapshot, ForgeAppError> {
        let engine = lock(&self.engine)?;
        Ok(to_ffi(engine.snapshot()))
    }

    pub fn apply(&self, action: ForgeActionKind) -> Result<ActionOutcome, ForgeAppError> {
        let core_action = match action {
            ForgeActionKind::Increment => CounterAction::Increment,
            ForgeActionKind::Decrement => CounterAction::Decrement,
            ForgeActionKind::Reset => CounterAction::Reset,
        };
        let mut engine = lock(&self.engine)?;
        let snapshot = session::apply_action(&mut engine, &self.store, core_action)?;
        Ok(ActionOutcome {
            snapshot: to_ffi(&snapshot),
        })
    }
}

/// Opens the app, replacing persisted data with a fresh counter **only** if it is
/// corrupt. Valid data is restored; data from a newer build
/// (`UnsupportedSchemaVersion`) or unreadable data is never overwritten.
#[uniffi::export]
pub fn recover_counter_app(storage_directory: String) -> Result<ForgeCounterApp, ForgeAppError> {
    let store = JsonSnapshotStore::new(storage_directory);
    let (engine, kind) = session::recover_after_corruption(&store)?;
    Ok(ForgeCounterApp {
        engine: Mutex::new(engine),
        store,
        startup_kind: to_public_startup(kind),
    })
}

/// Spike / diagnostics: fixed greeting from Rust (used in toolchain validation).
#[uniffi::export]
pub fn forge_greeting() -> String {
    "Forge Mobile (Rust core)".to_string()
}
