//! Background downloads that outlive the app: the Rust side of the
//! `transfers` service (`native_services!(transfers)`, feature `transfers`).
//!
//! On iOS the host runs the downloads in a background `URLSession`, which
//! keeps going while the app is suspended or ended by the system and wakes
//! the app when they finish. This module is everything about them that does
//! not need iOS, so it is tested on Linux against [`testing::FakeSession`]:
//!
//! - **A ledger saved before the transfer starts.** Every transfer gets an
//!   id, and its record (what was asked for, where the file goes, its state)
//!   is in `transfers.json` before the host is asked to start it, so a
//!   relaunched app knows what it asked for.
//! - **Files out of the system's hands at once.** The host moves a finished
//!   download into this service's `inbox/` folder before its callback
//!   returns (iOS deletes the temporary file afterwards), then reports it.
//! - **Verification before keeping a file.** HTTP status, size limit,
//!   expected size and, when given, the SHA-256. A file that fails is
//!   deleted, never kept.
//! - **Delivery into the app's [`FileStore`]** under the name the app chose,
//!   once the app has registered that store ([`Transfers::deliver_to`]).
//! - **Recovery after a relaunch** ([`Transfers::reconcile`]): a task the
//!   session has but the ledger does not is cancelled; a record whose task
//!   the session no longer has is marked [`Failure::Interrupted`]; files in
//!   the inbox nobody asked for are removed.
//!
//! Retrying is the app's decision: [`Transfers::retry`] starts a failed or
//! interrupted transfer again under the same id (use `forge_net::Backoff` to
//! space attempts out). Background sessions already wait out network loss on
//! their own, so a failure that reaches Rust is usually a server error, a
//! failed check, or the person force-quitting the app.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use forge_storage::files::{is_sha256_hex, sha256_file};
use forge_storage::{FileStore, JsonFile, StorageError};
use serde::{Deserialize, Serialize};

use crate::feed::{Feed, Watch};

/// Version of `transfers.json` this build writes.
const LEDGER_VERSION: u32 = 1;

/// What the app asks to download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRequest {
    /// An `https://` URL (or `http://` for a local test server).
    pub url: String,
    /// Extra request headers. Kept in the ledger, so don't put a secret here
    /// that must not be on disk.
    pub headers: Vec<(String, String)>,
    /// The key of the [`FileStore`] the file goes to; see
    /// [`Transfers::deliver_to`].
    pub store: String,
    /// The file's name in that store (`FileStore` naming rules).
    pub name: String,
    /// The size the server should send, when known: a file of any other
    /// size is rejected, and the host uses it to schedule the transfer.
    pub expected_bytes: Option<u64>,
    /// The largest file accepted. A transfer that grows past it is
    /// cancelled.
    pub max_bytes: Option<u64>,
    /// The SHA-256 the file must have (64 lowercase hex characters).
    pub sha256: Option<String>,
    /// May use cellular data. False keeps it to Wi-Fi and wired networks
    /// (for large files such as on-device models).
    pub allows_cellular: bool,
    /// Lets the system choose a good time (charging, on Wi-Fi). For
    /// prefetching, not for something the person is waiting for.
    pub discretionary: bool,
}

impl TransferRequest {
    /// A download of `url` into `store` as `name`, on any network.
    pub fn new(url: impl Into<String>, store: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
            store: store.into(),
            name: name.into(),
            expected_bytes: None,
            max_bytes: None,
            sha256: None,
            allows_cellular: true,
            discretionary: false,
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn expected_bytes(mut self, bytes: u64) -> Self {
        self.expected_bytes = Some(bytes);
        self
    }

    pub fn max_bytes(mut self, bytes: u64) -> Self {
        self.max_bytes = Some(bytes);
        self
    }

    pub fn sha256(mut self, hex: impl Into<String>) -> Self {
        self.sha256 = Some(hex.into());
        self
    }

    /// Wi-Fi and wired networks only.
    pub fn unmetered_only(mut self) -> Self {
        self.allows_cellular = false;
        self
    }

    pub fn discretionary(mut self) -> Self {
        self.discretionary = true;
        self
    }

    fn validate(&self) -> Result<(), TransferError> {
        let invalid = |reason: &str| {
            Err(TransferError::InvalidRequest {
                reason: reason.into(),
            })
        };
        let lower = self.url.to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            return invalid("the URL must start with https:// or http://");
        }
        if self.store.trim().is_empty() {
            return invalid("the store key must not be empty");
        }
        if FileStore::new("/").path(&self.name).is_err() {
            return invalid("the file name is not a valid FileStore name");
        }
        if let Some(hash) = &self.sha256 {
            if !is_sha256_hex(hash) {
                return invalid("sha256 must be 64 lowercase hex characters");
            }
        }
        if let (Some(expected), Some(max)) = (self.expected_bytes, self.max_bytes) {
            if expected > max {
                return invalid("expected_bytes is larger than max_bytes");
            }
        }
        for (name, value) in &self.headers {
            if name.is_empty() || name.contains([':', '\r', '\n']) || value.contains(['\r', '\n']) {
                return invalid("a header name or value is not valid");
            }
        }
        Ok(())
    }
}

/// Why a transfer did not produce a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Failure {
    /// The server answered with a status other than 2xx.
    Http { status: u16 },
    /// The host's session gave up (`code` is the `NSURLError` code on iOS).
    Network { code: i64, reason: String },
    /// The file arrived but failed a check (size, limit or SHA-256), and
    /// was deleted.
    Verification { reason: String },
    /// The transfer stopped without the app cancelling it: the person
    /// force-quit the app, or the session no longer knows the task after a
    /// relaunch. Safe to [`retry`](Transfers::retry).
    Interrupted,
    /// The file could not be kept on this device.
    Storage { reason: String },
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http { status } => write!(f, "the server answered {status}"),
            Self::Network { code, reason } => write!(f, "network error {code}: {reason}"),
            Self::Verification { reason } => write!(f, "the file failed a check: {reason}"),
            Self::Interrupted => f.write_str("the transfer was interrupted"),
            Self::Storage { reason } => write!(f, "the file could not be kept: {reason}"),
        }
    }
}

/// Where a transfer is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferState {
    /// Recorded; the host has been asked to start it.
    Queued,
    /// Bytes are arriving. `expected` is what the server announced.
    Running {
        received: u64,
        expected: Option<u64>,
    },
    /// Downloaded and verified, waiting in the inbox for its store to be
    /// registered with [`Transfers::deliver_to`].
    Downloaded {
        size: u64,
    },
    /// In its store under the requested name.
    Delivered {
        size: u64,
    },
    Failed(Failure),
    /// Cancelled by the app.
    Cancelled,
}

impl TransferState {
    /// Still with the host: queued or running.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Queued | Self::Running { .. })
    }

    /// Nothing more will happen without the app: delivered, failed or
    /// cancelled.
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            Self::Delivered { .. } | Self::Failed(_) | Self::Cancelled
        )
    }
}

/// One transfer, as saved in the ledger.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRecord {
    pub id: String,
    pub request: TransferRequest,
    pub state: TransferState,
    /// How many times it has been started (1 for the first attempt).
    pub attempts: u32,
    /// Unix milliseconds.
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Errors from [`Transfers`]. `reason` fields are diagnostics, not UI copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferError {
    InvalidRequest {
        reason: String,
    },
    /// No transfer has this id.
    NotFound {
        id: String,
    },
    /// The operation does not fit the transfer's state (retrying one that
    /// is still running, for example).
    WrongState {
        id: String,
        reason: String,
    },
    /// The ledger could not be read or saved.
    Storage {
        reason: String,
    },
    /// The host refused to start the transfer.
    Session {
        reason: String,
    },
}

impl fmt::Display for TransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { reason } => write!(f, "invalid transfer: {reason}"),
            Self::NotFound { id } => write!(f, "no transfer {id}"),
            Self::WrongState { id, reason } => write!(f, "transfer {id}: {reason}"),
            Self::Storage { reason } => write!(f, "transfer ledger: {reason}"),
            Self::Session { reason } => write!(f, "transfer session: {reason}"),
        }
    }
}

impl std::error::Error for TransferError {}

impl From<StorageError> for TransferError {
    fn from(err: StorageError) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

/// What the host is asked to download. The host names the task by `id`
/// (iOS: `URLSessionTask.taskDescription`), which survives a relaunch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRequest {
    pub id: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub expected_bytes: Option<u64>,
    pub allows_cellular: bool,
    pub discretionary: bool,
}

/// A task the host's session still has, reported after a relaunch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionTask {
    pub id: String,
    pub received: u64,
    pub expected: Option<u64>,
}

/// The host's download session (iOS: a background `URLSession`).
pub trait Session: Send + Sync {
    /// Starts downloading. The host must later report the outcome through
    /// [`Transfers::handle`]: [`HostEvent::Downloaded`] after moving the
    /// file to [`Transfers::inbox_file`], or [`HostEvent::Failed`].
    fn start(&self, request: &SessionRequest) -> Result<(), String>;
    /// Cancels the task with this id, if the session has one. The host then
    /// reports it as failed with `cancelled: true`.
    fn cancel(&self, id: &str);
}

/// What the host reports about a task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEvent {
    Progress {
        id: String,
        received: u64,
        expected: Option<u64>,
    },
    /// The body is in [`Transfers::inbox_file`] for `id`, moved there
    /// before the host's callback returned. `status` is the HTTP status.
    Downloaded { id: String, status: u16 },
    /// The task ended without a file. `cancelled` is true when it was
    /// cancelled (by the app, or by the system when the person force-quit
    /// the app).
    Failed {
        id: String,
        code: i64,
        reason: String,
        cancelled: bool,
    },
}

/// A change to a transfer, as passed to [`Transfers::watch`] listeners.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferUpdate {
    Changed(Box<TransferRecord>),
    /// The record was removed with [`Transfers::remove`].
    Removed {
        id: String,
    },
}

#[derive(Default, Serialize, Deserialize)]
struct Ledger {
    transfers: Vec<TransferRecord>,
}

struct Inner {
    records: Vec<TransferRecord>,
    stores: HashMap<String, FileStore>,
}

/// The transfers of one app. See the [module documentation](self).
pub struct Transfers {
    directory: PathBuf,
    ledger: JsonFile<Ledger>,
    session: Arc<dyn Session>,
    inner: Mutex<Inner>,
    feed: Arc<Feed<TransferUpdate>>,
}

/// Numbers ids made in this process.
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Ids are `t` and hex digits, so they are safe as file names and the host
/// can't name a path with one.
fn is_valid_id(id: &str) -> bool {
    id.len() > 1
        && id.len() <= 64
        && id.starts_with('t')
        && id[1..]
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'-'))
}

fn lock(mutex: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Transfers {
    /// Opens the ledger in `directory` (the host's folder for this service:
    /// on iOS `Application Support/ForgeTransfers`). An unreadable ledger is
    /// replaced by an empty one, since the session can be reconciled
    /// against it anyway; a ledger from a newer build is an error.
    pub fn open(
        directory: impl Into<PathBuf>,
        session: Arc<dyn Session>,
    ) -> Result<Self, TransferError> {
        let directory = directory.into();
        let ledger = JsonFile::<Ledger>::new(directory.join("transfers.json"), LEDGER_VERSION);
        let records = match ledger.load() {
            Ok(found) => found.unwrap_or_default().transfers,
            Err(StorageError::CorruptData { .. }) => {
                ledger.save_over_corrupt(&Ledger::default())?;
                Vec::new()
            }
            Err(err) => return Err(err.into()),
        };
        std::fs::create_dir_all(directory.join("inbox")).map_err(|err| TransferError::Storage {
            reason: format!("create the inbox: {err}"),
        })?;
        Ok(Self {
            directory,
            ledger,
            session,
            inner: Mutex::new(Inner {
                records,
                stores: HashMap::new(),
            }),
            feed: Feed::new(),
        })
    }

    /// The service's folder.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Where the host puts the finished download of `id`.
    pub fn inbox_file(&self, id: &str) -> PathBuf {
        self.directory.join("inbox").join(id)
    }

    /// Records `request` and asks the host to start it. Returns the
    /// transfer's id, which the app keeps to follow it.
    pub fn start(&self, request: TransferRequest) -> Result<String, TransferError> {
        request.validate()?;
        let mut inner = lock(&self.inner);
        let id = loop {
            let id = format!(
                "t{:x}-{:x}-{:x}",
                now_ms(),
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            );
            if !inner.records.iter().any(|r| r.id == id) {
                break id;
            }
        };
        let now = now_ms();
        let record = TransferRecord {
            id: id.clone(),
            request,
            state: TransferState::Queued,
            attempts: 1,
            created_at_ms: now,
            updated_at_ms: now,
        };
        inner.records.push(record.clone());
        if let Err(err) = self.save(&inner) {
            inner.records.pop();
            return Err(err);
        }
        let changed = self.launch(&mut inner, record);
        drop(inner);
        self.publish(changed);
        Ok(id)
    }

    /// Asks the host to start `record` (already saved), and marks it failed
    /// if the host refuses.
    fn launch(&self, inner: &mut Inner, record: TransferRecord) -> Vec<TransferUpdate> {
        let request = SessionRequest {
            id: record.id.clone(),
            url: record.request.url.clone(),
            headers: record.request.headers.clone(),
            expected_bytes: record.request.expected_bytes,
            allows_cellular: record.request.allows_cellular,
            discretionary: record.request.discretionary,
        };
        match self.session.start(&request) {
            Ok(()) => vec![TransferUpdate::Changed(Box::new(record))],
            Err(reason) => self.set_state(
                inner,
                &record.id,
                TransferState::Failed(Failure::Network { code: 0, reason }),
            ),
        }
    }

    /// Cancels a queued or running transfer. A transfer that already ended
    /// is left as it is.
    pub fn cancel(&self, id: &str) -> Result<(), TransferError> {
        let mut inner = lock(&self.inner);
        let state = self.find(&inner, id)?.state.clone();
        if !state.is_active() {
            return Ok(());
        }
        // Marked first, so the host's report of the cancelled task finds it
        // no longer active and is ignored.
        let changed = self.set_state(&mut inner, id, TransferState::Cancelled);
        drop(inner);
        self.session.cancel(id);
        self.publish(changed);
        Ok(())
    }

    /// Starts a failed or cancelled transfer again, under the same id.
    pub fn retry(&self, id: &str) -> Result<(), TransferError> {
        let mut inner = lock(&self.inner);
        let record = self.find(&inner, id)?.clone();
        if !matches!(
            record.state,
            TransferState::Failed(_) | TransferState::Cancelled
        ) {
            return Err(TransferError::WrongState {
                id: id.into(),
                reason: "only a failed or cancelled transfer can be retried".into(),
            });
        }
        let _ = std::fs::remove_file(self.inbox_file(id));
        let record = {
            let entry = inner
                .records
                .iter_mut()
                .find(|r| r.id == id)
                .expect("found above");
            entry.state = TransferState::Queued;
            entry.attempts += 1;
            entry.updated_at_ms = now_ms();
            entry.clone()
        };
        self.save(&inner)?;
        let changed = self.launch(&mut inner, record);
        drop(inner);
        self.publish(changed);
        Ok(())
    }

    /// Forgets a transfer that is no longer active, and deletes its file
    /// from the inbox if it was never delivered. A delivered file stays in
    /// its store.
    pub fn remove(&self, id: &str) -> Result<(), TransferError> {
        let mut inner = lock(&self.inner);
        if self.find(&inner, id)?.state.is_active() {
            return Err(TransferError::WrongState {
                id: id.into(),
                reason: "cancel it before removing it".into(),
            });
        }
        let index = inner
            .records
            .iter()
            .position(|r| r.id == id)
            .expect("found above");
        let removed = inner.records.remove(index);
        if let Err(err) = self.save(&inner) {
            inner.records.insert(index, removed);
            return Err(err);
        }
        drop(inner);
        let _ = std::fs::remove_file(self.inbox_file(id));
        self.publish(vec![TransferUpdate::Removed { id: id.into() }]);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<TransferRecord> {
        lock(&self.inner)
            .records
            .iter()
            .find(|r| r.id == id)
            .cloned()
    }

    /// Every transfer in the ledger, oldest first.
    pub fn list(&self) -> Vec<TransferRecord> {
        lock(&self.inner).records.clone()
    }

    /// Registers the [`FileStore`] that transfers with `store == key` are
    /// delivered to, and delivers the ones already waiting in the inbox.
    /// Call it at launch for each store the app downloads into.
    pub fn deliver_to(&self, key: impl Into<String>, store: FileStore) {
        let key = key.into();
        let mut inner = lock(&self.inner);
        inner.stores.insert(key.clone(), store);
        let waiting: Vec<String> = inner
            .records
            .iter()
            .filter(|r| {
                r.request.store == key && matches!(r.state, TransferState::Downloaded { .. })
            })
            .map(|r| r.id.clone())
            .collect();
        let mut changed = Vec::new();
        for id in waiting {
            changed.extend(self.deliver(&mut inner, &id));
        }
        drop(inner);
        self.publish(changed);
    }

    /// Calls `listener` with every change until the watch is dropped, on
    /// the thread that made the change (often the host's delegate queue),
    /// so copy what you need and return.
    pub fn watch(&self, listener: impl Fn(&TransferUpdate) + Send + Sync + 'static) -> Watch {
        self.feed
            .watch(listener, || Ok::<(), std::convert::Infallible>(()), || {})
            .unwrap_or_else(|never| match never {})
    }

    /// Applies a report from the host. Returns once the ledger is saved and
    /// a downloaded file verified (and delivered, if its store is
    /// registered), so the host may tell the system it is done afterwards.
    pub fn handle(&self, event: HostEvent) {
        let id = match &event {
            HostEvent::Progress { id, .. }
            | HostEvent::Downloaded { id, .. }
            | HostEvent::Failed { id, .. } => id,
        };
        if !is_valid_id(id) {
            return;
        }
        let mut inner = lock(&self.inner);
        let changed = match event {
            HostEvent::Progress {
                id,
                received,
                expected,
            } => self.progress(&mut inner, &id, received, expected),
            HostEvent::Downloaded { id, status } => self.downloaded(&mut inner, &id, status),
            HostEvent::Failed {
                id,
                code,
                reason,
                cancelled,
            } => self.failed(&mut inner, &id, code, reason, cancelled),
        };
        drop(inner);
        self.publish(changed);
    }

    /// Compares the ledger with the tasks the host's session still has,
    /// after a launch. A task the ledger doesn't know is cancelled; an
    /// active record whose task is gone (and whose file never arrived) is
    /// marked [`Failure::Interrupted`]; inbox files no record waits for
    /// are deleted. A report that arrives later for an interrupted record
    /// still counts, so the order of the host's callbacks doesn't matter.
    pub fn reconcile(&self, tasks: &[SessionTask]) {
        let mut inner = lock(&self.inner);
        let mut changed = Vec::new();
        let mut orphans = Vec::new();
        for task in tasks {
            match inner
                .records
                .iter()
                .find(|r| r.id == task.id)
                .map(|r| r.state.clone())
            {
                Some(state) if state.is_active() => {
                    changed.extend(self.progress(
                        &mut inner,
                        &task.id,
                        task.received,
                        task.expected,
                    ));
                }
                Some(TransferState::Failed(Failure::Interrupted)) => {
                    // Reported gone earlier but still running: back to running.
                    changed.extend(self.set_state(
                        &mut inner,
                        &task.id,
                        TransferState::Running {
                            received: task.received,
                            expected: task.expected,
                        },
                    ));
                }
                Some(_) => {}
                None => orphans.push(task.id.clone()),
            }
        }
        let live: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        let lost: Vec<String> = inner
            .records
            .iter()
            .filter(|r| r.state.is_active() && !live.contains(r.id.as_str()))
            .map(|r| r.id.clone())
            .collect();
        for id in lost {
            let _ = std::fs::remove_file(self.inbox_file(&id));
            changed.extend(self.set_state(
                &mut inner,
                &id,
                TransferState::Failed(Failure::Interrupted),
            ));
        }
        // Inbox files are kept only for Downloaded records.
        let waiting: HashSet<String> = inner
            .records
            .iter()
            .filter(|r| matches!(r.state, TransferState::Downloaded { .. }))
            .map(|r| r.id.clone())
            .collect();
        if let Ok(entries) = std::fs::read_dir(self.directory.join("inbox")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !waiting.contains(&name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        drop(inner);
        for id in orphans {
            self.session.cancel(&id);
        }
        self.publish(changed);
    }

    fn progress(
        &self,
        inner: &mut Inner,
        id: &str,
        received: u64,
        expected: Option<u64>,
    ) -> Vec<TransferUpdate> {
        let Some(record) = inner.records.iter().find(|r| r.id == id) else {
            return Vec::new();
        };
        if !matches!(
            record.state,
            TransferState::Queued
                | TransferState::Running { .. }
                | TransferState::Failed(Failure::Interrupted)
        ) {
            return Vec::new();
        }
        let limit = record.request.max_bytes;
        if let Some(limit) = limit {
            if received > limit || expected.is_some_and(|e| e > limit) {
                let changed = self.set_state(
                    inner,
                    id,
                    TransferState::Failed(Failure::Verification {
                        reason: format!("larger than the limit of {limit} bytes"),
                    }),
                );
                self.session.cancel(id);
                return changed;
            }
        }
        // Progress is not saved on every report: it is cheap to lose and
        // the session reports it again after a relaunch.
        let entry = inner
            .records
            .iter_mut()
            .find(|r| r.id == id)
            .expect("found above");
        let was_running = matches!(entry.state, TransferState::Running { .. });
        entry.state = TransferState::Running { received, expected };
        entry.updated_at_ms = now_ms();
        let record = entry.clone();
        if !was_running {
            let _ = self.save(inner);
        }
        vec![TransferUpdate::Changed(Box::new(record))]
    }

    fn downloaded(&self, inner: &mut Inner, id: &str, status: u16) -> Vec<TransferUpdate> {
        let file = self.inbox_file(id);
        let Some(record) = inner.records.iter().find(|r| r.id == id).cloned() else {
            // Nobody asked for it (an older install, or removed meanwhile).
            let _ = std::fs::remove_file(&file);
            return Vec::new();
        };
        if !matches!(
            record.state,
            TransferState::Queued
                | TransferState::Running { .. }
                | TransferState::Failed(Failure::Interrupted)
        ) {
            // Cancelled or failed a check meanwhile: the file is not wanted.
            let _ = std::fs::remove_file(&file);
            return Vec::new();
        }
        let failure = if !(200..300).contains(&status) {
            Some(Failure::Http { status })
        } else {
            verify(&file, &record.request).err()
        };
        if let Some(failure) = failure {
            let _ = std::fs::remove_file(&file);
            return self.set_state(inner, id, TransferState::Failed(failure));
        }
        let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        let mut changed = self.set_state(inner, id, TransferState::Downloaded { size });
        changed.extend(self.deliver(inner, id));
        changed
    }

    fn failed(
        &self,
        inner: &mut Inner,
        id: &str,
        code: i64,
        reason: String,
        cancelled: bool,
    ) -> Vec<TransferUpdate> {
        let Some(record) = inner.records.iter().find(|r| r.id == id) else {
            return Vec::new();
        };
        if !record.state.is_active() {
            // Already cancelled, failed or downloaded: a late report.
            return Vec::new();
        }
        // The app's own cancel already marked the record, so a cancelled
        // report for an active one came from the system (force quit).
        let state = if cancelled {
            TransferState::Failed(Failure::Interrupted)
        } else {
            TransferState::Failed(Failure::Network { code, reason })
        };
        self.set_state(inner, id, state)
    }

    /// Moves a Downloaded record's file into its store, if registered.
    fn deliver(&self, inner: &mut Inner, id: &str) -> Vec<TransferUpdate> {
        let Some(record) = inner.records.iter().find(|r| r.id == id).cloned() else {
            return Vec::new();
        };
        let Some(store) = inner.stores.get(&record.request.store).cloned() else {
            return Vec::new();
        };
        let file = self.inbox_file(id);
        let state = if file.is_file() {
            match store.accept(&file, &record.request.name) {
                Ok(stored) => TransferState::Delivered { size: stored.size },
                Err(err) => TransferState::Failed(Failure::Storage {
                    reason: err.to_string(),
                }),
            }
        } else if let Some(path) = store.get(&record.request.name) {
            // Moved before the ledger was saved last time.
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            TransferState::Delivered { size }
        } else {
            TransferState::Failed(Failure::Storage {
                reason: "the downloaded file is missing".into(),
            })
        };
        self.set_state(inner, id, state)
    }

    fn set_state(&self, inner: &mut Inner, id: &str, state: TransferState) -> Vec<TransferUpdate> {
        let Some(entry) = inner.records.iter_mut().find(|r| r.id == id) else {
            return Vec::new();
        };
        if !matches!(state, TransferState::Running { .. }) {
            // Ids and outcomes only, never URLs or file names.
            let outcome = match &state {
                TransferState::Failed(failure) => format!("failed: {failure}"),
                other => format!("{other:?}"),
            };
            forge_runtime::logging::info!(target: "transfers", "{id}: {outcome}");
        }
        entry.state = state;
        entry.updated_at_ms = now_ms();
        let record = entry.clone();
        // A failed save leaves the change in memory; the next save writes
        // it, and reconcile repairs what a crash loses.
        let _ = self.save(inner);
        vec![TransferUpdate::Changed(Box::new(record))]
    }

    fn find<'a>(&self, inner: &'a Inner, id: &str) -> Result<&'a TransferRecord, TransferError> {
        inner
            .records
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| TransferError::NotFound { id: id.into() })
    }

    fn save(&self, inner: &Inner) -> Result<(), TransferError> {
        self.ledger
            .save(&Ledger {
                transfers: inner.records.clone(),
            })
            .map_err(Into::into)
    }

    fn publish(&self, changes: Vec<TransferUpdate>) {
        for change in changes {
            self.feed.publish(change);
        }
    }
}

/// Whether a host-reported id is one this service could have made, so a
/// bad `taskDescription` can't name a path outside the inbox.
pub fn is_transfer_id(id: &str) -> bool {
    is_valid_id(id)
}

fn verify(file: &Path, request: &TransferRequest) -> Result<(), Failure> {
    let size = std::fs::metadata(file)
        .map_err(|err| Failure::Storage {
            reason: format!("the downloaded file is unreadable: {err}"),
        })?
        .len();
    if let Some(limit) = request.max_bytes {
        if size > limit {
            return Err(Failure::Verification {
                reason: format!("{size} bytes, larger than the limit of {limit}"),
            });
        }
    }
    if let Some(expected) = request.expected_bytes {
        if size != expected {
            return Err(Failure::Verification {
                reason: format!("{size} bytes, expected {expected}"),
            });
        }
    }
    if let Some(expected) = &request.sha256 {
        let actual = sha256_file(file).map_err(|err| Failure::Storage {
            reason: err.to_string(),
        })?;
        if &actual != expected {
            return Err(Failure::Verification {
                reason: format!("SHA-256 {actual}, expected {expected}"),
            });
        }
    }
    Ok(())
}

/// A session for tests: it records what it was asked, and the test plays
/// the host's part with [`FakeSession::finish`] and friends.
pub mod testing {
    use super::*;

    #[derive(Default)]
    pub struct FakeSession {
        started: Mutex<Vec<SessionRequest>>,
        cancelled: Mutex<Vec<String>>,
        refuse: Mutex<Option<String>>,
    }

    impl FakeSession {
        pub fn started(&self) -> Vec<SessionRequest> {
            self.started.lock().unwrap().clone()
        }

        pub fn cancelled(&self) -> Vec<String> {
            self.cancelled.lock().unwrap().clone()
        }

        /// Makes the next starts fail with `reason` (None: succeed again).
        pub fn refuse_starts(&self, reason: Option<&str>) {
            *self.refuse.lock().unwrap() = reason.map(Into::into);
        }

        /// Plays a finished download: writes `body` to the inbox the way the
        /// host moves its temporary file there, then reports it.
        pub fn finish(&self, transfers: &Transfers, id: &str, status: u16, body: &[u8]) {
            std::fs::write(transfers.inbox_file(id), body).unwrap();
            transfers.handle(HostEvent::Downloaded {
                id: id.into(),
                status,
            });
        }
    }

    impl Session for FakeSession {
        fn start(&self, request: &SessionRequest) -> Result<(), String> {
            if let Some(reason) = self.refuse.lock().unwrap().clone() {
                return Err(reason);
            }
            self.started.lock().unwrap().push(request.clone());
            Ok(())
        }

        fn cancel(&self, id: &str) {
            self.cancelled.lock().unwrap().push(id.into());
        }
    }
}
