//! Checklist: a list of things to do, kept on the phone.
//!
//! The app's whole Rust side. What a checklist is lives in [`checklist`];
//! this file is the API the SwiftUI app calls, built on the application
//! runtime (`forge_app`):
//!
//! - **Commands** (`add`, `toggle`, `rename`, `remove`, `clear_done`) run one
//!   at a time on the runtime's command thread. Each saves before it
//!   returns; the runtime then raises the revision and announces `Changed`.
//! - **Queries** (`items`, `summary`) read a [`Published`] snapshot and
//!   never wait for a command.
//! - **Work**: `search` is on-screen work (stopped by the background, by
//!   the host's cancellation, by shutdown); `export_text` writes a file to
//!   share and may finish in the background.
//! - **The runtime contract** (`events`, `set_phase`, `phase`, `revision`,
//!   `shutdown`) comes from `forge_app::export_runtime!`.

mod checklist;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use forge_app::{Cancellation, DataDirectory, ErrorKind, OpenError, Published, Runtime, Stopped};
use forge_storage::{JsonFile, StorageError};

use checklist::{Checklist, Saved};
pub use checklist::{Item, Summary, MAX_TITLE_CHARS};

uniffi::setup_scaffolding!("forge_ffi");

const SAVE_FILE: &str = "checklist.json";
const SAVE_VERSION: u32 = 1;
const EXPORT_FILE: &str = "Checklist.txt";

/// Everything that can go wrong, in the app's words. `checklist_error_kind`
/// maps each case to the framework's [`ErrorKind`], so ForgeKit can present
/// any app's errors the same way.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum ChecklistError {
    #[error("an item needs a title")]
    EmptyTitle,
    #[error("titles are at most {max} characters")]
    TitleTooLong { max: u32 },
    #[error("no item with id {id}")]
    NotFound { id: u64 },
    #[error("the checklist is open in another copy of the app")]
    InUse,
    #[error("the saved checklist can't be read: {reason}")]
    Corrupt { reason: String },
    #[error("the checklist was saved by a newer version of the app")]
    NewerData,
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// Stopped by the runtime: cancelled, interrupted by the background,
    /// or shut down. `kind` says which.
    #[error("stopped ({kind:?})")]
    Stopped { kind: ErrorKind },
}

impl From<Stopped> for ChecklistError {
    fn from(stopped: Stopped) -> Self {
        Self::Stopped { kind: stopped.kind }
    }
}

impl From<StorageError> for ChecklistError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::CorruptData { reason } => Self::Corrupt { reason },
            StorageError::UnsupportedSchemaVersion { .. } => Self::NewerData,
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

impl From<OpenError> for ChecklistError {
    fn from(err: OpenError) -> Self {
        match err {
            OpenError::InUse { .. } => Self::InUse,
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

/// The framework kind of a Checklist error.
#[uniffi::export]
pub fn checklist_error_kind(error: ChecklistError) -> ErrorKind {
    match error {
        ChecklistError::EmptyTitle | ChecklistError::TitleTooLong { .. } => ErrorKind::Invalid,
        ChecklistError::NotFound { .. } => ErrorKind::NotFound,
        ChecklistError::InUse => ErrorKind::Unavailable,
        ChecklistError::Corrupt { .. } => ErrorKind::Corrupt,
        ChecklistError::NewerData => ErrorKind::NewerData,
        ChecklistError::Storage { .. } => ErrorKind::Internal,
        ChecklistError::Stopped { kind } => kind,
    }
}

/// What the command thread owns: the checklist and the file it is saved in.
struct State {
    checklist: Checklist,
    file: JsonFile<Saved>,
    published: Arc<Published<Vec<Item>>>,
}

impl State {
    /// Changes a copy, saves it, then keeps it: a failed save leaves the
    /// checklist as it was, which the runtime requires of a failed command.
    fn change<T>(
        &mut self,
        edit: impl FnOnce(&mut Checklist) -> Result<T, ChecklistError>,
    ) -> Result<T, ChecklistError> {
        let mut next = self.checklist.clone();
        let value = edit(&mut next)?;
        self.file.save(&next.to_saved())?;
        self.checklist = next;
        self.published.set(self.checklist.items().to_vec());
        Ok(value)
    }
}

/// The app's core. The Swift app opens one per process through ForgeKit's
/// `ForgeHost` and shares it.
#[derive(uniffi::Object)]
pub struct ChecklistCore {
    rt: Runtime<State>,
    items: Arc<Published<Vec<Item>>>,
    caches: PathBuf,
}

#[uniffi::export]
impl ChecklistCore {
    /// Opens the checklist saved in `data_directory` (created if missing),
    /// holding the directory so no second core can write to it.
    /// `caches_directory` is for files the app can lose (exports).
    #[uniffi::constructor]
    pub fn new(
        data_directory: String,
        caches_directory: String,
    ) -> Result<Arc<Self>, ChecklistError> {
        let directory = DataDirectory::open(&data_directory)?;
        let file = JsonFile::new(directory.path().join(SAVE_FILE), SAVE_VERSION);
        let checklist = match file.load()? {
            Some(saved) => Checklist::from_saved(saved),
            None => Checklist::default(),
        };
        let items = Arc::new(Published::new(checklist.items().to_vec()));
        let state = State {
            checklist,
            file,
            published: Arc::clone(&items),
        };
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("checklist", directory, state),
            items,
            caches: PathBuf::from(caches_directory),
        }))
    }

    /// Every item, oldest first. Never waits for a command.
    pub fn items(&self) -> Vec<Item> {
        self.items.get().as_ref().clone()
    }

    /// How many items there are and how many are done.
    pub fn summary(&self) -> Summary {
        Summary::of(&self.items.get())
    }

    /// Adds an item at the end; returns its id.
    pub async fn add(&self, title: String) -> Result<u64, ChecklistError> {
        self.rt
            .command("add", move |state| state.change(|list| list.add(&title)))
            .await
    }

    /// Marks an item done, or not done again.
    pub async fn toggle(&self, id: u64) -> Result<(), ChecklistError> {
        self.rt
            .command("toggle", move |state| state.change(|list| list.toggle(id)))
            .await
    }

    pub async fn rename(&self, id: u64, title: String) -> Result<(), ChecklistError> {
        self.rt
            .command("rename", move |state| {
                state.change(|list| list.rename(id, &title))
            })
            .await
    }

    pub async fn remove(&self, id: u64) -> Result<(), ChecklistError> {
        self.rt
            .command("remove", move |state| state.change(|list| list.remove(id)))
            .await
    }

    /// Removes every done item; returns how many went.
    pub async fn clear_done(&self) -> Result<u32, ChecklistError> {
        self.rt
            .command("clear-done", |state| {
                state.change(|list| Ok(list.clear_done()))
            })
            .await
    }

    /// The items whose title contains every word of `query`, ignoring
    /// case. On-screen work: stopped by `cancellation`, by the app going to
    /// the background (`Stopped { Interrupted }`) and by shutdown.
    pub async fn search(
        &self,
        query: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Vec<Item>, ChecklistError> {
        let items = self.items.get();
        let work = self.rt.foreground(cancellation.as_deref());
        self.rt
            .run("search", work, move |cancel| {
                checklist::search(&items, &query, || cancel.is_cancelled()).ok_or(
                    ChecklistError::Stopped {
                        kind: ErrorKind::Cancelled,
                    },
                )
            })
            .await
    }

    /// Writes the checklist as plain text for sharing; returns the file's
    /// path. May finish after the app leaves the screen; stopped by
    /// `cancellation` and shutdown.
    pub async fn export_text(
        &self,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<String, ChecklistError> {
        let items = self.items.get();
        let path = self.caches.join(EXPORT_FILE);
        let work = self.rt.lifetime(cancellation.as_deref());
        self.rt
            .run("export", work, move |cancel| {
                if cancel.is_cancelled() {
                    return Err(ChecklistError::Stopped {
                        kind: ErrorKind::Cancelled,
                    });
                }
                write_export(&path, &checklist::as_text(&items))?;
                Ok(path.to_string_lossy().into_owned())
            })
            .await
    }
}

fn write_export(path: &Path, text: &str) -> Result<(), ChecklistError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| ChecklistError::Storage {
            reason: format!("create the export folder: {err}"),
        })?;
    }
    forge_storage::write_atomic(path, text.as_bytes()).map_err(ChecklistError::from)
}

forge_app::export_runtime!(ChecklistCore, rt);
