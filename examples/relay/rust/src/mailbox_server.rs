//! The mailbox server (N3 step 2, examples/relay/docs/relay-mailbox.md):
//! `relay/mailbox/1` over iroh, an index in SQLite and one file per
//! sealed item. The `relay-mailbox` binary runs it on Hetzner.
//!
//! The server is an untrusted courier. It never holds a key that opens an
//! item ([`crate::mailbox`]); what it enforces is who may *store* what:
//!
//! - **The caller is its iroh key.** A device registers passes for, lists,
//!   fetches and acknowledges only its own items: the recipient is always
//!   the connection's endpoint id, never a field of the request.
//! - **A deposit needs a pass** whose SHA-256 the item's recipient (`to`)
//!   registered, so a pass can't be used for anyone else. Its `from` must
//!   be the depositor's own key.
//! - **Quotas** (per pass and per recipient), the size limit, the 7-day
//!   life and duplicates are the server rules table in the plan.
//! - **Receipts** are small, one per (`from`, `id`), and outside the byte
//!   quota and the hourly count, so a full mailbox still takes them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use forge_runtime::{lock, now_ms};
use forge_storage::rusqlite::{params, OptionalExtension};
use forge_storage::Database;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointAddr};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::code::{connection, get_within, put, STEP};
use crate::hex::hex;
use crate::mailbox::{serde_bytes_vec, DepositPass, Header, SealedItem, VERSION};
use crate::model::RelayError;

/// The mailbox protocol's ALPN.
pub(crate) const MAILBOX_ALPN: &[u8] = b"forge-relay/mailbox/1";

/// How long an item is kept unread.
pub const KEPT_MS: i64 = 7 * 24 * 3600 * 1000;
/// Leeway on `expires_ms` for clocks that differ a little.
const CLOCK_SLACK_MS: i64 = 3600 * 1000;
/// The biggest item the mailbox takes; bigger files wait for a direct route.
pub const MAX_ITEM: u64 = 100 * 1024 * 1024;
/// The biggest receipt.
pub const MAX_RECEIPT: u64 = 4 * 1024;
/// Held at once for one recipient.
pub const PER_RECIPIENT: u64 = 1024 * 1024 * 1024;
/// Held at once under one deposit pass (a quarter of a mailbox).
pub const PER_PASS: u64 = 256 * 1024 * 1024;
/// Deposits under one pass in any hour.
pub const PER_PASS_HOURLY: u32 = 200;
/// Receipts held at once for one recipient.
const RECEIPTS_HELD: u32 = 10_000;
/// Passes one recipient may have registered.
const PASSES_HELD: u32 = 1_000;
/// Items listed in one answer (the rest come after acknowledging these).
const LISTED: u32 = 200;
/// How long a stream may be idle while an item's bytes move.
const IDLE: Duration = Duration::from_secs(60);
/// Uploads in progress at once, across everyone.
const UPLOADS: u32 = 16;
/// Bytes of uploads in progress at once, before any of them counts
/// against a quota.
const UPLOAD_BYTES: u64 = 512 * 1024 * 1024;
/// Fetches in progress at once, across everyone.
const FETCHES: usize = 16;

/// What is left: an item, or a receipt for one.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Item,
    Receipt,
}

/// A request on `relay/mailbox/1`, one per stream.
#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum Request {
    /// Registers pass digests for the caller's own mailbox.
    Register { digests: Vec<[u8; 32]> },
    /// Removes the caller's own pass digests.
    Deregister { digests: Vec<[u8; 32]> },
    /// Leaves an item; its ciphertext's `len` bytes follow on the stream.
    Deposit(Deposit),
    /// What waits for the caller.
    List,
    /// One waiting item; its ciphertext follows the answer.
    Fetch { from: [u8; 32], id: [u8; 16] },
    /// The caller has it: the server deletes it.
    Ack { from: [u8; 32], id: [u8; 16] },
}

/// What a deposit says about the item before its bytes.
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct Deposit {
    pub pass: [u8; 32],
    pub kind: Kind,
    pub header: Header,
    #[serde(with = "serde_bytes_vec")]
    pub enc: Vec<u8>,
    /// The ciphertext's length, in bytes.
    pub len: u64,
}

/// One item waiting, as listed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Waiting {
    pub from: [u8; 32],
    pub id: [u8; 16],
    pub kind: Kind,
    pub expires_ms: i64,
    pub size: u64,
}

/// Why the server refused.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Refusal {
    #[error("no pass for that recipient")]
    NoPass,
    #[error("that isn't the caller's")]
    NotYours,
    #[error("too big for the mailbox")]
    TooBig,
    #[error("the mailbox is full")]
    Full,
    #[error("too many items this hour")]
    TooMany,
    #[error("already expired")]
    Expired,
    #[error("kept longer than the mailbox keeps anything")]
    TooLong,
    #[error("nothing like that waiting")]
    NotFound,
    #[error("not a request this server reads")]
    Malformed,
    #[error("the server can't store it now")]
    Unavailable,
    #[error("the mailbox is busy; try again soon")]
    Busy,
}

/// The server's answer.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    Done,
    /// Stored, or (`duplicate`) the same item was already there.
    Stored {
        duplicate: bool,
    },
    Listed(Vec<Waiting>),
    /// The item's ciphertext, `len` bytes, follows.
    Item {
        kind: Kind,
        header: Header,
        #[serde(with = "serde_bytes_vec")]
        enc: Vec<u8>,
        len: u64,
    },
    Refused(Refusal),
}

impl From<forge_storage::StorageError> for Refusal {
    fn from(e: forge_storage::StorageError) -> Self {
        log::warn!("mailbox: storage error: {e}");
        Refusal::Unavailable
    }
}

fn unavailable(e: impl std::fmt::Display) -> Refusal {
    log::warn!("mailbox: storage error: {e}");
    Refusal::Unavailable
}

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out).expect("CBOR into memory");
    out
}

fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Option<T> {
    ciborium::from_reader(bytes).ok()
}

const MIGRATIONS: &[&str] = &["
    -- Deposit passes: only their SHA-256, and whose mailbox they open.
    CREATE TABLE passes (digest BLOB PRIMARY KEY, owner BLOB NOT NULL, since_ms INTEGER NOT NULL);
    CREATE INDEX passes_owner ON passes(owner);
    -- Items held: the header and HPKE `enc` here, the ciphertext in `file`.
    CREATE TABLE items (
        owner BLOB NOT NULL, sender BLOB NOT NULL, id BLOB NOT NULL,
        kind INTEGER NOT NULL, pass BLOB NOT NULL,
        expires_ms INTEGER NOT NULL, size INTEGER NOT NULL, stored_ms INTEGER NOT NULL,
        header BLOB NOT NULL, enc BLOB NOT NULL, file TEXT NOT NULL,
        PRIMARY KEY (owner, sender, id));
    CREATE INDEX items_pass ON items(pass);
    CREATE INDEX items_expires ON items(expires_ms);
    -- When each pass deposited, for the hourly count.
    CREATE TABLE deposits (pass BLOB NOT NULL, at_ms INTEGER NOT NULL);
    CREATE INDEX deposits_pass ON deposits(pass, at_ms);
"];

/// What the server keeps: the index and the item files.
pub struct MailboxStore {
    db: Mutex<Database>,
    dir: PathBuf,
    /// Uploads in progress and their declared bytes (see [`UPLOADS`]).
    uploading: Mutex<(u32, u64)>,
    /// The directory's lock (see [`MailboxStore::open`]).
    _hold: std::fs::File,
}

/// Room held for one upload in progress; given back when dropped.
pub(crate) struct Reservation<'a> {
    store: &'a MailboxStore,
    len: u64,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let mut held = lock(&self.store.uploading);
        held.0 -= 1;
        held.1 -= self.len;
    }
}

impl std::fmt::Debug for MailboxStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MailboxStore")
            .field("dir", &self.dir)
            .finish()
    }
}

/// An item that may be stored, checked before its bytes arrive.
pub(crate) struct Admitted {
    owner: [u8; 32],
    pass: [u8; 32],
    kind: Kind,
    header: Header,
    enc: Vec<u8>,
    len: u64,
}

impl MailboxStore {
    /// Opens (or creates) the store in `dir`. Partial uploads left by a
    /// stop are deleted.
    pub fn open(dir: &Path) -> Result<Self, RelayError> {
        let storage = |e: forge_storage::StorageError| RelayError::Storage {
            reason: e.to_string(),
        };
        let io = |e: std::io::Error| RelayError::Storage {
            reason: e.to_string(),
        };
        // One store per directory: a second would clean up the first's
        // uploads and files. Held until the store is dropped.
        std::fs::create_dir_all(dir).map_err(io)?;
        let hold = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("mailbox.lock"))
            .map_err(io)?;
        hold.try_lock().map_err(|_| RelayError::Storage {
            reason: format!("another mailbox server is using {}", dir.display()),
        })?;
        let (db, _) = Database::open(dir.join("mailbox.db"), MIGRATIONS).map_err(storage)?;
        let _ = std::fs::remove_dir_all(dir.join("partial"));
        for sub in ["partial", "items"] {
            std::fs::create_dir_all(dir.join(sub)).map_err(|e| RelayError::Storage {
                reason: e.to_string(),
            })?;
        }
        let store = Self {
            db: Mutex::new(db),
            dir: dir.to_path_buf(),
            uploading: Mutex::new((0, 0)),
            _hold: hold,
        };
        store.reconcile();
        Ok(store)
    }

    /// Room for an upload of `len` bytes, before its body is read, or
    /// `Busy` when too much is arriving at once.
    pub(crate) fn reserve(&self, len: u64) -> Result<Reservation<'_>, Refusal> {
        let mut held = lock(&self.uploading);
        if held.0 >= UPLOADS || held.1 + len > UPLOAD_BYTES {
            return Err(Refusal::Busy);
        }
        held.0 += 1;
        held.1 += len;
        Ok(Reservation { store: self, len })
    }

    /// Deletes item files no row names: left by a stop between a row's
    /// deletion and its file's, or a delete that failed. Runs at open and
    /// with every sweep, under the index's lock, so no item being kept
    /// can be caught half way.
    fn reconcile(&self) -> usize {
        let db = lock(&self.db);
        let named: std::collections::HashSet<String> = {
            let Ok(mut q) = db.connection().prepare("SELECT file FROM items") else {
                return 0;
            };
            let Ok(rows) = q.query_map([], |r| r.get::<_, String>(0)) else {
                return 0;
            };
            match rows.collect::<Result<_, _>>() {
                Ok(named) => named,
                Err(_) => return 0,
            }
        };
        let mut gone = 0;
        let Ok(owners) = std::fs::read_dir(self.dir.join("items")) else {
            return 0;
        };
        for owner in owners.flatten() {
            let Ok(files) = std::fs::read_dir(owner.path()) else {
                continue;
            };
            for file in files.flatten() {
                let name = format!(
                    "items/{}/{}",
                    owner.file_name().to_string_lossy(),
                    file.file_name().to_string_lossy()
                );
                if !named.contains(&name) && std::fs::remove_file(file.path()).is_ok() {
                    gone += 1;
                }
            }
            // Only an empty directory goes, so later sweeps don't keep
            // opening the folders of past recipients.
            let _ = std::fs::remove_dir(owner.path());
        }
        gone
    }

    fn sql<T>(
        &self,
        work: impl FnOnce(&forge_storage::rusqlite::Connection) -> forge_storage::rusqlite::Result<T>,
    ) -> Result<T, Refusal> {
        let db = lock(&self.db);
        work(db.connection()).map_err(|e| {
            log::warn!("mailbox: storage error: {e}");
            Refusal::Unavailable
        })
    }

    /// Registers `digests` for `owner`'s mailbox. A digest another
    /// recipient registered is refused, and nothing is registered.
    pub(crate) fn register(&self, owner: &[u8; 32], digests: &[[u8; 32]]) -> Result<(), Refusal> {
        let now = now_ms();
        let mut db = lock(&self.db);
        db.transaction(|tx| {
            for digest in digests {
                let had: Option<Vec<u8>> = tx
                    .query_row(
                        "SELECT owner FROM passes WHERE digest = ?1",
                        params![&digest[..]],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|_| Refusal::Unavailable)?;
                match had {
                    Some(o) if o.as_slice() != owner.as_slice() => return Err(Refusal::NotYours),
                    Some(_) => {}
                    None => {
                        tx.execute(
                            "INSERT INTO passes (digest, owner, since_ms) VALUES (?1, ?2, ?3)",
                            params![&digest[..], &owner[..], now],
                        )
                        .map_err(|_| Refusal::Unavailable)?;
                    }
                }
            }
            let held: u32 = tx
                .query_row(
                    "SELECT COUNT(*) FROM passes WHERE owner = ?1",
                    params![&owner[..]],
                    |r| r.get(0),
                )
                .map_err(|_| Refusal::Unavailable)?;
            if held > PASSES_HELD {
                return Err(Refusal::Full);
            }
            Ok(())
        })
    }

    /// Removes `owner`'s own `digests`; anyone else's are left alone.
    pub(crate) fn deregister(&self, owner: &[u8; 32], digests: &[[u8; 32]]) -> Result<(), Refusal> {
        self.sql(|c| {
            for digest in digests {
                c.execute(
                    "DELETE FROM passes WHERE digest = ?1 AND owner = ?2",
                    params![&digest[..], &owner[..]],
                )?;
            }
            Ok(())
        })
    }

    /// Checks a deposit from `by` before its bytes are read: the pass, the
    /// sender, the version, the life and the limits. `Ok(None)` is a
    /// duplicate, already stored.
    pub(crate) fn admit(
        &self,
        by: &[u8; 32],
        deposit: Deposit,
        now: i64,
    ) -> Result<Option<Admitted>, Refusal> {
        let Deposit {
            pass,
            kind,
            header,
            enc,
            len,
        } = deposit;
        if header.version != VERSION || enc.len() > 256 {
            return Err(Refusal::Malformed);
        }
        if header.from != *by {
            return Err(Refusal::NotYours);
        }
        if header.expires_ms <= now {
            return Err(Refusal::Expired);
        }
        if header.expires_ms > now + KEPT_MS + CLOCK_SLACK_MS {
            return Err(Refusal::TooLong);
        }
        let limit = match kind {
            Kind::Item => MAX_ITEM,
            Kind::Receipt => MAX_RECEIPT,
        };
        if len > limit {
            return Err(Refusal::TooBig);
        }
        let digest = DepositPass::from_bytes(pass).digest();
        let owner = header.to;
        let duplicate = self.sql(|c| rules(c, &owner, &digest, kind, &header, len, now))??;
        if duplicate {
            return Ok(None);
        }
        Ok(Some(Admitted {
            owner,
            pass: digest,
            kind,
            header,
            enc,
            len,
        }))
    }

    /// A new partial file for an admitted item's bytes.
    fn partial(&self) -> PathBuf {
        self.dir
            .join("partial")
            .join(hex(&crate::hex::random()[..16]))
    }

    /// A file name of its own for each item kept, so deleting an earlier
    /// copy of the same item can never remove a later one.
    fn item_file(owner: &[u8; 32], from: &[u8; 32], id: &[u8; 16]) -> String {
        format!(
            "items/{}/{}-{}-{}",
            hex(owner),
            hex(from),
            hex(id),
            hex(&crate::hex::random()[..8])
        )
    }

    /// Keeps an admitted item whose bytes are in `partial`. Every rule is
    /// checked again in the same transaction as the insert (the pass may
    /// have been deregistered, other uploads may have used the room, or the
    /// item may have expired while it arrived). A copy that arrived
    /// meanwhile wins, and this one is deleted.
    /// `now` is read once the index's lock is held, so time spent waiting
    /// for it counts.
    pub(crate) fn keep(
        &self,
        item: Admitted,
        partial: &Path,
        now: impl FnOnce() -> i64,
    ) -> Result<bool, Refusal> {
        let Admitted {
            owner,
            pass,
            kind,
            header,
            enc,
            len,
        } = item;
        let file = Self::item_file(&owner, &header.from, &header.id);
        let path = self.dir.join(&file);
        let mut db = lock(&self.db);
        let now = now();
        let stored = db.transaction(|tx| {
            if header.expires_ms <= now {
                return Err(Refusal::Expired);
            }
            if rules(tx, &owner, &pass, kind, &header, len, now).map_err(unavailable)?? {
                return Ok(false);
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(unavailable)?;
            }
            std::fs::rename(partial, &path)
                .map_err(unavailable)?;
            tx.execute(
                "INSERT INTO items (owner, sender, id, kind, pass, expires_ms, size, stored_ms, header, enc, file)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    &owner[..],
                    &header.from[..],
                    &header.id[..],
                    kind as i64,
                    &pass[..],
                    header.expires_ms,
                    len as i64,
                    now,
                    encode(&header),
                    enc,
                    file,
                ],
            )
            .map_err(unavailable)?;
            if kind == Kind::Item {
                tx.execute(
                    "INSERT INTO deposits (pass, at_ms) VALUES (?1, ?2)",
                    params![&pass[..], now],
                )
                .map_err(unavailable)?;
            }
            Ok::<_, Refusal>(true)
        });
        match stored {
            Ok(true) => Ok(true),
            Ok(false) => {
                let _ = std::fs::remove_file(partial);
                Ok(false)
            }
            Err(r) => {
                let _ = std::fs::remove_file(partial);
                let _ = std::fs::remove_file(&path);
                Err(r)
            }
        }
    }

    /// What waits for `owner`, oldest first.
    pub(crate) fn list(&self, owner: &[u8; 32], now: i64) -> Result<Vec<Waiting>, Refusal> {
        self.sql(|c| {
            let mut q = c.prepare(
                "SELECT sender, id, kind, expires_ms, size FROM items
                 WHERE owner = ?1 AND expires_ms > ?2 ORDER BY stored_ms LIMIT ?3",
            )?;
            let rows = q.query_map(params![&owner[..], now, LISTED], |r| {
                let from: Vec<u8> = r.get(0)?;
                let id: Vec<u8> = r.get(1)?;
                Ok(Waiting {
                    from: from.try_into().unwrap_or([0; 32]),
                    id: id.try_into().unwrap_or([0; 16]),
                    kind: if r.get::<_, i64>(2)? == 1 {
                        Kind::Receipt
                    } else {
                        Kind::Item
                    },
                    expires_ms: r.get(3)?,
                    size: r.get::<_, i64>(4)? as u64,
                })
            })?;
            rows.collect()
        })
    }

    /// One of `owner`'s items: its kind, header, `enc` and file.
    pub(crate) fn fetch(
        &self,
        owner: &[u8; 32],
        from: &[u8; 32],
        id: &[u8; 16],
        now: i64,
    ) -> Result<(Kind, Header, Vec<u8>, PathBuf, u64), Refusal> {
        let row = self.sql(|c| {
            c.query_row(
                "SELECT kind, header, enc, file, size FROM items
                 WHERE owner = ?1 AND sender = ?2 AND id = ?3 AND expires_ms > ?4",
                params![&owner[..], &from[..], &id[..], now],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()
        })?;
        let (kind, header, enc, file, size) = row.ok_or(Refusal::NotFound)?;
        let header: Header = decode(&header).ok_or(Refusal::Unavailable)?;
        let kind = if kind == 1 { Kind::Receipt } else { Kind::Item };
        Ok((kind, header, enc, self.dir.join(file), size as u64))
    }

    /// Deletes one of `owner`'s items. Acknowledging what isn't there is
    /// fine: a second acknowledgement after a lost answer.
    pub(crate) fn ack(
        &self,
        owner: &[u8; 32],
        from: &[u8; 32],
        id: &[u8; 16],
    ) -> Result<(), Refusal> {
        let file: Option<String> = self.sql(|c| {
            let file = c
                .query_row(
                    "SELECT file FROM items WHERE owner = ?1 AND sender = ?2 AND id = ?3",
                    params![&owner[..], &from[..], &id[..]],
                    |r| r.get(0),
                )
                .optional()?;
            c.execute(
                "DELETE FROM items WHERE owner = ?1 AND sender = ?2 AND id = ?3",
                params![&owner[..], &from[..], &id[..]],
            )?;
            Ok(file)
        })?;
        if let Some(file) = file {
            let _ = std::fs::remove_file(self.dir.join(file));
        }
        Ok(())
    }

    /// Deletes expired items unread, and deposit times older than an hour,
    /// then any file no row names. Returns how many items went.
    pub fn sweep(&self, now: i64) -> usize {
        let files: Vec<String> = self
            .sql(|c| {
                let mut q = c.prepare("SELECT file FROM items WHERE expires_ms <= ?1")?;
                let files = q
                    .query_map(params![now], |r| r.get(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                c.execute("DELETE FROM items WHERE expires_ms <= ?1", params![now])?;
                Ok(files)
            })
            .unwrap_or_default();
        for file in &files {
            let _ = std::fs::remove_file(self.dir.join(file));
        }
        let _ = self.sql(|c| {
            c.execute(
                "DELETE FROM deposits WHERE at_ms <= ?1",
                params![now - 3600 * 1000],
            )
        });
        self.reconcile();
        files.len()
    }
}

/// The rules a deposit must meet, against the index as it is now: the
/// pass is registered by the item's recipient, and (unless it is a
/// duplicate, `Ok(true)`) the limits hold. Checked when a deposit is
/// admitted and again in the transaction that keeps it.
fn rules(
    c: &forge_storage::rusqlite::Connection,
    owner: &[u8; 32],
    digest: &[u8; 32],
    kind: Kind,
    header: &Header,
    len: u64,
    now: i64,
) -> forge_storage::rusqlite::Result<Result<bool, Refusal>> {
    let registered: Option<Vec<u8>> = c
        .query_row(
            "SELECT owner FROM passes WHERE digest = ?1",
            params![&digest[..]],
            |r| r.get(0),
        )
        .optional()?;
    if registered.as_deref() != Some(&owner[..]) {
        return Ok(Err(Refusal::NoPass));
    }
    let duplicate = c
        .query_row(
            "SELECT 1 FROM items WHERE owner = ?1 AND sender = ?2 AND id = ?3",
            params![&owner[..], &header.from[..], &header.id[..]],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if duplicate {
        return Ok(Ok(true));
    }
    match kind {
        Kind::Receipt => {
            let held: u32 = c.query_row(
                "SELECT COUNT(*) FROM items WHERE owner = ?1 AND kind = 1",
                params![&owner[..]],
                |r| r.get(0),
            )?;
            if held >= RECEIPTS_HELD {
                return Ok(Err(Refusal::Full));
            }
        }
        Kind::Item => {
            let hourly: u32 = c.query_row(
                "SELECT COUNT(*) FROM deposits WHERE pass = ?1 AND at_ms > ?2",
                params![&digest[..], now - 3600 * 1000],
                |r| r.get(0),
            )?;
            if hourly >= PER_PASS_HOURLY {
                return Ok(Err(Refusal::TooMany));
            }
            let held = |column: &str, key: &[u8]| -> forge_storage::rusqlite::Result<u64> {
                c.query_row(
                    &format!(
                        "SELECT COALESCE(SUM(size), 0) FROM items WHERE {column} = ?1 AND kind = 0"
                    ),
                    params![key],
                    |r| r.get::<_, i64>(0),
                )
                .map(|n| n as u64)
            };
            if held("pass", digest)? + len > PER_PASS || held("owner", owner)? + len > PER_RECIPIENT
            {
                return Ok(Err(Refusal::Full));
            }
        }
    }
    Ok(Ok(false))
}

/// The `relay/mailbox/1` handler.
#[derive(Debug, Clone)]
pub struct MailboxServer {
    store: Arc<MailboxStore>,
    fetches: Arc<tokio::sync::Semaphore>,
}

impl MailboxServer {
    pub fn new(store: Arc<MailboxStore>) -> Self {
        Self {
            store,
            fetches: Arc::new(tokio::sync::Semaphore::new(FETCHES)),
        }
    }

    async fn answer(
        &self,
        caller: [u8; 32],
        mut send: SendStream,
        mut recv: RecvStream,
    ) -> Result<(), RelayError> {
        let request = get_within(&mut recv, STEP).await?;
        let now = now_ms();
        let reply = match decode::<Request>(&request) {
            None => Reply::Refused(Refusal::Malformed),
            Some(Request::Register { digests }) => done(self.store.register(&caller, &digests)),
            Some(Request::Deregister { digests }) => done(self.store.deregister(&caller, &digests)),
            Some(Request::List) => match self.store.list(&caller, now) {
                Ok(list) => Reply::Listed(list),
                Err(r) => Reply::Refused(r),
            },
            Some(Request::Ack { from, id }) => done(self.store.ack(&caller, &from, &id)),
            Some(Request::Fetch { from, id }) => {
                let Ok(_permit) = Arc::clone(&self.fetches).try_acquire_owned() else {
                    put(&mut send, &encode(&Reply::Refused(Refusal::Busy))).await?;
                    send.finish().ok();
                    return Ok(());
                };
                match self.store.fetch(&caller, &from, &id, now) {
                    Ok((kind, header, enc, path, len)) => {
                        match tokio::fs::File::open(&path).await {
                            Ok(file) if file.metadata().await.is_ok_and(|m| m.len() == len) => {
                                put(
                                    &mut send,
                                    &encode(&Reply::Item {
                                        kind,
                                        header,
                                        enc,
                                        len,
                                    }),
                                )
                                .await?;
                                stream_file(file, &mut send).await?;
                                send.finish().ok();
                                let _ = tokio::time::timeout(IDLE, send.stopped()).await;
                                return Ok(());
                            }
                            _ => Reply::Refused(Refusal::NotFound),
                        }
                    }
                    Err(r) => Reply::Refused(r),
                }
            }
            Some(Request::Deposit(deposit)) => {
                let len = deposit.len;
                match self.store.admit(&caller, deposit, now) {
                    Err(r) => Reply::Refused(r),
                    Ok(None) => Reply::Stored { duplicate: true },
                    Ok(Some(admitted)) => match self.store.reserve(len) {
                        Err(r) => Reply::Refused(r),
                        Ok(_room) => {
                            let partial = self.store.partial();
                            if let Err(e) = receive(&mut recv, &partial, len).await {
                                let _ = tokio::fs::remove_file(&partial).await;
                                return Err(e);
                            }
                            match self.store.keep(admitted, &partial, now_ms) {
                                Ok(stored) => Reply::Stored { duplicate: !stored },
                                Err(r) => Reply::Refused(r),
                            }
                        }
                    },
                }
            }
        };
        put(&mut send, &encode(&reply)).await?;
        send.finish().ok();
        let _ = tokio::time::timeout(STEP, send.stopped()).await;
        Ok(())
    }
}

fn done(result: Result<(), Refusal>) -> Reply {
    match result {
        Ok(()) => Reply::Done,
        Err(r) => Reply::Refused(r),
    }
}

/// Reads exactly `len` bytes from `recv` into `path`.
async fn receive(recv: &mut RecvStream, path: &Path, len: u64) -> Result<(), RelayError> {
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|e| RelayError::File {
            reason: e.to_string(),
        })?;
    let mut left = len;
    let mut buf = vec![0u8; 64 * 1024];
    while left > 0 {
        let n = left.min(buf.len() as u64) as usize;
        tokio::time::timeout(IDLE, recv.read_exact(&mut buf[..n]))
            .await
            .map_err(|_| connection("the depositor stopped sending"))?
            .map_err(connection)?;
        file.write_all(&buf[..n])
            .await
            .map_err(|e| RelayError::File {
                reason: e.to_string(),
            })?;
        left -= n as u64;
    }
    // A write error can wait for the next flush: flush first, so a short
    // file fails the upload rather than being kept.
    file.flush().await.map_err(|e| RelayError::File {
        reason: e.to_string(),
    })?;
    file.sync_all().await.map_err(|e| RelayError::File {
        reason: e.to_string(),
    })
}

/// Sends `file` in chunks, each within [`IDLE`]: a recipient that keeps
/// reading gets all of it, one that stops is let go.
async fn stream_file(mut file: tokio::fs::File, send: &mut SendStream) -> Result<(), RelayError> {
    use tokio::io::AsyncReadExt;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await.map_err(|e| RelayError::File {
            reason: e.to_string(),
        })?;
        if n == 0 {
            return Ok(());
        }
        tokio::time::timeout(IDLE, send.write_all(&buf[..n]))
            .await
            .map_err(|_| connection("the recipient stopped reading"))?
            .map_err(connection)?;
    }
}

impl ProtocolHandler for MailboxServer {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let caller = *conn.remote_id().as_bytes();
        loop {
            let Ok((send, recv)) = conn.accept_bi().await else {
                break;
            };
            let server = self.clone();
            tokio::spawn(async move {
                if let Err(e) = server.answer(caller, send, recv).await {
                    log::info!("mailbox: a request ended: {e}");
                }
            });
        }
        Ok(())
    }
}

/// Why a call to the mailbox didn't do what was asked.
// The client in step 3 calls it; until then only the tests do.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CallError {
    #[error("the mailbox refused: {0}")]
    Refused(Refusal),
    #[error("{0}")]
    Connection(String),
}

impl From<RelayError> for CallError {
    fn from(e: RelayError) -> Self {
        CallError::Connection(e.to_string())
    }
}

/// A connection to a mailbox server, as its caller's endpoint.
// The client in step 3 calls it; until then only the tests do.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct MailboxClient {
    conn: Connection,
}

#[cfg_attr(not(test), allow(dead_code))]
impl MailboxClient {
    pub(crate) async fn connect(
        endpoint: &Endpoint,
        server: impl Into<EndpointAddr>,
    ) -> Result<Self, CallError> {
        let conn = tokio::time::timeout(STEP, endpoint.connect(server, MAILBOX_ALPN))
            .await
            .map_err(|_| CallError::Connection("the mailbox didn't answer".into()))?
            .map_err(|e| CallError::Connection(e.to_string()))?;
        Ok(Self { conn })
    }

    pub(crate) fn close(&self) {
        self.conn.close(0u32.into(), b"bye");
    }

    async fn ask(
        &self,
        request: &Request,
        body: Option<&[u8]>,
    ) -> Result<(Reply, RecvStream), CallError> {
        let (mut send, mut recv) = self.conn.open_bi().await.map_err(connection)?;
        put(&mut send, &encode(request)).await?;
        if let Some(body) = body {
            // A server that refuses (or already has the item) answers
            // without reading the body, which can stop this write: its
            // answer is still read below.
            if let Err(e) = send.write_all(body).await {
                log::info!("mailbox: the body wasn't all taken: {e}");
            }
        }
        send.finish().ok();
        let reply = get_within(&mut recv, IDLE).await?;
        match decode::<Reply>(&reply) {
            Some(Reply::Refused(r)) => Err(CallError::Refused(r)),
            Some(reply) => Ok((reply, recv)),
            None => Err(CallError::Connection(
                "an answer this build can't read".into(),
            )),
        }
    }

    async fn simple(&self, request: Request) -> Result<(), CallError> {
        match self.ask(&request, None).await?.0 {
            Reply::Done => Ok(()),
            other => Err(CallError::Connection(format!(
                "unexpected answer {other:?}"
            ))),
        }
    }

    pub(crate) async fn register(&self, digests: Vec<[u8; 32]>) -> Result<(), CallError> {
        self.simple(Request::Register { digests }).await
    }

    pub(crate) async fn deregister(&self, digests: Vec<[u8; 32]>) -> Result<(), CallError> {
        self.simple(Request::Deregister { digests }).await
    }

    pub(crate) async fn ack(&self, from: [u8; 32], id: [u8; 16]) -> Result<(), CallError> {
        self.simple(Request::Ack { from, id }).await
    }

    /// Leaves `item` under `pass`. Returns true when the mailbox already
    /// had it.
    pub(crate) async fn deposit(
        &self,
        pass: &DepositPass,
        kind: Kind,
        item: &SealedItem,
    ) -> Result<bool, CallError> {
        let request = Request::Deposit(Deposit {
            pass: pass.bytes(),
            kind,
            header: item.header.clone(),
            enc: item.enc.clone(),
            len: item.ciphertext.len() as u64,
        });
        match self.ask(&request, Some(&item.ciphertext)).await?.0 {
            Reply::Stored { duplicate } => Ok(duplicate),
            other => Err(CallError::Connection(format!(
                "unexpected answer {other:?}"
            ))),
        }
    }

    pub(crate) async fn list(&self) -> Result<Vec<Waiting>, CallError> {
        match self.ask(&Request::List, None).await?.0 {
            Reply::Listed(list) => Ok(list),
            other => Err(CallError::Connection(format!(
                "unexpected answer {other:?}"
            ))),
        }
    }

    pub(crate) async fn fetch(
        &self,
        from: [u8; 32],
        id: [u8; 16],
    ) -> Result<(Kind, SealedItem), CallError> {
        let (reply, mut recv) = self.ask(&Request::Fetch { from, id }, None).await?;
        let Reply::Item {
            kind,
            header,
            enc,
            len,
        } = reply
        else {
            return Err(CallError::Connection(format!(
                "unexpected answer {reply:?}"
            )));
        };
        if len > MAX_ITEM {
            return Err(CallError::Connection(
                "an item bigger than the mailbox takes".into(),
            ));
        }
        // Idle time, not the whole transfer, is limited: a large item on a
        // slow link finishes as long as it keeps moving.
        let mut ciphertext = vec![0u8; len as usize];
        for chunk in ciphertext.chunks_mut(64 * 1024) {
            tokio::time::timeout(IDLE, recv.read_exact(chunk))
                .await
                .map_err(|_| CallError::Connection("the mailbox stopped sending".into()))?
                .map_err(connection)?;
        }
        Ok((
            kind,
            SealedItem {
                header,
                enc,
                ciphertext,
            },
        ))
    }
}

/// How the `relay-mailbox` binary runs.
#[derive(Debug, Clone, Default)]
pub struct ServeOptions {
    /// Where the index, the item files and the server's key live.
    pub dir: PathBuf,
    /// The UDP port to listen on (0: any).
    pub port: u16,
    /// Relay's relay server, so devices behind NAT reach it.
    pub relay: Option<url::Url>,
    /// Relay's lookup server, where it publishes its relay address. Needs
    /// `relay`.
    pub lookup: Option<url::Url>,
}

/// Runs a mailbox server until the process is stopped: the endpoint, the
/// protocol, and a sweep of expired items every ten minutes. Prints the
/// endpoint id devices dial it by. Its key is `device.key` in `dir`.
pub fn serve(options: ServeOptions) -> Result<std::convert::Infallible, RelayError> {
    use iroh::endpoint::presets;
    use iroh::RelayMode;
    // The lookup server publishes relay addresses only: without a relay,
    // it would publish nothing devices could dial.
    if options.lookup.is_some() && options.relay.is_none() {
        return Err(RelayError::Connection {
            reason: "--lookup needs --relay: the lookup server publishes the relay address".into(),
        });
    }
    crate::code::TOKIO.block_on(async move {
        std::fs::create_dir_all(&options.dir).map_err(|e| RelayError::Storage {
            reason: e.to_string(),
        })?;
        let key = crate::node::device_key(&options.dir)?;
        let store = Arc::new(MailboxStore::open(&options.dir)?);
        let mut builder = Endpoint::builder(presets::Minimal)
            .secret_key(key)
            .relay_mode(match &options.relay {
                Some(url) => {
                    RelayMode::Custom(iroh::RelayMap::from(iroh::RelayUrl::from(url.clone())))
                }
                None => RelayMode::Disabled,
            });
        if let Some(url) = &options.lookup {
            builder = builder.address_lookup(iroh::address_lookup::pkarr::PkarrPublisher::builder(
                url.clone(),
            ));
        }
        if options.port != 0 {
            builder = builder
                .clear_ip_transports()
                .bind_addr(std::net::SocketAddr::from((
                    std::net::Ipv4Addr::UNSPECIFIED,
                    options.port,
                )))
                .map_err(connection)?;
        }
        let endpoint = builder.bind().await.map_err(connection)?;
        let router = iroh::protocol::Router::builder(endpoint.clone())
            .accept(MAILBOX_ALPN, MailboxServer::new(Arc::clone(&store)))
            .spawn();
        println!("relay-mailbox {}", endpoint.id());
        // Runs until the process is stopped; the router serves meanwhile.
        let _router = router;
        loop {
            let gone = store.sweep(now_ms());
            if gone > 0 {
                log::info!("mailbox: {gone} expired items deleted unread");
            }
            tokio::time::sleep(Duration::from_secs(600)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::{open, seal, MailboxKey};
    use iroh::endpoint::presets;
    use iroh::protocol::Router;
    use iroh::{RelayMode, SecretKey};

    const SOON: i64 = 24 * 3600 * 1000;

    fn key(n: u8) -> SecretKey {
        SecretKey::from_bytes(&[n; 32])
    }

    async fn endpoint(n: u8) -> Endpoint {
        Endpoint::builder(presets::Minimal)
            .secret_key(key(n))
            .relay_mode(RelayMode::Disabled)
            .bind()
            .await
            .unwrap()
    }

    struct World {
        _dir: tempfile::TempDir,
        store: Arc<MailboxStore>,
        server: Router,
    }

    async fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MailboxStore::open(dir.path()).unwrap());
        let server = Router::builder(endpoint(100).await)
            .accept(MAILBOX_ALPN, MailboxServer::new(Arc::clone(&store)))
            .spawn();
        World {
            _dir: dir,
            store,
            server,
        }
    }

    async fn client(world: &World, n: u8) -> (Endpoint, MailboxClient) {
        let ep = endpoint(n).await;
        let c = MailboxClient::connect(&ep, world.server.endpoint().addr())
            .await
            .unwrap();
        (ep, c)
    }

    fn item(from: u8, to: u8, id: u8, payload: &[u8]) -> SealedItem {
        let to_key = MailboxKey::from_seed([to; 32]);
        seal(
            &key(from),
            &key(to).public(),
            &to_key.public(),
            [id; 16],
            now_ms() + SOON,
            payload,
        )
        .unwrap()
    }

    fn me(n: u8) -> [u8; 32] {
        *key(n).public().as_bytes()
    }

    /// A laptop (1) leaves an item for a closed phone (2); the phone
    /// collects, opens and acknowledges it, and the server deletes it.
    #[test]
    fn an_item_left_for_a_closed_device_is_collected_once() {
        crate::code::TOKIO.block_on(async {
            let world = world().await;
            let pass = DepositPass::from_bytes([42; 32]);
            let (_p, phone) = client(&world, 2).await;
            phone.register(vec![pass.digest()]).await.unwrap();
            phone.close();
            let (_l, laptop) = client(&world, 1).await;
            let sealed = item(1, 2, 7, b"a photo, privately");
            assert_eq!(laptop.deposit(&pass, Kind::Item, &sealed).await, Ok(false));
            assert_eq!(
                laptop.deposit(&pass, Kind::Item, &sealed).await,
                Ok(true),
                "stored once"
            );

            // On the server, only ciphertext.
            let (_, _, _, path, _) = world
                .store
                .fetch(&me(2), &me(1), &[7; 16], now_ms())
                .unwrap();
            let stored = std::fs::read(path).unwrap();
            assert!(!stored.windows(9).any(|w| w == b"privately"));

            let (_p, phone) = client(&world, 2).await;
            let waiting = phone.list().await.unwrap();
            assert_eq!(waiting.len(), 1);
            assert_eq!((waiting[0].from, waiting[0].id), (me(1), [7; 16]));
            let (kind, fetched) = phone.fetch(me(1), [7; 16]).await.unwrap();
            assert_eq!(kind, Kind::Item);
            let opened = open(
                &MailboxKey::from_seed([2; 32]),
                &key(2).public(),
                &fetched,
                now_ms(),
                |f| *f == key(1).public(),
            )
            .unwrap();
            assert_eq!(opened.payload, b"a photo, privately");
            phone.ack(me(1), [7; 16]).await.unwrap();
            assert!(phone.list().await.unwrap().is_empty());
            assert_eq!(
                phone.fetch(me(1), [7; 16]).await.unwrap_err(),
                CallError::Refused(Refusal::NotFound)
            );
            world.server.shutdown().await.unwrap();
        });
    }

    /// The recipient is always the caller: a device can't register a pass
    /// that opens someone else's mailbox, use one pass for another
    /// recipient, claim another sender, or see, fetch or delete anyone
    /// else's items.
    #[test]
    fn nobody_acts_for_another_device() {
        crate::code::TOKIO.block_on(async {
            let world = world().await;
            let (_l, laptop) = client(&world, 1).await;
            let (_p, phone) = client(&world, 2).await;
            let (_t, tablet) = client(&world, 3).await;

            // The laptop registers a pass "for the phone": it is the
            // laptop's own, so it doesn't open the phone's mailbox.
            let forged = DepositPass::from_bytes([9; 32]);
            laptop.register(vec![forged.digest()]).await.unwrap();
            assert_eq!(
                laptop
                    .deposit(&forged, Kind::Item, &item(1, 2, 1, b"x"))
                    .await,
                Err(CallError::Refused(Refusal::NoPass))
            );
            // And the phone can't take over a digest someone registered.
            assert_eq!(
                phone.register(vec![forged.digest()]).await,
                Err(CallError::Refused(Refusal::NotYours))
            );

            // The phone's pass, used for the tablet, is refused.
            let pass = DepositPass::from_bytes([10; 32]);
            phone.register(vec![pass.digest()]).await.unwrap();
            assert_eq!(
                laptop
                    .deposit(&pass, Kind::Item, &item(1, 3, 2, b"x"))
                    .await,
                Err(CallError::Refused(Refusal::NoPass))
            );
            // The tablet can't leave an item that says it is the laptop's.
            assert_eq!(
                tablet
                    .deposit(&pass, Kind::Item, &item(1, 2, 3, b"x"))
                    .await,
                Err(CallError::Refused(Refusal::NotYours))
            );

            // An item for the phone: only the phone lists, fetches or
            // deletes it.
            laptop
                .deposit(&pass, Kind::Item, &item(1, 2, 4, b"x"))
                .await
                .unwrap();
            assert!(tablet.list().await.unwrap().is_empty());
            assert_eq!(
                tablet.fetch(me(1), [4; 16]).await.unwrap_err(),
                CallError::Refused(Refusal::NotFound)
            );
            tablet.ack(me(1), [4; 16]).await.unwrap();
            laptop.ack(me(1), [4; 16]).await.unwrap();
            assert_eq!(phone.list().await.unwrap().len(), 1, "still there");

            // Only the phone removes its pass, and then it opens nothing.
            tablet.deregister(vec![pass.digest()]).await.unwrap();
            laptop
                .deposit(&pass, Kind::Item, &item(1, 2, 5, b"x"))
                .await
                .unwrap();
            phone.deregister(vec![pass.digest()]).await.unwrap();
            assert_eq!(
                laptop
                    .deposit(&pass, Kind::Item, &item(1, 2, 6, b"x"))
                    .await,
                Err(CallError::Refused(Refusal::NoPass))
            );
            world.server.shutdown().await.unwrap();
        });
    }

    fn header(from: u8, to: u8, id: u8, expires_ms: i64) -> Header {
        Header {
            version: VERSION,
            id: [id; 16],
            to: me(to),
            from: me(from),
            expires_ms,
        }
    }

    /// Admits and keeps an item of `len` bytes (the file itself is tiny:
    /// the quotas count what the deposit declared).
    fn leave(
        store: &MailboxStore,
        pass: &DepositPass,
        kind: Kind,
        h: Header,
        len: u64,
    ) -> Result<bool, Refusal> {
        let now = now_ms();
        let by = h.from;
        let deposit = Deposit {
            pass: pass.bytes(),
            kind,
            header: h,
            enc: vec![0; 32],
            len,
        };
        let Some(admitted) = store.admit(&by, deposit, now)? else {
            return Ok(false);
        };
        let partial = store.partial();
        std::fs::write(&partial, b"sealed").unwrap();
        store.keep(admitted, &partial, || now)
    }

    fn admit(store: &MailboxStore, pass: &DepositPass, h: Header, len: u64) -> Admitted {
        let by = h.from;
        let deposit = Deposit {
            pass: pass.bytes(),
            kind: Kind::Item,
            header: h,
            enc: vec![0; 32],
            len,
        };
        store.admit(&by, deposit, now_ms()).unwrap().unwrap()
    }

    fn keep(store: &MailboxStore, admitted: Admitted, now: i64) -> Result<bool, Refusal> {
        let partial = store.partial();
        std::fs::write(&partial, b"sealed").unwrap();
        let kept = store.keep(admitted, &partial, || now);
        if kept.is_err() {
            assert!(!partial.exists(), "a refused upload leaves no file");
        }
        kept
    }

    /// What was true when an upload began is checked again when it ends:
    /// room used by uploads that finished first, a pass deregistered, an
    /// item that expired on the way.
    #[test]
    fn the_rules_are_checked_again_when_an_upload_ends() {
        let dir = tempfile::tempdir().unwrap();
        let store = MailboxStore::open(dir.path()).unwrap();
        let now = now_ms();
        let mb = 1024 * 1024;
        let pass = DepositPass::from_bytes([1; 32]);
        store.register(&me(2), &[pass.digest()]).unwrap();
        assert_eq!(
            leave(
                &store,
                &pass,
                Kind::Item,
                header(1, 2, 1, now + SOON),
                100 * mb
            ),
            Ok(true)
        );
        // Two more 100 MB uploads both fit when they begin...
        let first = admit(&store, &pass, header(1, 2, 2, now + SOON), 100 * mb);
        let second = admit(&store, &pass, header(1, 2, 3, now + SOON), 100 * mb);
        // ...but only one fits when they end.
        assert_eq!(keep(&store, first, now), Ok(true));
        assert_eq!(keep(&store, second, now), Err(Refusal::Full));

        let late = admit(&store, &pass, header(1, 2, 4, now + 1000), 1);
        assert_eq!(keep(&store, late, now + 1000), Err(Refusal::Expired));

        let revoked = admit(&store, &pass, header(1, 2, 5, now + SOON), 1);
        store.deregister(&me(2), &[pass.digest()]).unwrap();
        assert_eq!(keep(&store, revoked, now), Err(Refusal::NoPass));
    }

    #[test]
    fn uploads_in_progress_are_limited_and_given_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = MailboxStore::open(dir.path()).unwrap();
        let big = store.reserve(UPLOAD_BYTES).unwrap();
        assert_eq!(
            store.reserve(1).err(),
            Some(Refusal::Busy),
            "all the bytes are held"
        );
        drop(big);
        let held: Vec<_> = (0..UPLOADS).map(|_| store.reserve(1).unwrap()).collect();
        assert_eq!(
            store.reserve(1).err(),
            Some(Refusal::Busy),
            "all the slots are held"
        );
        drop(held);
        assert!(store.reserve(UPLOAD_BYTES).is_ok());
    }

    /// A file no row names (a stop between deleting a row and its file)
    /// is deleted by the next sweep; an item kept again after an
    /// acknowledgement gets a file of its own.
    #[test]
    fn files_left_behind_are_cleaned_up() {
        let dir = tempfile::tempdir().unwrap();
        let store = MailboxStore::open(dir.path()).unwrap();
        let now = now_ms();
        let pass = DepositPass::from_bytes([1; 32]);
        store.register(&me(2), &[pass.digest()]).unwrap();
        assert_eq!(
            leave(&store, &pass, Kind::Item, header(1, 2, 1, now + SOON), 6),
            Ok(true)
        );
        let (_, _, _, first, _) = store.fetch(&me(2), &me(1), &[1; 16], now).unwrap();
        store.ack(&me(2), &me(1), &[1; 16]).unwrap();
        assert_eq!(
            leave(&store, &pass, Kind::Item, header(1, 2, 1, now + SOON), 6),
            Ok(true)
        );
        let (_, _, _, second, _) = store.fetch(&me(2), &me(1), &[1; 16], now).unwrap();
        assert_ne!(first, second);

        let stray = dir
            .path()
            .join("items")
            .join(hex(&me(2)))
            .join("left-behind");
        std::fs::write(&stray, b"x").unwrap();
        assert_eq!(store.sweep(now), 0);
        assert!(!stray.exists());
        assert!(second.exists(), "a live item's file stays");

        store.ack(&me(2), &me(1), &[1; 16]).unwrap();
        store.sweep(now);
        assert!(
            !dir.path().join("items").join(hex(&me(2))).exists(),
            "an empty recipient folder goes"
        );
    }

    #[test]
    fn one_store_per_directory() {
        let dir = tempfile::tempdir().unwrap();
        let store = MailboxStore::open(dir.path()).unwrap();
        assert!(MailboxStore::open(dir.path()).is_err());
        drop(store);
        assert!(MailboxStore::open(dir.path()).is_ok());
    }

    #[test]
    fn the_server_rules_hold() {
        let dir = tempfile::tempdir().unwrap();
        let store = MailboxStore::open(dir.path()).unwrap();
        let now = now_ms();
        let pass = DepositPass::from_bytes([1; 32]);
        let other = DepositPass::from_bytes([2; 32]);
        store
            .register(&me(2), &[pass.digest(), other.digest()])
            .unwrap();
        let mb = 1024 * 1024;

        // Life: not already over, and no longer than the mailbox keeps.
        assert_eq!(
            leave(&store, &pass, Kind::Item, header(1, 2, 1, now - 1), 10),
            Err(Refusal::Expired)
        );
        assert_eq!(
            leave(
                &store,
                &pass,
                Kind::Item,
                header(1, 2, 1, now + KEPT_MS + 2 * CLOCK_SLACK_MS),
                10
            ),
            Err(Refusal::TooLong)
        );
        // Size: 100 MB an item, 4 KB a receipt.
        assert_eq!(
            leave(
                &store,
                &pass,
                Kind::Item,
                header(1, 2, 1, now + SOON),
                MAX_ITEM + 1
            ),
            Err(Refusal::TooBig)
        );
        assert_eq!(
            leave(
                &store,
                &pass,
                Kind::Receipt,
                header(1, 2, 1, now + SOON),
                5000
            ),
            Err(Refusal::TooBig)
        );
        // A newer version isn't stored.
        let mut newer = header(1, 2, 1, now + SOON);
        newer.version = VERSION + 1;
        assert_eq!(
            leave(&store, &pass, Kind::Item, newer, 10),
            Err(Refusal::Malformed)
        );

        // 256 MB under one pass: two 100 MB items fit, a third doesn't.
        assert_eq!(
            leave(
                &store,
                &pass,
                Kind::Item,
                header(1, 2, 1, now + SOON),
                100 * mb
            ),
            Ok(true)
        );
        assert_eq!(
            leave(
                &store,
                &pass,
                Kind::Item,
                header(1, 2, 2, now + SOON),
                100 * mb
            ),
            Ok(true)
        );
        assert_eq!(
            leave(
                &store,
                &pass,
                Kind::Item,
                header(1, 2, 3, now + SOON),
                100 * mb
            ),
            Err(Refusal::Full)
        );
        // Another sender's pass still has room; and a receipt always fits.
        assert_eq!(
            leave(
                &store,
                &other,
                Kind::Item,
                header(3, 2, 3, now + SOON),
                100 * mb
            ),
            Ok(true)
        );
        assert_eq!(
            leave(
                &store,
                &pass,
                Kind::Receipt,
                header(1, 2, 9, now + SOON),
                100
            ),
            Ok(true)
        );
        // A duplicate is stored once.
        assert_eq!(
            leave(
                &store,
                &pass,
                Kind::Item,
                header(1, 2, 1, now + SOON),
                100 * mb
            ),
            Ok(false)
        );
        assert_eq!(store.list(&me(2), now).unwrap().len(), 4);

        // 200 deposits an hour under one pass.
        let busy = DepositPass::from_bytes([3; 32]);
        store.register(&me(4), &[busy.digest()]).unwrap();
        for n in 0..PER_PASS_HOURLY {
            let mut h = header(1, 4, 0, now + SOON);
            h.id[..4].copy_from_slice(&n.to_be_bytes());
            assert_eq!(leave(&store, &busy, Kind::Item, h, 1), Ok(true));
        }
        assert_eq!(
            leave(&store, &busy, Kind::Item, header(1, 4, 0xEE, now + SOON), 1),
            Err(Refusal::TooMany)
        );

        // Expired items are deleted unread.
        assert_eq!(store.sweep(now + SOON + 1), 4 + PER_PASS_HOURLY as usize);
        assert!(store.list(&me(2), now).unwrap().is_empty());
        assert!(
            !dir.path().join("items").join(hex(&me(2))).exists(),
            "its files and then its folder are gone"
        );
    }
}
