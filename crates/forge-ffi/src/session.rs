use forge_core::{CounterAction, CounterEngine, CounterSnapshot};
use forge_storage::{SnapshotStore, StorageError};

use crate::errors::ForgeAppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupKind {
    FreshInstall,
    Restored,
}

pub fn load_engine(
    store: &(impl SnapshotStore<Snapshot = CounterSnapshot> + ?Sized),
) -> Result<(CounterEngine, StartupKind), ForgeAppError> {
    match store.load()? {
        None => Ok((CounterEngine::new(), StartupKind::FreshInstall)),
        Some(snapshot) => {
            let engine = CounterEngine::from_snapshot(snapshot).map_err(|err| {
                ForgeAppError::CorruptPersistence {
                    reason: err.to_string(),
                }
            })?;
            Ok((engine, StartupKind::Restored))
        }
    }
}

/// Valid domain transitions commit only after a successful save.
pub fn apply_action(
    engine: &mut CounterEngine,
    store: &(impl SnapshotStore<Snapshot = CounterSnapshot> + ?Sized),
    action: CounterAction,
) -> Result<CounterSnapshot, ForgeAppError> {
    let mut candidate = engine.clone();
    let next = candidate.apply(action)?;
    store.save(candidate.snapshot())?;
    *engine = candidate;
    Ok(next)
}

/// Starts fresh **only** when the stored data is corrupt (or absent).
///
/// Valid data is restored rather than wiped, and data written by a newer build,
/// or data that could not be read, is left untouched and reported as an error.
/// The fresh state is written with the store's atomic replace, so there is
/// never a moment with no snapshot file.
pub fn recover_after_corruption(
    store: &(impl SnapshotStore<Snapshot = CounterSnapshot> + ?Sized),
) -> Result<(CounterEngine, StartupKind), ForgeAppError> {
    match store.load() {
        Ok(Some(snapshot)) => {
            if let Ok(engine) = CounterEngine::from_snapshot(snapshot) {
                return Ok((engine, StartupKind::Restored));
            }
        }
        Ok(None) | Err(StorageError::CorruptData { .. }) => {}
        Err(other) => return Err(other.into()),
    }
    let engine = CounterEngine::new();
    store.save(engine.snapshot())?;
    Ok((engine, StartupKind::FreshInstall))
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::CounterSnapshot;
    use forge_storage::JsonSnapshotStore;
    use std::cell::RefCell;
    use std::fs;

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
                return Err(StorageError::ReadFailed {
                    reason: "test".into(),
                });
            }
            Ok(self.snapshot.borrow().clone())
        }

        fn save(&self, snapshot: &Self::Snapshot) -> Result<(), StorageError> {
            if *self.fail_save.borrow() {
                return Err(StorageError::WriteFailed {
                    reason: "test".into(),
                });
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
        assert!(matches!(err, ForgeAppError::PersistenceFailed { .. }));
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
        *store.load_returns.borrow_mut() = Some(Err(StorageError::CorruptData {
            reason: "bad".into(),
        }));
        let err = load_engine(&store).unwrap_err();
        assert!(matches!(err, ForgeAppError::CorruptPersistence { .. }));
    }

    #[test]
    fn read_failure_is_explicit() {
        let store = MemoryStore::new();
        *store.fail_load.borrow_mut() = true;
        let err = load_engine(&store).unwrap_err();
        assert!(matches!(err, ForgeAppError::PersistenceReadFailed { .. }));
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

    fn write_newer_snapshot(json: &JsonSnapshotStore) -> String {
        let contents = format!(
            r#"{{"schema_version": {}, "count": 7}}"#,
            forge_storage::SCHEMA_VERSION + 1
        );
        fs::write(json.path(), &contents).unwrap();
        contents
    }

    #[test]
    fn newer_schema_is_unsupported_not_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let json = JsonSnapshotStore::new(dir.path());
        write_newer_snapshot(&json);
        let err = load_engine(&json).unwrap_err();
        assert!(
            matches!(err, ForgeAppError::UnsupportedSchemaVersion { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn recovery_refuses_to_overwrite_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let json = JsonSnapshotStore::new(dir.path());
        let contents = write_newer_snapshot(&json);
        let err = recover_after_corruption(&json).unwrap_err();
        assert!(matches!(
            err,
            ForgeAppError::UnsupportedSchemaVersion { .. }
        ));
        assert_eq!(fs::read_to_string(json.path()).unwrap(), contents);
    }

    #[test]
    fn recovery_keeps_valid_data() {
        let dir = tempfile::tempdir().unwrap();
        let json = JsonSnapshotStore::new(dir.path());
        json.save(&CounterSnapshot { count: 4 }).unwrap();
        let (engine, kind) = recover_after_corruption(&json).unwrap();
        assert_eq!(kind, StartupKind::Restored);
        assert_eq!(engine.snapshot().count, 4);
    }

    #[test]
    fn recovery_does_not_overwrite_after_read_failure() {
        let store = MemoryStore::new();
        *store.snapshot.borrow_mut() = Some(CounterSnapshot { count: 9 });
        *store.fail_load.borrow_mut() = true;
        let err = recover_after_corruption(&store).unwrap_err();
        assert!(matches!(err, ForgeAppError::PersistenceReadFailed { .. }));
        assert_eq!(store.snapshot.borrow().as_ref().unwrap().count, 9);
    }
}
