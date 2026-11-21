//! Relay's one database (review P3, docs/relay-audit-2026-11.md): this
//! device's record (trusted devices, deliveries, received ids, the
//! history) and every device conversation, in one SQLite file,
//! `relay.db`, on forge-storage's [`Database`].
//!
//! - **The record stays a value in memory** ([`Record`]), as the rules
//!   and the screens use it, and a change writes only the rows it changed
//!   ([`save`]): a delivery, a device, a history entry, not the whole
//!   document. What isn't a list (identity, policy, mode, servers, flags)
//!   is one row of `meta`.
//! - **Device conversations are rows** (`messages`, with each
//!   conversation's last stamp in `conversations` and where attachments
//!   are on this device in `attachments`), read by query, never held whole.
//! - **What belongs together is one transaction.** A change to the record
//!   and to conversations made in one [`crate::State::change_with`] is
//!   committed together: receiving an item and remembering its id, queueing
//!   a message and its delivery, ending a delivery and setting its message's
//!   status, and the move from the JSON files. A crash between them can no
//!   longer show a delivered message as failed.
//! - **Moving from the JSON files** ([`Store::open`]) happens once, in one
//!   transaction: `relay.json` and each `devices/<id>/messages.json` are
//!   read, written here, then renamed `*.migrated` (kept, read-only, for
//!   one release). Relay ending half way leaves the JSON files where they
//!   were, and the next open moves them again.
//! - **A record that can't be read** (a damaged database, or a
//!   `relay.json` that doesn't parse) is set aside, never deleted, and
//!   Relay starts again. The device key is a separate file, so the device
//!   keeps its identity; only its trusted devices and history are lost.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chat_core::{Message, MessageStatus};
use forge_runtime::{lock, now_ms};
use forge_storage::rusqlite::{self, params, Connection, OptionalExtension};
use forge_storage::{Database, JsonFile, Opened, StorageError};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::device_chat::{device_conversation, LogFile};
use crate::model::RelayError;
use crate::record::Record;

/// The database's file in Relay's data directory.
pub(crate) const DB_FILE: &str = "relay.db";
/// The JSON record it replaced.
pub(crate) const JSON_FILE: &str = "relay.json";
/// What a moved JSON file is renamed to (kept for one release).
const MIGRATED: &str = "migrated";

/// The schema, one step per version.
const MIGRATIONS: &[&str] = &["
    -- What isn't a list: identity, policy, mode, servers, flags.
    CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    -- The record's lists, each row its saved JSON, in the order added.
    CREATE TABLE devices (key TEXT PRIMARY KEY, seq INTEGER NOT NULL, body TEXT NOT NULL);
    CREATE TABLE deliveries (key TEXT PRIMARY KEY, seq INTEGER NOT NULL, body TEXT NOT NULL);
    CREATE TABLE received (key TEXT PRIMARY KEY, seq INTEGER NOT NULL, body TEXT NOT NULL);
    CREATE TABLE history (key TEXT PRIMARY KEY, seq INTEGER NOT NULL, body TEXT NOT NULL);
    -- Device conversations.
    CREATE TABLE conversations (id TEXT PRIMARY KEY, last_at INTEGER NOT NULL);
    CREATE TABLE messages (
        conversation TEXT NOT NULL,
        id TEXT NOT NULL,
        sent_at INTEGER NOT NULL,
        changed_at INTEGER NOT NULL,
        queued INTEGER NOT NULL,
        body TEXT NOT NULL,
        PRIMARY KEY (conversation, id)
    );
    CREATE INDEX messages_by_time ON messages (conversation, sent_at);
    CREATE INDEX messages_by_change ON messages (conversation, changed_at);
    CREATE TABLE attachments (
        conversation TEXT NOT NULL,
        id TEXT NOT NULL,
        path TEXT NOT NULL,
        PRIMARY KEY (conversation, id)
    );
"];

/// The record's lists, by table: each row's key, and its value.
const LISTS: [&str; 4] = ["devices", "deliveries", "received", "history"];

impl From<StorageError> for RelayError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::UnsupportedSchemaVersion { .. } => Self::NewerData,
            StorageError::CorruptData { reason } => Self::Corrupt { reason },
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

impl From<rusqlite::Error> for RelayError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

fn json<T: Serialize>(value: &T) -> Result<String, RelayError> {
    serde_json::to_string(value).map_err(|e| RelayError::Storage {
        reason: e.to_string(),
    })
}

fn parse<T: DeserializeOwned>(text: &str) -> Result<T, RelayError> {
    serde_json::from_str(text).map_err(|e| RelayError::Corrupt {
        reason: e.to_string(),
    })
}

/// How the store found Relay's data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Found {
    /// Nothing saved yet.
    Nothing,
    /// The database, as saved.
    Saved,
    /// The JSON files, moved in now. `not_moved` are device conversations
    /// that couldn't be read: left where they were, for the person to know.
    Moved { not_moved: Vec<PathBuf> },
    /// What was saved couldn't be read; it was set aside at `kept`, and
    /// Relay starts again (the device key is kept).
    SetAside { kept: PathBuf, reason: String },
}

/// What the person should hear about how Relay's data was found, if
/// anything: what couldn't be read, and where it is kept.
pub(crate) fn notice(found: &Found) -> Option<crate::record::SavedNotice> {
    let path = |p: &PathBuf| p.to_string_lossy().into_owned();
    match found {
        Found::SetAside { kept, .. } => Some(crate::record::SavedNotice {
            message: "Relay's saved data couldn't be read, so it started again. This device \
                      kept its identity, but its trusted devices and history need setting up \
                      again. The old data is kept on this device."
                .into(),
            kept: vec![path(kept)],
        }),
        Found::Moved { not_moved } if !not_moved.is_empty() => {
            let count = not_moved.len();
            let which = if count == 1 {
                "One device conversation".to_string()
            } else {
                format!("{count} device conversations")
            };
            Some(crate::record::SavedNotice {
                message: format!(
                    "Relay now keeps everything in one database. {which} couldn't be read, \
                     so {} not moved in and won't show in Chats. {} kept on this device as \
                     {} were.",
                    if count == 1 { "it was" } else { "they were" },
                    if count == 1 { "It is" } else { "They are" },
                    if count == 1 { "it" } else { "they" },
                ),
                kept: not_moved.iter().map(path).collect(),
            })
        }
        _ => None,
    }
}

/// Relay's database.
pub(crate) struct Store {
    db: Mutex<Database>,
}

impl Store {
    /// Opens (or creates) `relay.db` in `dir`, moving the JSON files in on
    /// the first open after the update. Returns the record as saved
    /// (`None` when there is none yet), and how it was found.
    pub fn open(dir: &Path) -> Result<(Self, Option<Record>, Found), RelayError> {
        let (db, opened) = Database::open(dir.join(DB_FILE), MIGRATIONS)?;
        let store = Self { db: Mutex::new(db) };
        let mut found = match opened {
            Opened::Recovered { moved_to, reason } => {
                forge_runtime::logging::warn!(
                    "relay's database couldn't be read and was set aside: {reason}"
                );
                Found::SetAside {
                    kept: moved_to,
                    reason,
                }
            }
            Opened::Created | Opened::Existing => Found::Nothing,
        };
        if let Some(record) = store.load()? {
            if found == Found::Nothing {
                found = Found::Saved;
            }
            // JSON files the move didn't get to rename (Relay ended just
            // after it): renamed now, so they are never read again.
            let json = dir.join(JSON_FILE);
            if json.exists() {
                for (endpoint, _) in read_json_logs(&dir.join("devices")).0 {
                    let file = dir.join("devices").join(endpoint).join("messages.json");
                    let _ = std::fs::rename(&file, file.with_extension(format!("json.{MIGRATED}")));
                }
                let _ = std::fs::rename(&json, json.with_extension(format!("json.{MIGRATED}")));
            }
            return Ok((store, Some(record), found));
        }
        // Nothing in the database: the JSON files, if any, move in now.
        let json = dir.join(JSON_FILE);
        if !json.exists() {
            return Ok((store, None, found));
        }
        let record = match JsonFile::<Record>::new(&json, crate::record::VERSION).load() {
            Ok(record) => record.unwrap_or_default(),
            Err(StorageError::UnsupportedSchemaVersion { .. }) => {
                return Err(RelayError::NewerData)
            }
            Err(err) => {
                let reason = err.to_string();
                let kept = set_aside(&json)?;
                forge_runtime::logging::warn!(
                    "relay's saved record couldn't be read and was set aside: {reason}"
                );
                return Ok((store, None, Found::SetAside { kept, reason }));
            }
        };
        let (logs, not_moved) = read_json_logs(&dir.join("devices"));
        store.write(|tx| {
            save(tx, &Record::default(), &record)?;
            for (endpoint, log) in &logs {
                put_log(tx, &device_conversation(endpoint), log)?;
            }
            Ok(())
        })?;
        // Only once the database holds them: kept, but never read again.
        for (endpoint, _) in &logs {
            let file = dir.join("devices").join(endpoint).join("messages.json");
            let _ = std::fs::rename(&file, file.with_extension(format!("json.{MIGRATED}")));
        }
        let _ = std::fs::rename(&json, json.with_extension(format!("json.{MIGRATED}")));
        forge_runtime::logging::info!(
            "relay moved its record and {} device conversations into its database",
            logs.len()
        );
        Ok((store, Some(record), Found::Moved { not_moved }))
    }

    /// A store in memory, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self {
            db: Mutex::new(Database::in_memory(MIGRATIONS).expect("an in-memory database")),
        }
    }

    /// Runs `work` in one transaction: all of it is saved, or none.
    pub fn write<T>(
        &self,
        work: impl FnOnce(&Connection) -> Result<T, RelayError>,
    ) -> Result<T, RelayError> {
        lock(&self.db).transaction(|tx| work(tx))
    }

    /// Runs `work` on the connection, for reads.
    pub fn read<T>(&self, work: impl FnOnce(&Connection) -> T) -> T {
        work(lock(&self.db).connection())
    }

    /// The record as saved, or `None` when nothing is.
    fn load(&self) -> Result<Option<Record>, RelayError> {
        self.read(|c| {
            let Some(meta) = c
                .query_row("SELECT value FROM meta WHERE key = 'record'", [], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?
            else {
                return Ok(None);
            };
            let mut record: Record = parse(&meta)?;
            record.devices.list = rows(c, "devices")?;
            record.deliveries = rows(c, "deliveries")?;
            record.received_ids = rows(c, "received")?;
            record.transfers = rows(c, "history")?;
            Ok(Some(record))
        })
    }
}

/// A list's rows, in the order they were added.
fn rows<T: DeserializeOwned>(c: &Connection, table: &str) -> Result<Vec<T>, RelayError> {
    debug_assert!(LISTS.contains(&table));
    let mut statement = c.prepare(&format!("SELECT body FROM {table} ORDER BY seq"))?;
    let bodies = statement.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for body in bodies {
        out.push(parse(&body?)?);
    }
    Ok(out)
}

/// Saves what changed from `before` to `after`: changed and new rows of
/// each list, rows that left it, and `meta` if it changed. A row keeps its
/// place; a new one goes after the rest, as the lists only ever push to
/// the end.
pub(crate) fn save(c: &Connection, before: &Record, after: &Record) -> Result<(), RelayError> {
    // Everything but the lists, so a field added to the record later is
    // saved without a change here.
    let shell = |r: &Record| Record {
        devices: crate::trust::Devices {
            list: Vec::new(),
            ..r.devices.clone()
        },
        transfers: Vec::new(),
        deliveries: Vec::new(),
        received_ids: Vec::new(),
        ..r.clone()
    };
    let meta = shell(after);
    if meta != shell(before) || before == &Record::default() {
        c.execute(
            "INSERT INTO meta (key, value) VALUES ('record', ?1)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [json(&meta)?],
        )?;
    }
    sync(
        c,
        "devices",
        &before.devices.list,
        &after.devices.list,
        |d| d.endpoint_id.clone(),
    )?;
    sync(
        c,
        "deliveries",
        &before.deliveries,
        &after.deliveries,
        |d| d.id.clone(),
    )?;
    sync(
        c,
        "received",
        &before.received_ids,
        &after.received_ids,
        |r| format!("{}/{}", r.from, r.id),
    )?;
    sync(c, "history", &before.transfers, &after.transfers, |t| {
        t.id.to_string()
    })?;
    Ok(())
}

fn sync<T: Serialize + PartialEq>(
    c: &Connection,
    table: &str,
    before: &[T],
    after: &[T],
    key: impl Fn(&T) -> String,
) -> Result<(), RelayError> {
    debug_assert!(LISTS.contains(&table));
    let was: HashMap<String, &T> = before.iter().map(|t| (key(t), t)).collect();
    let mut kept = std::collections::HashSet::new();
    let mut next: Option<i64> = None;
    for item in after {
        let k = key(item);
        let unchanged = was.get(&k).is_some_and(|old| *old == item);
        kept.insert(k.clone());
        if unchanged {
            continue;
        }
        if was.contains_key(&k) {
            c.execute(
                &format!("UPDATE {table} SET body = ?2 WHERE key = ?1"),
                params![k, json(item)?],
            )?;
        } else {
            let seq = match next {
                Some(seq) => seq,
                None => c.query_row(
                    &format!("SELECT COALESCE(MAX(seq), 0) + 1 FROM {table}"),
                    [],
                    |r| r.get(0),
                )?,
            };
            next = Some(seq + 1);
            c.execute(
                &format!(
                    "INSERT INTO {table} (key, seq, body) VALUES (?1, ?2, ?3)
                     ON CONFLICT (key) DO UPDATE SET seq = excluded.seq, body = excluded.body"
                ),
                params![k, seq, json(item)?],
            )?;
        }
    }
    for k in was.keys().filter(|k| !kept.contains(*k)) {
        c.execute(&format!("DELETE FROM {table} WHERE key = ?1"), [k])?;
    }
    Ok(())
}

/// A conversation's messages and attachment locations from a JSON log.
fn put_log(c: &Connection, conversation: &str, log: &LogFile) -> Result<(), RelayError> {
    let mut last = log.last_at;
    for m in &log.messages {
        let mut m = m.clone();
        m.conversation_id = conversation.to_string();
        last = last.max(m.changed_at());
        insert_message(c, conversation, &m)?;
    }
    set_last(c, conversation, last)?;
    for (id, path) in &log.files {
        set_file(c, conversation, id, Path::new(path))?;
    }
    Ok(())
}

/// Every device conversation's JSON log under `dir`. One that can't be
/// read is left where it is (and logged): its messages aren't moved.
fn read_json_logs(dir: &Path) -> (Vec<(String, LogFile)>, Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (Vec::new(), Vec::new());
    };
    let (mut logs, mut unreadable) = (Vec::new(), Vec::new());
    for entry in entries.flatten() {
        let endpoint = entry.file_name().to_string_lossy().into_owned();
        let file = entry.path().join("messages.json");
        if !file.exists() {
            continue;
        }
        match JsonFile::<LogFile>::new(&file, crate::device_chat::LOG_VERSION).load() {
            Ok(Some(log)) => logs.push((endpoint, log)),
            Ok(None) => {}
            Err(err) => {
                forge_runtime::logging::warn!(
                    "relay couldn't read a device conversation to move it: {err}"
                );
                unreadable.push(file);
            }
        }
    }
    logs.sort_by(|a, b| a.0.cmp(&b.0));
    unreadable.sort();
    (logs, unreadable)
}

/// Renames `path` aside, with the time, and returns where it went.
fn set_aside(path: &Path) -> Result<PathBuf, RelayError> {
    let kept = path.with_extension(format!("json.unreadable-{}", now_ms()));
    std::fs::rename(path, &kept).map_err(|e| RelayError::Storage {
        reason: e.to_string(),
    })?;
    Ok(kept)
}

// Device conversations. Each takes a connection, so a write is part of
// whatever transaction it is in.

fn insert_message(c: &Connection, conversation: &str, m: &Message) -> Result<(), RelayError> {
    c.execute(
        "INSERT INTO messages (conversation, id, sent_at, changed_at, queued, body)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (conversation, id) DO UPDATE SET
             sent_at = excluded.sent_at, changed_at = excluded.changed_at,
             queued = excluded.queued, body = excluded.body",
        params![
            conversation,
            m.id,
            m.sent_at,
            m.changed_at(),
            m.status == MessageStatus::Queued,
            json(m)?
        ],
    )?;
    Ok(())
}

fn last_at(c: &Connection, conversation: &str) -> Result<i64, RelayError> {
    Ok(c.query_row(
        "SELECT last_at FROM conversations WHERE id = ?1",
        [conversation],
        |r| r.get(0),
    )
    .optional()?
    .unwrap_or(0))
}

fn set_last(c: &Connection, conversation: &str, at: i64) -> Result<(), RelayError> {
    c.execute(
        "INSERT INTO conversations (id, last_at) VALUES (?1, ?2)
         ON CONFLICT (id) DO UPDATE SET last_at = MAX(last_at, excluded.last_at)",
        params![conversation, at],
    )?;
    Ok(())
}

/// A new stamp for `conversation`: now, or after its newest, so stamps
/// only ever increase.
fn stamp(c: &Connection, conversation: &str) -> Result<i64, RelayError> {
    let at = now_ms().max(last_at(c, conversation)? + 1);
    set_last(c, conversation, at)?;
    Ok(at)
}

/// The messages a query finds. A query that fails, or a row that doesn't
/// read as a message, is an error, never a message left out: history
/// doesn't quietly lose a message.
fn messages(
    c: &Connection,
    sql: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Message>, RelayError> {
    let mut statement = c.prepare(sql)?;
    let rows = statement.query_map(args, |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for body in rows {
        out.push(parse::<Message>(&body?).map_err(|err| RelayError::Corrupt {
            reason: format!("a saved message can't be read ({err})"),
        })?);
    }
    Ok(out)
}

/// One message of a conversation.
pub(crate) fn message(
    c: &Connection,
    conversation: &str,
    id: &str,
) -> Result<Option<Message>, RelayError> {
    Ok(messages(
        c,
        "SELECT body FROM messages WHERE conversation = ?1 AND id = ?2",
        &[&conversation, &id],
    )?
    .pop())
}

/// Messages changed after `since` (all without it), in the order shown
/// (by time).
pub(crate) fn since(
    c: &Connection,
    conversation: &str,
    since: Option<i64>,
) -> Result<Vec<Message>, RelayError> {
    match since {
        None => messages(
            c,
            "SELECT body FROM messages WHERE conversation = ?1 ORDER BY sent_at, id",
            &[&conversation],
        ),
        Some(since) => messages(
            c,
            "SELECT body FROM messages WHERE conversation = ?1 AND changed_at > ?2
             ORDER BY sent_at, id",
            &[&conversation, &since],
        ),
    }
}

/// The newest message of a conversation.
pub(crate) fn last(c: &Connection, conversation: &str) -> Result<Option<Message>, RelayError> {
    Ok(messages(
        c,
        "SELECT body FROM messages WHERE conversation = ?1 ORDER BY sent_at DESC, id DESC LIMIT 1",
        &[&conversation],
    )?
    .pop())
}

/// Our messages still waiting in the outbox.
pub(crate) fn queued(c: &Connection, conversation: &str) -> Result<Vec<Message>, RelayError> {
    messages(
        c,
        "SELECT body FROM messages WHERE conversation = ?1 AND queued = 1 ORDER BY sent_at, id",
        &[&conversation],
    )
}

/// Adds `message`, stamped now, unless one with its id is there: then that
/// one comes back and nothing changes (the bool says whether it was
/// added). It is `Sent` unless it is one of ours `Queued` in the outbox.
pub(crate) fn add(
    c: &Connection,
    conversation: &str,
    mut message: Message,
) -> Result<(Message, bool), RelayError> {
    if let Some(found) = self::message(c, conversation, &message.id)? {
        return Ok((found, false));
    }
    message.conversation_id = conversation.to_string();
    if message.status != MessageStatus::Queued {
        message.status = MessageStatus::Sent;
    }
    message.updated_at = None;
    message.sent_at = stamp(c, conversation)?;
    insert_message(c, conversation, &message)?;
    Ok((message, true))
}

/// Moves one of our messages on (`Sent`, `Failed`, or `Queued` again),
/// with a new change stamp, so catch-up after `since` sees it. `None` when
/// it isn't there or already has that status.
pub(crate) fn set_status(
    c: &Connection,
    conversation: &str,
    id: &str,
    status: MessageStatus,
) -> Result<Option<Message>, RelayError> {
    let Some(mut m) = message(c, conversation, id)? else {
        return Ok(None);
    };
    if m.status == status {
        return Ok(None);
    }
    m.status = status;
    m.updated_at = Some(stamp(c, conversation)?);
    insert_message(c, conversation, &m)?;
    Ok(Some(m))
}

/// Gives every message authored `from` the author `to`, each with a new
/// stamp so catch-up after `since` sees it. Returns what changed.
pub(crate) fn reauthor(
    c: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<(String, Message)>, RelayError> {
    let mut found = Vec::new();
    {
        let mut statement = c.prepare("SELECT conversation, body FROM messages")?;
        let rows =
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (conversation, body) = row?;
            let m = parse::<Message>(&body).map_err(|err| RelayError::Corrupt {
                reason: format!("a saved message can't be read ({err})"),
            })?;
            if m.author_id == from {
                found.push((conversation, m));
            }
        }
    }
    let mut changed = Vec::new();
    for (conversation, mut m) in found {
        m.author_id = to.to_string();
        m.updated_at = Some(stamp(c, &conversation)?);
        insert_message(c, &conversation, &m)?;
        changed.push((conversation, m));
    }
    Ok(changed)
}

/// Adds a message moved from elsewhere (the history, before notes were
/// messages), keeping its time, unless its id is there. Its change is
/// stamped now, so catch-up after `since` sees it.
pub(crate) fn import(
    c: &Connection,
    conversation: &str,
    mut message: Message,
    path: Option<&Path>,
) -> Result<Option<Message>, RelayError> {
    if self::message(c, conversation, &message.id)?.is_some() {
        return Ok(None);
    }
    message.conversation_id = conversation.to_string();
    message.updated_at = Some(stamp(c, conversation)?);
    if let (Some(a), Some(path)) = (message.attachments.first(), path) {
        set_file(c, conversation, &a.id, path)?;
    }
    insert_message(c, conversation, &message)?;
    Ok(Some(message))
}

/// Keeps where attachment `id` is on this device.
pub(crate) fn set_file(
    c: &Connection,
    conversation: &str,
    id: &str,
    path: &Path,
) -> Result<(), RelayError> {
    c.execute(
        "INSERT INTO attachments (conversation, id, path) VALUES (?1, ?2, ?3)
         ON CONFLICT (conversation, id) DO UPDATE SET path = excluded.path",
        params![conversation, id, path.to_string_lossy()],
    )?;
    Ok(())
}

/// Where attachment `id` is on this device, as kept (it may be gone).
pub(crate) fn file(c: &Connection, conversation: &str, id: &str) -> Option<PathBuf> {
    c.query_row(
        "SELECT path FROM attachments WHERE conversation = ?1 AND id = ?2",
        [conversation, id],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outbox::{Payload, ReceivedId, SavedDelivery};
    use crate::record::{SavedItem, SavedStatus};

    fn delivery(id: &str) -> SavedDelivery {
        SavedDelivery {
            id: id.into(),
            to: "phone".into(),
            transfer: 0,
            payload: Payload::Note { text: id.into() },
            queued_ms: 1,
        }
    }

    fn reload(store: &Store) -> Record {
        store.load().unwrap().unwrap()
    }

    #[test]
    fn a_change_saves_only_what_changed_and_reads_back_the_same() {
        let store = Store::in_memory();
        let mut record = Record {
            device_id: "me".into(),
            name: "laptop".into(),
            ..Record::default()
        };
        record.devices.trust("phone", "Phone", 1);
        record.devices.trust("tablet", "Tablet", 2);
        record.deliveries = vec![delivery("a"), delivery("b"), delivery("c")];
        record.begin("phone", true, SavedItem::Note { text: "x".into() }, 3);
        store
            .write(|c| save(c, &Record::default(), &record))
            .unwrap();
        assert_eq!(reload(&store), record);

        // The middle delivery ends, a new one goes after the rest, a device
        // changes, a history entry finishes, an id is remembered.
        let mut next = record.clone();
        next.deliveries.remove(1);
        next.deliveries.push(delivery("d"));
        next.devices.revoke("phone");
        next.finish(1, SavedStatus::Done, None);
        next.received_ids.push(ReceivedId {
            from: "phone".into(),
            id: "ab".into(),
            at_ms: 4,
        });
        next.name = "create".into();
        store.write(|c| save(c, &record, &next)).unwrap();
        let back = reload(&store);
        assert_eq!(back, next);
        let ids: Vec<_> = back.deliveries.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["a", "c", "d"]);
        let rows: i64 = store.read(|c| {
            c.query_row("SELECT COUNT(*) FROM deliveries", [], |r| r.get(0))
                .unwrap()
        });
        assert_eq!(rows, 3);
    }

    #[test]
    fn a_failed_transaction_saves_nothing() {
        let store = Store::in_memory();
        let record = Record {
            device_id: "me".into(),
            ..Record::default()
        };
        let conversation = device_conversation("phone");
        let failed: Result<(), RelayError> = store.write(|c| {
            save(c, &Record::default(), &record)?;
            add(c, &conversation, msg("m"))?;
            Err(RelayError::Storage {
                reason: "crash".into(),
            })
        });
        assert!(failed.is_err());
        assert!(store.load().unwrap().is_none());
        assert!(store
            .read(|c| since(c, &conversation, None))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn conversations_stamp_in_order_and_add_each_id_once() {
        let store = Store::in_memory();
        let conv = device_conversation("abc123");
        let (a, added) = store.write(|c| add(c, &conv, msg("a"))).unwrap();
        assert!(added);
        let (b, _) = store.write(|c| add(c, &conv, msg("b"))).unwrap();
        assert!(b.sent_at > a.sent_at);
        assert_eq!(a.status, MessageStatus::Sent);
        let (again, added) = store.write(|c| add(c, &conv, msg("a"))).unwrap();
        assert!(!added);
        assert_eq!(again, a);
        assert_eq!(
            store.read(|c| since(c, &conv, None)).unwrap(),
            vec![a.clone(), b.clone()]
        );
        assert_eq!(
            store.read(|c| since(c, &conv, Some(a.sent_at))).unwrap(),
            vec![b.clone()]
        );
        assert_eq!(store.read(|c| last(c, &conv)).unwrap(), Some(b));
        let other = device_conversation("other");
        assert!(store.read(|c| since(c, &other, None)).unwrap().is_empty());
    }

    #[test]
    fn own_messages_change_author_with_a_new_stamp_and_others_dont() {
        let store = Store::in_memory();
        let conv = device_conversation("abc123");
        let own = |id: &str| Message {
            author_id: "relay:old".into(),
            ..msg(id)
        };
        let (a, _) = store.write(|c| add(c, &conv, own("a"))).unwrap();
        let (b, _) = store.write(|c| add(c, &conv, msg("b"))).unwrap();
        let changed = store
            .write(|c| reauthor(c, "relay:old", "relay:me"))
            .unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].0, conv);
        // Catch-up after the newest message sees the change, once.
        let seen = store
            .read(|c| since(c, &conv, Some(b.changed_at())))
            .unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].id, a.id);
        assert_eq!(seen[0].author_id, "relay:me");
        let all = store.read(|c| since(c, &conv, None)).unwrap();
        assert_eq!(all[1].author_id, b.author_id, "the other side's are left");
        // A second run changes nothing.
        assert!(store
            .write(|c| reauthor(c, "relay:old", "relay:me"))
            .unwrap()
            .is_empty());
    }

    /// An older build's record and one conversation, as it saved them.
    fn older_build(dir: &Path) -> Record {
        let mut record = Record {
            device_id: "me".into(),
            name: "laptop".into(),
            notes_are_messages: true,
            ..Record::default()
        };
        record.devices.trust("phone", "Phone", 1);
        record.deliveries = vec![delivery("a")];
        JsonFile::<Record>::new(dir.join(JSON_FILE), crate::record::VERSION)
            .save(&record)
            .unwrap();
        let log = LogFile {
            messages: vec![msg("m1"), msg("m2")],
            last_at: 2,
            ..LogFile::default()
        };
        let file = dir.join("devices").join("phone").join("messages.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        JsonFile::<LogFile>::new(file, crate::device_chat::LOG_VERSION)
            .save(&log)
            .unwrap();
        record
    }

    #[test]
    fn a_move_interrupted_half_way_is_done_again_whole() {
        let dir = tempfile::tempdir().unwrap();
        let record = older_build(dir.path());
        // Relay ends in the middle of the move: the record and part of the
        // conversation written, nothing committed.
        {
            let (db, _) = Database::open(dir.path().join(DB_FILE), MIGRATIONS).unwrap();
            let store = Store { db: Mutex::new(db) };
            let conv = device_conversation("phone");
            let ended: Result<(), RelayError> = store.write(|c| {
                save(c, &Record::default(), &record)?;
                insert_message(c, &conv, &msg("m1"))?;
                Err(RelayError::Stopped {
                    kind: forge_app::ErrorKind::ShutDown,
                })
            });
            assert!(ended.is_err());
        }
        assert!(
            dir.path().join(JSON_FILE).exists(),
            "the files are untouched"
        );

        // The next open moves everything, once.
        let (store, saved, found) = Store::open(dir.path()).unwrap();
        assert_eq!(found, Found::Moved { not_moved: vec![] });
        assert_eq!(saved.as_ref(), Some(&record));
        let conv = device_conversation("phone");
        let ids: Vec<String> = store
            .read(|c| since(c, &conv, None))
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, ["m1", "m2"]);
        drop(store);
        let (_, again, found) = Store::open(dir.path()).unwrap();
        assert_eq!(found, Found::Saved);
        assert_eq!(again, Some(record));
    }

    #[test]
    fn a_damaged_message_is_an_error_not_a_gap() {
        let store = Store::in_memory();
        let conv = device_conversation("abc123");
        store.write(|c| add(c, &conv, msg("a"))).unwrap();
        store.write(|c| add(c, &conv, msg("b"))).unwrap();
        store
            .write(|c| {
                c.execute("UPDATE messages SET body = '{' WHERE id = 'a'", [])?;
                Ok(())
            })
            .unwrap();
        let err = store.read(|c| since(c, &conv, None)).unwrap_err();
        assert!(
            matches!(&err, RelayError::Corrupt { reason } if reason.contains("saved message")),
            "{err:?}"
        );
        assert!(store.read(|c| message(c, &conv, "a")).is_err());
    }

    fn msg(id: &str) -> Message {
        Message {
            id: id.into(),
            body: id.into(),
            ..Message::default()
        }
    }
}
