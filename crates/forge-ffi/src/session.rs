use std::fs;

use forge_core::{CounterAction, CounterEngine, CounterSnapshot};
use forge_storage::{JsonSnapshotStore, SnapshotStore, StorageError};

use crate::errors::ForgeAppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupKind {
    FreshInstall,
    Restored,
}

pub fn load_engine(
    store: &impl SnapshotStore<Snapshot = CounterSnapshot>,
) -> Result<(CounterEngine, StartupKind), ForgeAppError> {
    match store.load() {
        Ok(None) => Ok((CounterEngine::new(), StartupKind::FreshInstall)),
        Ok(Some(snapshot)) => {
            let engine = CounterEngine::from_snapshot(snapshot)
                .map_err(|_| ForgeAppError::CorruptPersistence)?;
            Ok((engine, StartupKind::Restored))
        }
        Err(StorageError::CorruptData) => Err(ForgeAppError::CorruptPersistence),
        Err(StorageError::ReadFailed) => Err(ForgeAppError::PersistenceReadFailed),
        Err(StorageError::MissingDirectory) => Err(ForgeAppError::StorageDirectoryUnavailable),
        Err(StorageError::WriteFailed) => Err(ForgeAppError::PersistenceFailed),
    }
}

/// Valid domain transitions commit only after a successful save.
pub fn apply_action(
    engine: &mut CounterEngine,
    store: &impl SnapshotStore<Snapshot = CounterSnapshot>,
    action: CounterAction,
) -> Result<CounterSnapshot, ForgeAppError> {
    let mut candidate = engine.clone();
    let next = candidate.apply(action)?;
    store.save(candidate.snapshot())?;
    *engine = candidate;
    Ok(next)
}

pub fn recover_after_corruption(
    store: &JsonSnapshotStore,
) -> Result<(CounterEngine, StartupKind), ForgeAppError> {
    let path = store.path();
    if path.exists() {
        fs::remove_file(&path).map_err(|_| ForgeAppError::PersistenceFailed)?;
    }
    let engine = CounterEngine::new();
    store.save(engine.snapshot())?;
    Ok((engine, StartupKind::FreshInstall))
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::CounterSnapshot;
    use std::cell::RefCell;

    struct MemoryStore {
        snapshot: RefCell<Option<CounterSnapshot>>,
        fail_save: RefCell<bool>,
        fail_load: RefCell<bool>,
        load_returns: RefCell<Option<Result<Option<CounterSnapshot>, StorageError>>>,
    }

    impl MemoryStore {
        fn new() -> Self {
            Self {
                snapshot: RefCell::new(None),
                fail_save: RefCell::new(false),
                fail_load: RefCell::new(false),
                load_returns: RefCell::new(None),
            }
        }
    }

    impl SnapshotStore for MemoryStore {
        type Snapshot = CounterSnapshot;

        fn load(&self) -> Result<Option<Self::Snapshot>, StorageError> {
            if let Some(custom) = self.load_returns.borrow_mut().take() {
                return custom;
            }
            if *self.fail_load.borrow() {
                return Err(StorageError::ReadFailed);
            }
            Ok(self.snapshot.borrow().clone())
        }

        fn save(&self, snapshot: &Self::Snapshot) -> Result<(), StorageError> {
            if *self.fail_save.borrow() {
                return Err(StorageError::WriteFailed);
            }
            *self.snapshot.borrow_mut() = Some(snapshot.clone());
            Ok(())
        }
    }

    #[test]
    fn successful_action_updates_state_and_persistence() {
        let store = MemoryStore::new();
        let mut engine = CounterEngine::new();
        let next = apply_action(&mut engine, &store, CounterAction::Increment).unwrap();
        assert_eq!(next.count, 1);
        assert_eq!(engine.snapshot().count, 1);
        assert_eq!(store.snapshot.borrow().as_ref().unwrap().count, 1);
    }

    #[test]
    fn rejected_domain_action_leaves_state_and_store_unchanged() {
        let store = MemoryStore::new();
        *store.snapshot.borrow_mut() = Some(CounterSnapshot { count: 0 });
        let mut engine = CounterEngine::new();
        let err = apply_action(&mut engine, &store, CounterAction::Decrement).unwrap_err();
        assert_eq!(err, ForgeAppError::WouldUnderflow);
        assert_eq!(engine.snapshot().count, 0);
        assert_eq!(store.snapshot.borrow().as_ref().unwrap().count, 0);
    }

    #[test]
    fn failed_save_does_not_commit_memory_state() {
        let store = MemoryStore::new();
        *store.snapshot.borrow_mut() = Some(CounterSnapshot { count: 2 });
        let mut engine = CounterEngine::from_snapshot(CounterSnapshot { count: 2 }).unwrap();
        *store.fail_save.borrow_mut() = true;
        let err = apply_action(&mut engine, &store, CounterAction::Increment).unwrap_err();
        assert_eq!(err, ForgeAppError::PersistenceFailed);
        assert_eq!(engine.snapshot().count, 2);
        assert_eq!(store.snapshot.borrow().as_ref().unwrap().count, 2);
    }

    #[test]
    fn snapshot_reflects_last_committed_state() {
        let store = MemoryStore::new();
        let mut engine = CounterEngine::new();
        apply_action(&mut engine, &store, CounterAction::Increment).unwrap();
        apply_action(&mut engine, &store, CounterAction::Increment).unwrap();
        *store.fail_save.borrow_mut() = true;
        let _ = apply_action(&mut engine, &store, CounterAction::Increment);
        assert_eq!(engine.snapshot().count, 2);
    }

    #[test]
    fn reload_after_commit_restores_state() {
        let dir = tempfile::tempdir().unwrap();
        let json = JsonSnapshotStore::new(dir.path());
        let mut engine = CounterEngine::new();
        apply_action(&mut engine, &json, CounterAction::Increment).unwrap();
        let (loaded, kind) = load_engine(&json).unwrap();
        assert_eq!(kind, StartupKind::Restored);
        assert_eq!(loaded.snapshot().count, 1);
    }

    #[test]
    fn fresh_install_when_no_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let json = JsonSnapshotStore::new(dir.path());
        let (engine, kind) = load_engine(&json).unwrap();
        assert_eq!(kind, StartupKind::FreshInstall);
        assert_eq!(engine.snapshot().count, 0);
    }

    #[test]
    fn corrupt_snapshot_is_error_not_silent_reset() {
        let store = MemoryStore::new();
        *store.load_returns.borrow_mut() = Some(Err(StorageError::CorruptData));
        let err = load_engine(&store).unwrap_err();
        assert_eq!(err, ForgeAppError::CorruptPersistence);
    }

    #[test]
    fn read_failure_is_explicit() {
        let store = MemoryStore::new();
        *store.fail_load.borrow_mut() = true;
        let err = load_engine(&store).unwrap_err();
        assert_eq!(err, ForgeAppError::PersistenceReadFailed);
    }

    #[test]
    fn recovery_replaces_corrupt_file_with_fresh_state() {
        let dir = tempfile::tempdir().unwrap();
        let json = JsonSnapshotStore::new(dir.path());
        fs::write(json.path(), "{bad").unwrap();
        let (engine, kind) = recover_after_corruption(&json).unwrap();
        assert_eq!(kind, StartupKind::FreshInstall);
        assert_eq!(engine.snapshot().count, 0);
        let (reloaded, _) = load_engine(&json).unwrap();
        assert_eq!(reloaded.snapshot().count, 0);
    }

    #[test]
    fn missing_directory_on_load_surfaces_error() {
        let store = MemoryStore::new();
        *store.load_returns.borrow_mut() = Some(Err(StorageError::MissingDirectory));
        let err = load_engine(&store).unwrap_err();
        assert_eq!(err, ForgeAppError::StorageDirectoryUnavailable);
    }
}
