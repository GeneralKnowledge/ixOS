//! The chat store: every conversation, the messages of each, the
//! per-conversation `since` cursor and the drafts, in one SQLite database
//! (`forge-chat.db`, through `forge_storage::Database`).
//!
//! The client works on a [`ChatSnapshot`] in memory and calls
//! [`ChatStore::save`] after every change; the store writes only what
//! changed since the last save, in one transaction, so a kill at any point
//! leaves the previous or the next state, never half of one. Each
//! conversation keeps at most [`MAX_MESSAGES_PER_CONVERSATION`] messages.
//! Message bodies are indexed for full-text search ([`ChatStore::search`]).
//!
//! Builds before M2 kept a JSON snapshot (`forge-chat-store.json`, shape
//! below). The first open of the database imports it once and renames it to
//! `forge-chat-store.json.imported`.
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "conversations": [ { "id": "general", "title": "General", "last_message_at": 1730000000000, "unread_count": 0 } ],
//!   "messages": { "general": [ { "id": "...", "conversation_id": "general", "author": "me", "body": "hi", "sent_at": 1730000000000, "status": "sent" } ] },
//!   "last_synced": { "general": 1730000000000 }
//! }
//! ```

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use forge_storage::rusqlite;
use forge_storage::{Database, Opened, StorageError};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::types::{Conversation, Message, MessageStatus};

/// Schema version this build writes.
pub const STORE_VERSION: u32 = 1;

/// The snapshot's file name. Stays the same across versions so a newer
/// build finds (and migrates) what an older one wrote.
pub const STORE_FILENAME: &str = "forge-chat-store.json";

/// Older messages beyond this are dropped from the store (not the server).
pub const MAX_MESSAGES_PER_CONVERSATION: usize = 500;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("stored chat data is invalid: {reason}")]
    Corrupt { reason: String },
    #[error("stored chat data uses schema version {found}; this build supports up to {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("chat store I/O failed: {reason}")]
    Io { reason: String },
}

/// Everything the client keeps on the device.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSnapshot {
    pub schema_version: u32,
    /// In the order the server listed them.
    pub conversations: Vec<Conversation>,
    /// Per conversation, sorted by `sent_at` (then id).
    pub messages: BTreeMap<String, Vec<Message>>,
    /// Per conversation: the greatest server `sent_at` such that every
    /// message up to it is in `messages`. The next fetch asks for
    /// `since = last_synced`.
    pub last_synced: BTreeMap<String, i64>,
    /// Per conversation: the unsent text in its composer.
    #[serde(default)]
    pub drafts: BTreeMap<String, String>,
}

/// What [`ChatSnapshot::upsert_message`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upsert {
    /// The id was new.
    Inserted,
    /// The id existed and its status changed (other fields may have too).
    StatusChanged,
    /// The id existed and content changed without a status change.
    Updated,
    /// Nothing changed.
    Unchanged,
}

impl ChatSnapshot {
    pub fn new() -> Self {
        Self {
            schema_version: STORE_VERSION,
            ..Self::default()
        }
    }

    pub fn conversation(&self, id: &str) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.id == id)
    }

    pub fn conversation_mut(&mut self, id: &str) -> Option<&mut Conversation> {
        self.conversations.iter_mut().find(|c| c.id == id)
    }

    pub fn messages(&self, conversation_id: &str) -> &[Message] {
        self.messages
            .get(conversation_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn message(&self, id: &str) -> Option<&Message> {
        self.messages.values().flatten().find(|m| m.id == id)
    }

    pub fn last_synced(&self, conversation_id: &str) -> Option<i64> {
        self.last_synced.get(conversation_id).copied()
    }

    /// Raises the cursor (never lowers it).
    pub fn advance_cursor(&mut self, conversation_id: &str, sent_at: i64) {
        let cursor = self
            .last_synced
            .entry(conversation_id.to_string())
            .or_insert(sent_at);
        *cursor = (*cursor).max(sent_at);
    }

    /// Inserts or replaces the message with this id, keeping the list sorted
    /// and capped.
    pub fn upsert_message(&mut self, message: Message) -> Upsert {
        let list = self
            .messages
            .entry(message.conversation_id.clone())
            .or_default();
        let id = message.id.clone();
        let outcome = match list.iter_mut().find(|m| m.id == message.id) {
            Some(existing) if *existing == message => return Upsert::Unchanged,
            Some(existing) => {
                let status_changed = existing.status != message.status;
                *existing = message;
                if status_changed {
                    Upsert::StatusChanged
                } else {
                    Upsert::Updated
                }
            }
            None => {
                list.push(message);
                Upsert::Inserted
            }
        };
        list.sort_by(|a, b| a.sent_at.cmp(&b.sent_at).then_with(|| a.id.cmp(&b.id)));
        // Evict the oldest *sent* messages: unsent ones are the user's and
        // must survive until they are sent or retried.
        while list.len() > MAX_MESSAGES_PER_CONVERSATION {
            let Some(oldest_sent) = list.iter().position(|m| m.status == MessageStatus::Sent)
            else {
                break;
            };
            list.remove(oldest_sent);
        }
        // A message older than the whole window is not kept, and so must not
        // be announced either (or a re-delivery would announce it again).
        if outcome == Upsert::Inserted && !list.iter().any(|m| m.id == id) {
            return Upsert::Unchanged;
        }
        outcome
    }

    /// Sets a message's status. `None` if the id is unknown.
    pub fn set_status(&mut self, id: &str, status: MessageStatus) -> Option<Message> {
        let message = self.messages.values_mut().flatten().find(|m| m.id == id)?;
        message.status = status;
        Some(message.clone())
    }

    /// Removes a message (an unsent one the user deleted). The removed
    /// message, if there was one.
    pub fn remove_message(&mut self, id: &str) -> Option<Message> {
        for list in self.messages.values_mut() {
            if let Some(index) = list.iter().position(|m| m.id == id) {
                return Some(list.remove(index));
            }
        }
        None
    }

    /// Messages still in the outbox (`Sending`), oldest first.
    pub fn outbox(&self) -> Vec<Message> {
        let mut queued: Vec<Message> = self
            .messages
            .values()
            .flatten()
            .filter(|m| m.status == MessageStatus::Sending)
            .cloned()
            .collect();
        queued.sort_by_key(|m| m.sent_at);
        queued
    }
}

/// Upgrades stored JSON of any known version to the current
/// [`ChatSnapshot`]. Only v1 exists, so this is where v2 will go: add an
/// arm that rewrites a v1 value into v2's shape and falls through.
pub fn migrate(old: Value) -> Result<ChatSnapshot, StoreError> {
    let version = old
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| StoreError::Corrupt {
            reason: "missing schema_version".into(),
        })?;
    let version = u32::try_from(version).map_err(|_| StoreError::Corrupt {
        reason: format!("schema_version {version} out of range"),
    })?;
    match version {
        STORE_VERSION => serde_json::from_value(old).map_err(|e| StoreError::Corrupt {
            reason: e.to_string(),
        }),
        // 1 => migrate(v1_to_v2(old)?),
        found if found > STORE_VERSION => Err(StoreError::UnsupportedVersion {
            found,
            supported: STORE_VERSION,
        }),
        other => Err(StoreError::Corrupt {
            reason: format!("unknown schema_version {other}"),
        }),
    }
}

/// The database's file name.
pub const DATABASE_FILENAME: &str = "forge-chat.db";

/// The database schema: step `i` upgrades version `i` to `i + 1`.
const SCHEMA: &[&str] = &[
    // 1: conversations in list order, messages (the whole message as JSON,
    // plus the columns queries need), cursors, drafts, and a full-text
    // index over message bodies kept in step by triggers.
    "CREATE TABLE conversations (
         id TEXT PRIMARY KEY,
         position INTEGER NOT NULL,
         data TEXT NOT NULL
     );
     CREATE TABLE messages (
         id TEXT PRIMARY KEY,
         conversation_id TEXT NOT NULL,
         sent_at INTEGER NOT NULL,
         deleted INTEGER NOT NULL DEFAULT 0,
         body TEXT NOT NULL,
         data TEXT NOT NULL
     );
     CREATE INDEX messages_by_room ON messages (conversation_id, sent_at, id);
     CREATE TABLE cursors (
         conversation_id TEXT PRIMARY KEY,
         last_synced INTEGER NOT NULL
     );
     CREATE TABLE drafts (
         conversation_id TEXT PRIMARY KEY,
         body TEXT NOT NULL
     );
     CREATE VIRTUAL TABLE messages_fts USING fts5(
         body, content = 'messages', content_rowid = 'rowid',
         tokenize = 'unicode61 remove_diacritics 2'
     );
     CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
         INSERT INTO messages_fts (rowid, body) VALUES (new.rowid, new.body);
     END;
     CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
         INSERT INTO messages_fts (messages_fts, rowid, body) VALUES ('delete', old.rowid, old.body);
     END;
     CREATE TRIGGER messages_au AFTER UPDATE OF body ON messages BEGIN
         INSERT INTO messages_fts (messages_fts, rowid, body) VALUES ('delete', old.rowid, old.body);
         INSERT INTO messages_fts (rowid, body) VALUES (new.rowid, new.body);
     END;",
];

/// Most results [`ChatStore::search`] returns.
pub const MAX_SEARCH_RESULTS: usize = 100;

/// Where the chat data lives. Cheap to clone; clones share one database.
#[derive(Clone)]
pub struct ChatStore {
    directory: Option<PathBuf>,
    open: Arc<Mutex<Option<Backend>>>,
}

impl std::fmt::Debug for ChatStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatStore")
            .field("directory", &self.directory)
            .finish()
    }
}

/// The open database, and what it holds as of the last save.
struct Backend {
    database: Database,
    saved: HashMap<String, Message>,
}

fn io(context: &str, error: impl std::fmt::Display) -> StoreError {
    StoreError::Io {
        reason: format!("{context}: {error}"),
    }
}

impl From<StorageError> for StoreError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::UnsupportedSchemaVersion { found, supported } => {
                StoreError::UnsupportedVersion { found, supported }
            }
            StorageError::CorruptData { reason } => StoreError::Corrupt { reason },
            other => StoreError::Io {
                reason: other.to_string(),
            },
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        io("database", error)
    }
}

impl ChatStore {
    /// A store that lives in memory only (tests, or a host that passes no
    /// directory). It is still SQLite, so search works the same.
    pub fn in_memory() -> Self {
        Self {
            directory: None,
            open: Arc::default(),
        }
    }

    /// A store in `directory` (created when first saved).
    pub fn in_directory(directory: impl AsRef<Path>) -> Self {
        Self {
            directory: Some(directory.as_ref().to_path_buf()),
            open: Arc::default(),
        }
    }

    /// Where this store keeps attachment files: an `attachments` folder
    /// next to the database, or a fresh temporary folder for a store in
    /// memory. Signing out (deleting the account's folder) removes them.
    pub fn attachments(&self) -> crate::AttachmentStore {
        match &self.directory {
            Some(directory) => crate::AttachmentStore::new(directory.join("attachments")),
            None => crate::AttachmentStore::temporary(),
        }
    }

    /// The database file, or `None` in memory.
    pub fn path(&self) -> Option<PathBuf> {
        self.directory.as_ref().map(|d| d.join(DATABASE_FILENAME))
    }

    /// Every file the store may have written (the database, SQLite's
    /// journal files, and a JSON store from before M2), for sign-out.
    pub fn files(&self) -> Vec<PathBuf> {
        let Some(directory) = &self.directory else {
            return Vec::new();
        };
        [
            DATABASE_FILENAME.to_string(),
            format!("{DATABASE_FILENAME}-wal"),
            format!("{DATABASE_FILENAME}-shm"),
            STORE_FILENAME.to_string(),
            format!("{STORE_FILENAME}.imported"),
        ]
        .iter()
        .map(|name| directory.join(name))
        .collect()
    }

    /// Opens the database (first call) and reads everything into a
    /// snapshot. A database that was damaged is set aside and replaced by
    /// an empty one: this returns [`StoreError::Corrupt`] once, and later
    /// saves go to the new database. A database from a newer build is
    /// [`StoreError::UnsupportedVersion`] and is never written to.
    pub fn load(&self) -> Result<ChatSnapshot, StoreError> {
        let mut open = self.lock();
        let mut warning = None;
        if open.is_none() {
            let (backend, problem) = self.open_backend()?;
            *open = Some(backend);
            warning = problem;
        }
        let backend = open.as_mut().expect("opened above");
        let snapshot = read_snapshot(&backend.database)?;
        backend.saved = snapshot
            .messages
            .values()
            .flatten()
            .map(|m| (m.id.clone(), m.clone()))
            .collect();
        match warning {
            Some(problem) => Err(problem),
            None => Ok(snapshot),
        }
    }

    /// Writes what changed since the last save or load, in one transaction.
    pub fn save(&self, snapshot: &ChatSnapshot) -> Result<(), StoreError> {
        let mut open = self.lock();
        if open.is_none() {
            let (backend, _) = self.open_backend()?;
            *open = Some(backend);
        }
        let backend = open.as_mut().expect("opened above");
        let current: HashMap<&str, &Message> = snapshot
            .messages
            .values()
            .flatten()
            .map(|m| (m.id.as_str(), m))
            .collect();
        let saved = &backend.saved;
        backend
            .database
            .transaction(|tx| write_snapshot(tx, snapshot, saved, &current))?;
        backend.saved = current
            .into_iter()
            .map(|(id, m)| (id.to_string(), m.clone()))
            .collect();
        Ok(())
    }

    /// Stored messages matching every word of `query` (each as a prefix,
    /// ignoring case and accents), newest first, in one conversation or in
    /// all of them. Deleted messages never match.
    pub fn search(
        &self,
        query: &str,
        conversation_id: Option<&str>,
    ) -> Result<Vec<Message>, StoreError> {
        let Some(pattern) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let mut open = self.lock();
        if open.is_none() {
            let (backend, _) = self.open_backend()?;
            *open = Some(backend);
        }
        let database = &open.as_ref().expect("opened above").database;
        let mut statement = database.connection().prepare(
            "SELECT messages.data FROM messages_fts
             JOIN messages ON messages.rowid = messages_fts.rowid
             WHERE messages_fts MATCH ?1
               AND messages.deleted = 0
               AND (?2 IS NULL OR messages.conversation_id = ?2)
             ORDER BY messages.sent_at DESC
             LIMIT ?3",
        )?;
        let rows = statement.query_map(
            rusqlite::params![pattern, conversation_id, MAX_SEARCH_RESULTS as i64],
            |row| row.get::<_, String>(0),
        )?;
        let mut found = Vec::new();
        for row in rows {
            found.push(decode::<Message>(&row?)?);
        }
        Ok(found)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Backend>> {
        self.open.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Opens the database, importing a JSON store from before M2 into a
    /// new one. The second value is a problem worth reporting (a damaged
    /// database that was replaced, or a JSON store that could not be read).
    fn open_backend(&self) -> Result<(Backend, Option<StoreError>), StoreError> {
        let Some(directory) = &self.directory else {
            let database = Database::in_memory(SCHEMA)?;
            return Ok((
                Backend {
                    database,
                    saved: HashMap::new(),
                },
                None,
            ));
        };
        let (database, opened) = Database::open(directory.join(DATABASE_FILENAME), SCHEMA)?;
        let mut backend = Backend {
            database,
            saved: HashMap::new(),
        };
        let problem = match opened {
            Opened::Existing => None,
            Opened::Recovered { moved_to, reason } => Some(StoreError::Corrupt {
                reason: format!(
                    "the chat database was damaged ({reason}); it was moved to {}",
                    moved_to.display()
                ),
            }),
            Opened::Created => self.import_json(directory, &mut backend).err(),
        };
        Ok((backend, problem))
    }

    /// Moves a JSON store from before M2 into the new database, then
    /// renames the file so it is imported only once.
    fn import_json(&self, directory: &Path, backend: &mut Backend) -> Result<(), StoreError> {
        let path = directory.join(STORE_FILENAME);
        let raw = match fs::read(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(io(&format!("read {}", path.display()), err)),
        };
        let value: Value = serde_json::from_slice(&raw).map_err(|e| StoreError::Corrupt {
            reason: e.to_string(),
        })?;
        let snapshot = migrate(value)?;
        let current: HashMap<&str, &Message> = snapshot
            .messages
            .values()
            .flatten()
            .map(|m| (m.id.as_str(), m))
            .collect();
        let saved = HashMap::new();
        backend
            .database
            .transaction(|tx| write_snapshot(tx, &snapshot, &saved, &current))?;
        let imported = directory.join(format!("{STORE_FILENAME}.imported"));
        fs::rename(&path, &imported).map_err(|e| io("set the imported JSON store aside", e))
    }
}

fn decode<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, StoreError> {
    serde_json::from_str(json).map_err(|e| StoreError::Corrupt {
        reason: e.to_string(),
    })
}

fn encode<T: Serialize>(value: &T) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(|e| io("encode", e))
}

/// Turns what the user typed into an FTS5 query: every word must match, as
/// a prefix. Quotes keep FTS5 syntax characters literal. `None` when there
/// is nothing to search for.
fn fts_query(query: &str) -> Option<String> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

fn read_snapshot(database: &Database) -> Result<ChatSnapshot, StoreError> {
    let connection = database.connection();
    let mut snapshot = ChatSnapshot::new();
    let mut statement = connection.prepare("SELECT data FROM conversations ORDER BY position")?;
    for row in statement.query_map([], |row| row.get::<_, String>(0))? {
        snapshot.conversations.push(decode(&row?)?);
    }
    let mut statement =
        connection.prepare("SELECT data FROM messages ORDER BY conversation_id, sent_at, id")?;
    for row in statement.query_map([], |row| row.get::<_, String>(0))? {
        let message: Message = decode(&row?)?;
        snapshot
            .messages
            .entry(message.conversation_id.clone())
            .or_default()
            .push(message);
    }
    let mut statement = connection.prepare("SELECT conversation_id, last_synced FROM cursors")?;
    for row in statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })? {
        let (id, cursor) = row?;
        snapshot.last_synced.insert(id, cursor);
    }
    let mut statement = connection.prepare("SELECT conversation_id, body FROM drafts")?;
    for row in statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (id, body) = row?;
        snapshot.drafts.insert(id, body);
    }
    Ok(snapshot)
}

/// Writes `snapshot` over what the database held (`saved`): conversations,
/// cursors and drafts in full (they are few), messages only where they
/// changed.
fn write_snapshot(
    tx: &rusqlite::Transaction<'_>,
    snapshot: &ChatSnapshot,
    saved: &HashMap<String, Message>,
    current: &HashMap<&str, &Message>,
) -> Result<(), StoreError> {
    tx.execute("DELETE FROM conversations", [])?;
    for (position, conversation) in snapshot.conversations.iter().enumerate() {
        tx.execute(
            "INSERT INTO conversations (id, position, data) VALUES (?1, ?2, ?3)",
            rusqlite::params![conversation.id, position as i64, encode(conversation)?],
        )?;
    }
    tx.execute("DELETE FROM cursors", [])?;
    for (id, cursor) in &snapshot.last_synced {
        tx.execute(
            "INSERT INTO cursors (conversation_id, last_synced) VALUES (?1, ?2)",
            rusqlite::params![id, cursor],
        )?;
    }
    tx.execute("DELETE FROM drafts", [])?;
    for (id, body) in &snapshot.drafts {
        tx.execute(
            "INSERT INTO drafts (conversation_id, body) VALUES (?1, ?2)",
            rusqlite::params![id, body],
        )?;
    }
    for id in saved.keys().filter(|id| !current.contains_key(id.as_str())) {
        tx.execute("DELETE FROM messages WHERE id = ?1", [id])?;
    }
    for (id, message) in current {
        if saved.get(*id) == Some(*message) {
            continue;
        }
        tx.execute(
            "INSERT INTO messages (id, conversation_id, sent_at, deleted, body, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET
                 conversation_id = excluded.conversation_id,
                 sent_at = excluded.sent_at,
                 deleted = excluded.deleted,
                 body = excluded.body,
                 data = excluded.data",
            rusqlite::params![
                id,
                message.conversation_id,
                message.sent_at,
                message.deleted,
                searchable_text(message),
                encode(*message)?
            ],
        )?;
    }
    Ok(())
}

/// What search finds a message by: its text and its attachments' names.
fn searchable_text(message: &Message) -> String {
    let mut text = message.body.clone();
    for attachment in &message.attachments {
        text.push('\n');
        text.push_str(&attachment.name);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, sent_at: i64, status: MessageStatus) -> Message {
        Message {
            id: id.into(),
            conversation_id: "general".into(),
            author: "ana".into(),
            author_id: "ana".into(),
            body: id.into(),
            sent_at,
            status,
            ..Message::default()
        }
    }

    #[test]
    fn upsert_sorts_dedupes_and_reports() {
        let mut snap = ChatSnapshot::new();
        assert_eq!(
            snap.upsert_message(message("b", 2, MessageStatus::Sent)),
            Upsert::Inserted
        );
        assert_eq!(
            snap.upsert_message(message("a", 1, MessageStatus::Sending)),
            Upsert::Inserted
        );
        assert_eq!(
            snap.upsert_message(message("b", 2, MessageStatus::Sent)),
            Upsert::Unchanged
        );
        assert_eq!(
            snap.upsert_message(message("a", 3, MessageStatus::Sent)),
            Upsert::StatusChanged
        );
        let ids: Vec<_> = snap
            .messages("general")
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(ids, ["b", "a"]);
    }

    #[test]
    fn upsert_caps_each_conversation() {
        let mut snap = ChatSnapshot::new();
        for i in 0..(MAX_MESSAGES_PER_CONVERSATION as i64 + 5) {
            snap.upsert_message(message(&format!("m{i}"), i, MessageStatus::Sent));
        }
        let list = snap.messages("general");
        assert_eq!(list.len(), MAX_MESSAGES_PER_CONVERSATION);
        assert_eq!(list[0].id, "m5");
        // Re-delivering an evicted message neither re-inserts nor announces it.
        assert_eq!(
            snap.upsert_message(message("m0", 0, MessageStatus::Sent)),
            Upsert::Unchanged
        );
        assert_eq!(snap.messages("general")[0].id, "m5");
        // Unsent messages are never evicted.
        snap.upsert_message(message("draft", -1, MessageStatus::Failed));
        let list = snap.messages("general");
        assert_eq!(list.len(), MAX_MESSAGES_PER_CONVERSATION);
        assert_eq!((list[0].id.as_str(), list[1].id.as_str()), ("draft", "m6"));
    }

    #[test]
    fn cursor_only_moves_forward() {
        let mut snap = ChatSnapshot::new();
        snap.advance_cursor("general", 10);
        snap.advance_cursor("general", 5);
        assert_eq!(snap.last_synced("general"), Some(10));
    }

    #[test]
    fn round_trips_through_the_database_and_keeps_the_outbox() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChatStore::in_directory(dir.path());
        assert_eq!(store.load().unwrap(), ChatSnapshot::new());

        let mut snap = ChatSnapshot::new();
        snap.conversations.push(Conversation {
            id: "general".into(),
            title: "General".into(),
            last_message_at: Some(2),
            unread_count: 1,
            muted: true,
            ..Conversation::default()
        });
        snap.upsert_message(message("a", 1, MessageStatus::Sent));
        snap.upsert_message(message("b", 2, MessageStatus::Sending));
        snap.advance_cursor("general", 1);
        snap.drafts
            .insert("general".into(), "half a thought".into());
        store.save(&snap).unwrap();

        // A second store on the same directory: what a relaunch sees.
        let loaded = ChatStore::in_directory(dir.path()).load().unwrap();
        assert_eq!(loaded, snap);
        assert_eq!(loaded.message("b").unwrap().status, MessageStatus::Sending);
        assert_eq!(loaded.outbox().len(), 1);

        // Only changes are written: edit one, remove one.
        snap.upsert_message(Message {
            body: "a, edited".into(),
            edited_at: Some(3),
            ..message("a", 1, MessageStatus::Sent)
        });
        snap.remove_message("b");
        snap.drafts.clear();
        store.save(&snap).unwrap();
        let loaded = ChatStore::in_directory(dir.path()).load().unwrap();
        assert_eq!(loaded, snap);
    }

    #[test]
    fn search_matches_words_as_prefixes_and_skips_deleted_messages() {
        let store = ChatStore::in_memory();
        let mut snap = ChatSnapshot::new();
        let say = |id: &str, room: &str, body: &str, at: i64| Message {
            conversation_id: room.into(),
            body: body.into(),
            ..message(id, at, MessageStatus::Sent)
        };
        snap.upsert_message(say("1", "general", "Weekend at the lake?", 1));
        snap.upsert_message(say("2", "random", "The LAKE was cold", 2));
        snap.upsert_message(say("3", "general", "Café on Friday", 3));
        snap.upsert_message(Message {
            deleted: true,
            ..say("4", "general", "", 4)
        });
        store.save(&snap).unwrap();

        let ids = |found: Vec<Message>| found.into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(ids(store.search("lak", None).unwrap()), ["2", "1"]);
        assert_eq!(ids(store.search("lake", Some("general")).unwrap()), ["1"]);
        assert_eq!(ids(store.search("weekend lake", None).unwrap()), ["1"]);
        assert_eq!(
            ids(store.search("cafe", None).unwrap()),
            ["3"],
            "accents ignored"
        );
        assert!(store.search("  \"*(", None).unwrap().is_empty());

        // Edits are re-indexed.
        snap.upsert_message(say("3", "general", "Tea on Friday", 3));
        store.save(&snap).unwrap();
        assert!(store.search("cafe", None).unwrap().is_empty());
        assert_eq!(ids(store.search("tea", None).unwrap()), ["3"]);
    }

    #[test]
    fn migrate_accepts_v1_and_refuses_newer_or_garbage() {
        let v1 = serde_json::json!({
            "schema_version": 1, "conversations": [], "messages": {}, "last_synced": {}
        });
        assert_eq!(migrate(v1).unwrap(), ChatSnapshot::new());
        assert_eq!(
            migrate(serde_json::json!({"schema_version": 9})).unwrap_err(),
            StoreError::UnsupportedVersion {
                found: 9,
                supported: STORE_VERSION
            }
        );
        assert!(matches!(
            migrate(serde_json::json!({"conversations": []})),
            Err(StoreError::Corrupt { .. })
        ));
        assert!(matches!(
            migrate(serde_json::json!({"schema_version": 0})),
            Err(StoreError::Corrupt { .. })
        ));
    }

    #[test]
    fn an_unreadable_json_store_is_reported_and_left_in_place() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(STORE_FILENAME), b"{not json").unwrap();
        assert!(matches!(
            ChatStore::in_directory(dir.path()).load(),
            Err(StoreError::Corrupt { .. })
        ));
        assert!(dir.path().join(STORE_FILENAME).exists());
    }
}
