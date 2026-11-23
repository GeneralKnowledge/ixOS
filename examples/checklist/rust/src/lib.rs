//! Checklist: a list of things to do, kept on the phone.
//!
//! The app's whole Rust side. What a checklist is lives in [`checklist`];
//! this file is the API the SwiftUI app calls, built on the application
//! runtime (`forge_app`):
//!
//! - **Commands** (`add`, `toggle`, `rename`, `set_due`, `remove`,
//!   `clear_done`) run one
//!   at a time on the runtime's command thread. Each saves before it
//!   returns; the runtime then raises the revision and announces `Changed`.
//! - **Queries** (`items`, `summary`, `agenda`) read a [`Published`] snapshot and
//!   never wait for a command.
//! - **Work**: `search` is on-screen work (stopped by the background, by
//!   the host's cancellation, by shutdown); `export_text` writes a file to
//!   share and is finishing work: if the app leaves the screen meanwhile,
//!   the host asks iOS for time to finish it. `export_csv` is the same work
//!   as a spreadsheet; `import_csv` adds a spreadsheet's rows in one command.
//! - **The runtime contract** (`events`, `set_phase`, `phase`, `revision`,
//!   background work, the launch report, `shutdown`) comes from
//!   `forge_app::export_runtime!`.

mod checklist;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use forge_app::{Cancellation, DataDirectory, Document, ErrorKind, Published, Runtime};

use checklist::Checklist;
pub use checklist::{Agenda, Item, Summary, MAX_TITLE_CHARS};

// No namespace argument: UniFFI takes the library's name (checklist_core), so this
// crate keeps its own bindings when another app links it.
uniffi::setup_scaffolding!();

// `forge_log` for the host: Swift's `ForgeLog("Category")` writes into the
// same log as Rust (`forge device logs`, crash reports).
forge_runtime::log_export!();

const SAVE_FILE: &str = "checklist.json";
const SAVE_VERSION: u32 = 1;
const EXPORT_FILE: &str = "Checklist.txt";
const CSV_FILE: &str = "Checklist.csv";

/// One spreadsheet row: the columns a person sees in Numbers or Excel.
#[derive(serde::Serialize, serde::Deserialize)]
struct CsvRow {
    #[serde(rename = "Item")]
    item: String,
    /// "Yes" when done; anything else (or a missing column) is not done.
    #[serde(rename = "Done", default)]
    done: String,
    /// The day it is due, `YYYY-MM-DD`; empty (or a missing column) for none.
    #[serde(rename = "Due", default)]
    due: String,
}

/// What `import_csv` did: how many items it added, and why it skipped the
/// rows it couldn't use ("Line 4: an item needs a title").
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct CsvImportSummary {
    pub added: u32,
    pub skipped: Vec<String>,
}

/// Everything that can go wrong, in the app's words. `checklist_error_kind`
/// maps each case to the framework's [`ErrorKind`], so ForgeKit can present
/// any app's errors the same way.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum ChecklistError {
    #[error("an item needs a title")]
    EmptyTitle,
    #[error("titles are at most {max} characters")]
    TitleTooLong { max: u32 },
    #[error("\"{day}\" is not a day (use YYYY-MM-DD)")]
    InvalidDay { day: String },
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

forge_app::app_error_conversions!(ChecklistError: Stopped, OpenError, DocumentError, StorageError);

/// The framework kind of a Checklist error.
#[uniffi::export]
pub fn checklist_error_kind(error: ChecklistError) -> ErrorKind {
    match error {
        ChecklistError::EmptyTitle
        | ChecklistError::TitleTooLong { .. }
        | ChecklistError::InvalidDay { .. } => ErrorKind::Invalid,
        ChecklistError::NotFound { .. } => ErrorKind::NotFound,
        ChecklistError::InUse => ErrorKind::Unavailable,
        ChecklistError::Corrupt { .. } => ErrorKind::Corrupt,
        ChecklistError::NewerData => ErrorKind::NewerData,
        ChecklistError::Storage { .. } => ErrorKind::Internal,
        ChecklistError::Stopped { kind } => kind,
    }
}

/// What the command thread owns: the checklist, saved in its document.
struct State {
    checklist: Document<Checklist>,
    published: Arc<Published<Vec<Item>>>,
}

impl State {
    /// Changes the checklist through its document (a copy is changed and
    /// saved, then kept: a failed save leaves it as it was, which the
    /// runtime requires of a failed command), then publishes the items.
    fn change<T>(
        &mut self,
        edit: impl FnOnce(&mut Checklist) -> Result<T, ChecklistError>,
    ) -> Result<T, ChecklistError> {
        let value = self.checklist.change(edit)?;
        self.published.set(self.checklist.value().items().to_vec());
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
        let checklist =
            Document::<Checklist>::open(directory.path().join(SAVE_FILE), SAVE_VERSION)?;
        let items = Arc::new(Published::new(checklist.value().items().to_vec()));
        let state = State {
            checklist,
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

    /// The items not yet done that have a due day, split into overdue, due
    /// `today` and upcoming. `today` is the phone's local day
    /// (`YYYY-MM-DD`): Swift knows the time zone, Rust doesn't need to.
    /// Never waits for a command.
    pub fn agenda(&self, today: String) -> Agenda {
        checklist::agenda(&self.items.get(), &today)
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

    /// Sets the local day an item is due (`YYYY-MM-DD`), or clears it with
    /// none. A day that isn't one is refused (`InvalidDay`).
    pub async fn set_due(&self, id: u64, due: Option<String>) -> Result<(), ChecklistError> {
        self.rt
            .command("set-due", move |state| {
                state.change(|list| list.set_due(id, due.as_deref()))
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
    /// path. Finishing work: it keeps going when the app leaves the screen,
    /// for as long as iOS gives the app (then `Stopped { Interrupted }`);
    /// stopped by `cancellation` and shutdown.
    pub async fn export_text(
        &self,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<String, ChecklistError> {
        let items = self.items.get();
        let path = self.caches.join(EXPORT_FILE);
        let work = self.rt.finishing(cancellation.as_deref());
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

    /// Writes the checklist as a spreadsheet (Item, Done, Due) for sharing;
    /// returns the file's path. Finishing work, like `export_text`.
    pub async fn export_csv(
        &self,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<String, ChecklistError> {
        let items = self.items.get();
        let path = self.caches.join(CSV_FILE);
        let work = self.rt.finishing(cancellation.as_deref());
        self.rt
            .run("export-csv", work, move |cancel| {
                if cancel.is_cancelled() {
                    return Err(ChecklistError::Stopped {
                        kind: ErrorKind::Cancelled,
                    });
                }
                let rows: Vec<CsvRow> = items
                    .iter()
                    .map(|item| CsvRow {
                        item: item.title.clone(),
                        done: if item.done { "Yes" } else { "No" }.into(),
                        due: item.due.clone().unwrap_or_default(),
                    })
                    .collect();
                let bytes = forge_storage::csv_file::to_csv(&rows)?;
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|err| ChecklistError::Storage {
                        reason: format!("create the export folder: {err}"),
                    })?;
                }
                forge_storage::write_atomic(&path, &bytes)?;
                Ok(path.to_string_lossy().into_owned())
            })
            .await
    }

    /// Adds the rows of a spreadsheet at `path` (an "Item" column, and
    /// optionally "Done" and "Due") after the existing items, in one save. Rows it
    /// can't use are skipped and listed; it adds nothing if none are usable.
    pub async fn import_csv(&self, path: String) -> Result<CsvImportSummary, ChecklistError> {
        // Read the file outside the command, so a slow file doesn't hold up
        // other commands.
        // Not `?`: a corrupt *spreadsheet* is not a corrupt saved checklist.
        let read =
            forge_storage::csv_file::import_csv::<CsvRow>(Path::new(&path)).map_err(|err| {
                ChecklistError::Storage {
                    reason: format!("that spreadsheet can't be read: {err}"),
                }
            })?;
        self.rt
            .command("import-csv", move |state| {
                let mut skipped: Vec<String> = read
                    .problems
                    .iter()
                    .map(|p| format!("Line {}: {}", p.line, p.reason))
                    .collect();
                let mut already = 0u32;
                let added = state.change(|list| {
                    let mut added = 0u32;
                    // Importing the same file twice adds nothing new.
                    let mut known: std::collections::HashSet<String> =
                        list.items().iter().map(|i| same_title(&i.title)).collect();
                    for (row, line) in read.rows.iter().zip(&read.row_lines) {
                        if known.contains(&same_title(&row.item)) {
                            already += 1;
                            continue;
                        }
                        // A row with a day that isn't one is skipped whole,
                        // not added without its day.
                        let due = row.due.trim();
                        if !due.is_empty() {
                            if let Err(err) = checklist::valid_day(due) {
                                skipped.push(format!("Line {line}: {err}"));
                                continue;
                            }
                        }
                        match list.add(&row.item) {
                            Ok(id) => {
                                added += 1;
                                known.insert(same_title(&row.item));
                                if is_yes(&row.done) {
                                    list.toggle(id)?;
                                }
                                if !due.is_empty() {
                                    list.set_due(id, Some(due))?;
                                }
                            }
                            Err(err) => skipped.push(format!("Line {line}: {err}")),
                        }
                    }
                    Ok(added)
                })?;
                skipped.sort_by_key(|text| line_of(text));
                if already > 0 {
                    skipped.push(if already == 1 {
                        "1 item was already on the list".into()
                    } else {
                        format!("{already} items were already on the list")
                    });
                }
                Ok(CsvImportSummary { added, skipped })
            })
            .await
    }
}

/// A title as compared for duplicates: tidied the way a saved title is
/// (runs of spaces, tabs and line breaks become one space), ignoring case.
fn same_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The line number a "Line N: ..." note starts with, for sorting.
fn line_of(text: &str) -> u64 {
    text.strip_prefix("Line ")
        .and_then(|rest| rest.split(':').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(u64::MAX)
}

fn is_yes(text: &str) -> bool {
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "yes" | "y" | "true" | "1" | "x" | "done"
    )
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

#[cfg(test)]
mod host_log_tests {
    /// Swift's `ForgeLog` calls `forge_log`: its records reach the same
    /// pipeline as Rust's (the recent records `forge device logs` and
    /// crash reports read), not just stdout.
    #[test]
    fn host_records_reach_the_log() {
        super::forge_log(
            super::ForgeLogLevel::Warn,
            "HostTest".into(),
            "from Swift".into(),
        );
        assert!(forge_runtime::logging::recent(100)
            .iter()
            .any(|r| r.target == "HostTest" && r.message == "from Swift"));
    }
}
