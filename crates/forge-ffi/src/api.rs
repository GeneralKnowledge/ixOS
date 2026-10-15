use std::sync::Mutex;

use forge_core::{CounterAction, CounterEngine, CounterError, CounterSnapshot};
use forge_storage::{JsonSnapshotStore, SnapshotStore, StorageError};

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

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum ForgeAppError {
    #[error("count cannot go below zero")]
    WouldUnderflow,
    #[error("count cannot exceed maximum")]
    WouldOverflow,
    #[error("persistence failed")]
    PersistenceFailed,
    #[error("stored data is invalid")]
    CorruptPersistence,
}

impl From<CounterError> for ForgeAppError {
    fn from(value: CounterError) -> Self {
        match value {
            CounterError::WouldUnderflow { .. } => ForgeAppError::WouldUnderflow,
            CounterError::WouldOverflow { .. } => ForgeAppError::WouldOverflow,
        }
    }
}

impl From<StorageError> for ForgeAppError {
    fn from(value: StorageError) -> Self {
        match value {
            StorageError::CorruptData => ForgeAppError::CorruptPersistence,
            StorageError::MissingDirectory
            | StorageError::ReadFailed
            | StorageError::WriteFailed => ForgeAppError::PersistenceFailed,
        }
    }
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct ActionOutcome {
    pub snapshot: CounterStateSnapshot,
    pub persistence_ok: bool,
}

fn to_ffi(snapshot: &CounterSnapshot) -> CounterStateSnapshot {
    CounterStateSnapshot {
        count: snapshot.count,
    }
}

#[derive(uniffi::Object)]
pub struct ForgeCounterApp {
    engine: Mutex<CounterEngine>,
    store: JsonSnapshotStore,
    last_persistence_ok: Mutex<bool>,
}

#[uniffi::export]
impl ForgeCounterApp {
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Self, ForgeAppError> {
        let store = JsonSnapshotStore::new(storage_directory);
        let engine = match store.load() {
            Ok(Some(snapshot)) => CounterEngine::from_snapshot(snapshot)
                .map_err(|_| ForgeAppError::CorruptPersistence)?,
            Ok(None) => CounterEngine::new(),
            Err(StorageError::CorruptData) => return Err(ForgeAppError::CorruptPersistence),
            Err(_) => CounterEngine::new(),
        };
        Ok(Self {
            engine: Mutex::new(engine),
            store,
            last_persistence_ok: Mutex::new(true),
        })
    }

    pub fn snapshot(&self) -> CounterStateSnapshot {
        let engine = self.engine.lock().expect("counter lock poisoned");
        to_ffi(engine.snapshot())
    }

    pub fn last_persistence_ok(&self) -> bool {
        *self
            .last_persistence_ok
            .lock()
            .expect("persistence flag lock poisoned")
    }

    pub fn apply(&self, action: ForgeActionKind) -> Result<ActionOutcome, ForgeAppError> {
        let core_action = match action {
            ForgeActionKind::Increment => CounterAction::Increment,
            ForgeActionKind::Decrement => CounterAction::Decrement,
            ForgeActionKind::Reset => CounterAction::Reset,
        };
        let mut engine = self.engine.lock().expect("counter lock poisoned");
        let snapshot = engine.apply(core_action)?;
        let persistence_ok = match self.store.save(engine.snapshot()) {
            Ok(()) => true,
            Err(e) => {
                *self
                    .last_persistence_ok
                    .lock()
                    .expect("persistence flag lock poisoned") = false;
                return Err(e.into());
            }
        };
        *self
            .last_persistence_ok
            .lock()
            .expect("persistence flag lock poisoned") = persistence_ok;
        Ok(ActionOutcome {
            snapshot: to_ffi(&snapshot),
            persistence_ok,
        })
    }
}

/// Spike / diagnostics: fixed greeting from Rust (used in toolchain validation).
#[uniffi::export]
pub fn forge_greeting() -> String {
    "Forge Mobile (Rust core)".to_string()
}
