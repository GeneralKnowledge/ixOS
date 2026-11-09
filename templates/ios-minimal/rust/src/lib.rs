//! The app's Rust core: a checklist that is saved on the phone.
//!
//! This is the minimal starter (`forge init --minimal`). It shows only what a
//! first app needs: a type the screen shows ([`Item`]), the rules for
//! changing it ([`AppCore`]), and saving it to a file. Everything marked
//! `#[uniffi::export]` can be called from Swift (`import ForgeFFI`);
//! `forge run ios` rebuilds this crate and regenerates that Swift API.
//!
//! The full starter (`forge init` without `--minimal`) adds events,
//! networking with cancellation, the app lifecycle, native services and
//! logging from Swift. See docs/first-app.md and docs/app-crate.md.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use forge_runtime::{background, lock};
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
}

impl From<StorageError> for AppError {
    fn from(err: StorageError) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

/// What is written to `items.json`.
#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    next_id: u64,
    items: Vec<Item>,
}

/// The app. Swift creates one (`AppCore(storageDirectory:)`) and keeps it.
#[derive(uniffi::Object)]
pub struct AppCore {
    saved: Arc<Mutex<Saved>>,
    file: Arc<JsonFile<Saved>>,
}

#[uniffi::export]
impl AppCore {
    /// Opens the list saved in `storage_directory`, or starts an empty one.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, AppError> {
        // Rust's log goes to the phone's system log (`forge device logs`).
        forge_runtime::logging::init();
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
        }))
    }

    /// The list, in the order items were added. Quick: it reads memory only.
    pub fn items(&self) -> Vec<Item> {
        lock(&self.saved).items.clone()
    }

    /// Adds an item and saves. Returns the new list.
    pub async fn add_item(&self, title: String) -> Result<Vec<Item>, AppError> {
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
            Ok(())
        })
        .await
    }

    /// Ticks an item off, or back on. Returns the new list.
    pub async fn toggle_item(&self, id: u64) -> Result<Vec<Item>, AppError> {
        self.change(move |saved| {
            let item = find(saved, id)?;
            item.done = !item.done;
            Ok(())
        })
        .await
    }

    /// Removes an item. Returns the new list.
    pub async fn delete_item(&self, id: u64) -> Result<Vec<Item>, AppError> {
        self.change(move |saved| {
            find(saved, id)?;
            saved.items.retain(|i| i.id != id);
            Ok(())
        })
        .await
    }
}

impl AppCore {
    /// Applies `edit` to a copy of the list, saves the copy, and only then
    /// keeps it, so a failed save changes nothing. Saving touches the disk,
    /// so it runs on a background thread and Swift `await`s it.
    async fn change<F>(&self, edit: F) -> Result<Vec<Item>, AppError>
    where
        F: FnOnce(&mut Saved) -> Result<(), AppError> + Send + 'static,
    {
        let saved = Arc::clone(&self.saved);
        let file = Arc::clone(&self.file);
        background("save", move || {
            let mut current = lock(&saved);
            let mut next = current.clone();
            edit(&mut next)?;
            file.save(&next)?;
            *current = next;
            Ok(current.items.clone())
        })
        .await
    }
}

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
        assert_eq!(open(&dir).items(), items);
    }
}
