//! The chat store: one JSON snapshot holding every conversation, the
//! messages of each, and the per-conversation `since` cursor.
//!
//! Deliberately dumb: the whole snapshot is rewritten (atomically, with
//! `forge_storage::write_atomic`) after every change, and each conversation
//! keeps at most [`MAX_MESSAGES_PER_CONVERSATION`] messages. The point is to
//! exercise persistence and the `since` cursor, not to build a database.
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "conversations": [ { "id": "general", "title": "General", "last_message_at": 1730000000000, "unread_count": 0 } ],
//!   "messages": { "general": [ { "id": "...", "conversation_id": "general", "author": "me", "body": "hi", "sent_at": 1730000000000, "status": "sent" } ] },
//!   "last_synced": { "general": 1730000000000 }
//! }
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

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

    /// A message left `Sending` by a previous run never got its answer.
    /// Called on load: those become `Failed` so the user can retry them.
    fn fail_interrupted_sends(&mut self) {
        for message in self.messages.values_mut().flatten() {
            if message.status == MessageStatus::Sending {
                message.status = MessageStatus::Failed;
            }
        }
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

/// Where the snapshot lives. `None` keeps it in memory only (tests, or a host
/// that passes no directory).
#[derive(Debug, Clone)]
pub struct ChatStore {
    path: Option<PathBuf>,
}

impl ChatStore {
    pub fn in_memory() -> Self {
        Self { path: None }
    }

    pub fn in_directory(directory: impl AsRef<Path>) -> Self {
        Self {
            path: Some(directory.as_ref().join(STORE_FILENAME)),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The stored snapshot, or an empty one when there is none yet.
    pub fn load(&self) -> Result<ChatSnapshot, StoreError> {
        let Some(path) = &self.path else {
            return Ok(ChatSnapshot::new());
        };
        let raw = match fs::read(path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(ChatSnapshot::new()),
            Err(err) => {
                return Err(StoreError::Io {
                    reason: format!("read {}: {err}", path.display()),
                })
            }
        };
        let value: Value = serde_json::from_slice(&raw).map_err(|e| StoreError::Corrupt {
            reason: e.to_string(),
        })?;
        let mut snapshot = migrate(value)?;
        snapshot.fail_interrupted_sends();
        Ok(snapshot)
    }

    pub fn save(&self, snapshot: &ChatSnapshot) -> Result<(), StoreError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let json = serde_json::to_vec_pretty(snapshot).map_err(|e| StoreError::Io {
            reason: e.to_string(),
        })?;
        forge_storage::write_atomic(path, &json).map_err(|e| StoreError::Io {
            reason: e.to_string(),
        })
    }
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
    fn round_trips_through_disk_and_fails_interrupted_sends() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChatStore::in_directory(dir.path());
        assert_eq!(store.load().unwrap(), ChatSnapshot::new());

        let mut snap = ChatSnapshot::new();
        snap.conversations.push(Conversation {
            id: "general".into(),
            title: "General".into(),
            last_message_at: Some(2),
            unread_count: 1,
        });
        snap.upsert_message(message("a", 1, MessageStatus::Sent));
        snap.upsert_message(message("b", 2, MessageStatus::Sending));
        snap.advance_cursor("general", 1);
        store.save(&snap).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.conversations, snap.conversations);
        assert_eq!(loaded.last_synced("general"), Some(1));
        assert_eq!(loaded.message("b").unwrap().status, MessageStatus::Failed);
        assert_eq!(loaded.message("a").unwrap().status, MessageStatus::Sent);
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
    fn corrupt_file_is_an_error_not_silently_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(STORE_FILENAME), b"{not json").unwrap();
        assert!(matches!(
            ChatStore::in_directory(dir.path()).load(),
            Err(StoreError::Corrupt { .. })
        ));
    }
}
