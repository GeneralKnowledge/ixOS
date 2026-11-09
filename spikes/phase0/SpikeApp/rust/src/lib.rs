//! Phase 0 spike: an app crate whose exported API uses the shared runtime
//! contract (`forge_app::AppPhase`, `RuntimeEvents`, `Cancellation`,
//! `ErrorKind`) as UniFFI external types. Everything else is the app's own.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use forge_app::{AppPhase, Cancellation, ErrorKind, Runtime, RuntimeEvents};
use forge_runtime::{background, background_cancellable, lock};
use forge_storage::{JsonFile, StorageError};
use serde::{Deserialize, Serialize};

uniffi::setup_scaffolding!("forge_ffi");

const SAVE_VERSION: u32 = 1;

#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum AppError {
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error("no item with id {id}")]
    NotFound { id: u64 },
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// Stopped by the runtime: cancelled, interrupted by the background, or
    /// shut down. `kind` says which.
    #[error("stopped ({kind:?})")]
    Stopped { kind: ErrorKind },
}

impl From<StorageError> for AppError {
    fn from(err: StorageError) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

/// The framework kind of an app error, so ForgeKit can present any app's
/// errors by kind.
#[uniffi::export]
pub fn error_kind(error: AppError) -> ErrorKind {
    match error {
        AppError::InvalidInput { .. } => ErrorKind::Invalid,
        AppError::NotFound { .. } => ErrorKind::NotFound,
        AppError::Storage { .. } => ErrorKind::Internal,
        AppError::Stopped { kind } => kind,
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    next_id: u64,
    items: Vec<Item>,
}

#[derive(uniffi::Object)]
pub struct AppCore {
    saved: Arc<Mutex<Saved>>,
    file: Arc<JsonFile<Saved>>,
    runtime: Arc<Runtime>,
}

#[uniffi::export]
impl AppCore {
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, AppError> {
        if storage_directory.is_empty() {
            return Err(AppError::InvalidInput {
                reason: "storage directory must not be empty".into(),
            });
        }
        let file = JsonFile::new(
            PathBuf::from(storage_directory).join("items.json"),
            SAVE_VERSION,
        );
        let saved = file.load()?.unwrap_or_default();
        Ok(Arc::new(Self {
            saved: Arc::new(Mutex::new(saved)),
            file: Arc::new(file),
            runtime: Arc::new(Runtime::new()),
        }))
    }

    pub fn items(&self) -> Vec<Item> {
        lock(&self.saved).items.clone()
    }

    pub fn revision(&self) -> u64 {
        self.runtime.revision()
    }

    /// Adds an item; returns its id. `Changed` follows.
    pub async fn add_item(&self, title: String) -> Result<u64, AppError> {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err(AppError::InvalidInput {
                reason: "an item needs a title".into(),
            });
        }
        self.change(move |saved| {
            saved.next_id += 1;
            saved.items.push(Item {
                id: saved.next_id,
                title,
                done: false,
            });
            Ok(saved.next_id)
        })
        .await
    }

    pub async fn toggle_item(&self, id: u64) -> Result<(), AppError> {
        self.change(move |saved| {
            let item = saved
                .items
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or(AppError::NotFound { id })?;
            item.done = !item.done;
            Ok(())
        })
        .await
    }

    /// Counts to `to`, slowly, as on-screen work: stopped by `cancellation`,
    /// by the app going to the background, or by shutdown.
    pub async fn slow_count(
        &self,
        to: u32,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<u32, AppError> {
        let work = self.runtime.foreground_work(cancellation.as_deref());
        let cancel = work.cancel.clone();
        background_cancellable("slow-count", cancel, move |cancel| {
            for i in 0..to {
                if cancel.is_cancelled() {
                    return Err(AppError::Stopped {
                        kind: work.stop_kind(),
                    });
                }
                std::thread::sleep(Duration::from_millis(20));
                let _ = i;
            }
            Ok(to)
        })
        .await
    }

    // The runtime contract, delegated. In phase 1 a macro would emit these.

    pub fn events(&self) -> Option<Arc<RuntimeEvents>> {
        self.runtime.subscribe()
    }

    pub fn set_phase(&self, phase: AppPhase) {
        self.runtime.set_phase(phase)
    }

    pub fn phase(&self) -> AppPhase {
        self.runtime.phase()
    }

    pub fn shutdown(&self) {
        self.runtime.shutdown()
    }
}

impl AppCore {
    async fn change<T, F>(&self, edit: F) -> Result<T, AppError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Saved) -> Result<T, AppError> + Send + 'static,
    {
        if self.runtime.is_shut_down() {
            return Err(AppError::Stopped {
                kind: ErrorKind::ShutDown,
            });
        }
        let saved = Arc::clone(&self.saved);
        let file = Arc::clone(&self.file);
        let runtime = Arc::clone(&self.runtime);
        background("change", move || {
            let mut current = lock(&saved);
            let mut next = current.clone();
            let out = edit(&mut next)?;
            file.save(&next)?;
            *current = next;
            runtime.committed();
            Ok(out)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_app::RuntimeEvent;
    use forge_runtime::block_on;

    fn open(dir: &tempfile::TempDir) -> Arc<AppCore> {
        AppCore::new(dir.path().to_string_lossy().into_owned()).unwrap()
    }

    #[test]
    fn commands_commit_then_publish_a_revision() {
        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir);
        let events = core.events().unwrap();
        let id = block_on(core.add_item("Milk".into())).unwrap();
        assert_eq!(
            block_on(events.next()),
            Some(RuntimeEvent::Changed { revision: 1 })
        );
        block_on(core.toggle_item(id)).unwrap();
        assert_eq!(core.revision(), 2);
        assert!(open(&dir).items()[0].done, "saved before the event");
    }

    #[test]
    fn errors_map_to_framework_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir);
        let err = block_on(core.toggle_item(9)).unwrap_err();
        assert_eq!(error_kind(err), ErrorKind::NotFound);
        core.shutdown();
        let err = block_on(core.add_item("x".into())).unwrap_err();
        assert_eq!(error_kind(err), ErrorKind::ShutDown);
    }

    #[test]
    fn background_and_host_cancellation_stop_on_screen_work() {
        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir);
        core.set_phase(AppPhase::Active);

        let c = Cancellation::new();
        let c2 = Arc::clone(&c);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            c2.cancel();
        });
        let err = block_on(core.slow_count(1000, Some(c))).unwrap_err();
        assert_eq!(error_kind(err), ErrorKind::Cancelled);

        let core2 = Arc::clone(&core);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            core2.set_phase(AppPhase::Background);
        });
        let err = block_on(core.slow_count(1000, None)).unwrap_err();
        assert_eq!(error_kind(err), ErrorKind::Interrupted);
    }
}
