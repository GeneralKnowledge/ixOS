//! The app's Rust core: a checklist that is saved on the phone.
//!
//! This is the minimal starter (`forge init --minimal`). It shows only what a
//! first app needs: a type the screen shows ([`Item`]), the rules for
//! changing it ([`AppCore`]), and saving it to a file. Everything marked
//! `#[uniffi::export]` can be called from Swift (`import ForgeFFI`);
//! `forge run ios` rebuilds this crate and regenerates that Swift API.
//!
//! It runs on the ixOS application runtime (`forge_app`): each change is a
//! **command** that runs on the runtime's command thread, one at a time, and
//! saves before it returns; then the runtime tells the Swift host
//! (ForgeKit's `ForgeHost`), which re-reads. The lifecycle and shutdown come
//! from `forge_app::export_runtime!`.
//!
//! The full starter (`forge init` without `--minimal`) adds networking with
//! cancellation, native services and logging from Swift. See
//! docs/first-app.md and docs/app-crate.md.

use std::sync::Arc;

use forge_app::{DataDirectory, ErrorKind, OpenError, Published, Runtime, Stopped};
use forge_storage::{JsonFile, StorageError};
use serde::{Deserialize, Serialize};

uniffi::setup_scaffolding!("forge_ffi");

/// Version of `items.json`. Bump it (and migrate) when [`Saved`] changes shape.
const SAVE_VERSION: u32 = 1;

/// One thing on the list. A `uniffi::Record` arrives in Swift as a struct
/// with the same fields (`item.title`, `item.done`).
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

/// What can go wrong. Arrives in Swift as a thrown `AppError`.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum AppError {
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error("no item with id {id}")]
    NotFound { id: u64 },
    /// The list could not be read or saved.
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// Another copy of the app has the list open (one core per data folder).
    #[error("the list is open in another copy of the app")]
    InUse,
    /// The app is shutting down and takes no more changes.
    #[error("the app has been shut down")]
    ShutDown,
}

/// How the runtime refuses a command after shutdown.
impl From<Stopped> for AppError {
    fn from(_: Stopped) -> Self {
        Self::ShutDown
    }
}

impl From<StorageError> for AppError {
    fn from(err: StorageError) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

impl From<OpenError> for AppError {
    fn from(err: OpenError) -> Self {
        match err {
            OpenError::InUse { .. } => Self::InUse,
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

/// The framework kind of an error, so ForgeKit can present any of them.
#[uniffi::export]
pub fn app_error_kind(error: AppError) -> ErrorKind {
    match error {
        AppError::InvalidInput { .. } => ErrorKind::Invalid,
        AppError::NotFound { .. } => ErrorKind::NotFound,
        AppError::Storage { .. } => ErrorKind::Internal,
        AppError::InUse => ErrorKind::Unavailable,
        AppError::ShutDown => ErrorKind::ShutDown,
    }
}

/// What is written to `items.json`.
#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    next_id: u64,
    items: Vec<Item>,
}

/// What the command thread owns: the list and the file it is saved in.
struct State {
    saved: Saved,
    file: JsonFile<Saved>,
    published: Arc<Published<Vec<Item>>>,
}

impl State {
    /// Applies `edit` to a copy of the list, saves the copy, and only then
    /// keeps and publishes it, so a failed save changes nothing.
    fn change(
        &mut self,
        edit: impl FnOnce(&mut Saved) -> Result<(), AppError>,
    ) -> Result<Vec<Item>, AppError> {
        let mut next = self.saved.clone();
        edit(&mut next)?;
        self.file.save(&next)?;
        self.saved = next;
        self.published.set(self.saved.items.clone());
        Ok(self.saved.items.clone())
    }
}

/// The app. ForgeKit's `ForgeHost` opens one per process and keeps it.
#[derive(uniffi::Object)]
pub struct AppCore {
    rt: Runtime<State>,
    items: Arc<Published<Vec<Item>>>,
}

#[uniffi::export]
impl AppCore {
    /// Opens the list saved in `storage_directory`, or starts an empty one.
    /// Fails with `InUse` if another core has that folder open.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, AppError> {
        // Rust's log goes to the phone's system log (`forge device logs`).
        forge_runtime::logging::init();
        if storage_directory.is_empty() {
            return Err(AppError::InvalidInput {
                reason: "storage directory must not be empty".into(),
            });
        }
        let directory = DataDirectory::open(&storage_directory)?;
        let file = JsonFile::new(directory.path().join("items.json"), SAVE_VERSION);
        let saved: Saved = file.load()?.unwrap_or_default();
        let items = Arc::new(Published::new(saved.items.clone()));
        let state = State {
            saved,
            file,
            published: Arc::clone(&items),
        };
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("app-core", directory, state),
            items,
        }))
    }

    /// The list, in the order items were added. Quick: it never waits for a
    /// command.
    pub fn items(&self) -> Vec<Item> {
        self.items.get().as_ref().clone()
    }

    /// Adds an item and saves. Returns the new list.
    pub async fn add_item(&self, title: String) -> Result<Vec<Item>, AppError> {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err(AppError::InvalidInput {
                reason: "an item needs a title".into(),
            });
        }
        self.rt
            .command("add-item", move |state| {
                state.change(|saved| {
                    saved.next_id += 1;
                    saved.items.push(Item {
                        id: saved.next_id,
                        title,
                        done: false,
                    });
                    Ok(())
                })
            })
            .await
    }

    /// Ticks an item off, or back on. Returns the new list.
    pub async fn toggle_item(&self, id: u64) -> Result<Vec<Item>, AppError> {
        self.rt
            .command("toggle-item", move |state| {
                state.change(|saved| {
                    let item = find(saved, id)?;
                    item.done = !item.done;
                    Ok(())
                })
            })
            .await
    }

    /// Removes an item. Returns the new list.
    pub async fn delete_item(&self, id: u64) -> Result<Vec<Item>, AppError> {
        self.rt
            .command("delete-item", move |state| {
                state.change(|saved| {
                    find(saved, id)?;
                    saved.items.retain(|i| i.id != id);
                    Ok(())
                })
            })
            .await
    }
}

// events(), set_phase(), phase(), revision() and shutdown(), for ForgeKit.
forge_app::export_runtime!(AppCore, rt);

fn find(saved: &mut Saved, id: u64) -> Result<&mut Item, AppError> {
    saved
        .items
        .iter_mut()
        .find(|i| i.id == id)
        .ok_or(AppError::NotFound { id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_runtime::block_on;

    fn open(dir: &tempfile::TempDir) -> Arc<AppCore> {
        AppCore::new(dir.path().to_string_lossy().into_owned()).unwrap()
    }

    #[test]
    fn items_are_saved_and_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir);
        assert!(core.items().is_empty());
        block_on(core.add_item("Milk".into())).unwrap();
        let items = block_on(core.add_item("  Bread ".into())).unwrap();
        assert_eq!(
            items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
            ["Milk", "Bread"]
        );
        block_on(core.toggle_item(items[0].id)).unwrap();
        assert_eq!(core.revision(), 3);
        drop(core);

        let reopened = open(&dir);
        let items = reopened.items();
        assert_eq!(items.len(), 2);
        assert!(items[0].done);
        assert!(!items[1].done);
    }

    #[test]
    fn blank_titles_and_unknown_ids_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir);
        assert!(matches!(
            block_on(core.add_item("   ".into())),
            Err(AppError::InvalidInput { .. })
        ));
        assert_eq!(
            block_on(core.toggle_item(7)),
            Err(AppError::NotFound { id: 7 })
        );
        assert_eq!(
            block_on(core.delete_item(7)),
            Err(AppError::NotFound { id: 7 })
        );
        // A refused change is not a change.
        assert_eq!(core.revision(), 0);
    }

    #[test]
    fn delete_removes_only_that_item() {
        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir);
        block_on(core.add_item("a".into())).unwrap();
        let items = block_on(core.add_item("b".into())).unwrap();
        let items = block_on(core.delete_item(items[0].id)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "b");
        drop(core);
        assert_eq!(open(&dir).items(), items);
    }

    #[test]
    fn one_core_per_folder_and_nothing_after_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir);
        assert_eq!(
            AppCore::new(dir.path().to_string_lossy().into_owned()).err(),
            Some(AppError::InUse)
        );
        core.shutdown();
        assert_eq!(
            block_on(core.add_item("late".into())),
            Err(AppError::ShutDown)
        );
    }
}
