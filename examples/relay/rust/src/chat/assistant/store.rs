//! `assistant.db`: the assistant's conversations and messages, on this
//! iPhone only. Separate from the chat accounts, so it survives signing out
//! and works without one.

use std::path::Path;

use forge_storage::rusqlite::{self, params, OptionalExtension, Row};
use forge_storage::{Database, Opened, StorageError};

use super::{
    AssistantError, AssistantMessage, AssistantMessageState, AssistantRole, AssistantStats,
};

const MIGRATIONS: &[&str] = &["CREATE TABLE conversations (
         id TEXT PRIMARY KEY,
         character_id TEXT NOT NULL,
         created_at INTEGER NOT NULL,
         updated_at INTEGER NOT NULL
     );
     CREATE TABLE messages (
         seq INTEGER PRIMARY KEY AUTOINCREMENT,
         id TEXT NOT NULL UNIQUE,
         conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
         role TEXT NOT NULL,
         text TEXT NOT NULL,
         state TEXT NOT NULL,
         error TEXT,
         created_at INTEGER NOT NULL,
         engine_id TEXT,
         first_text_ms INTEGER,
         duration_ms INTEGER,
         pieces INTEGER
     );
     CREATE INDEX messages_by_conversation ON messages(conversation_id, seq);"];

pub(crate) struct Store {
    db: Database,
}

/// A conversation row, before the character and last message are added.
pub(crate) struct ConversationRow {
    pub id: String,
    pub character_id: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, AssistantError> {
        let (db, opened) = Database::open(path, MIGRATIONS)?;
        if let Opened::Recovered { reason, .. } = opened {
            forge_runtime::logging::warn!(target: "assistant", "assistant.db was damaged and was replaced: {reason}");
        }
        let store = Self { db };
        // A generation cannot outlive the process: answers still marked as
        // streaming were cut off by a crash or a kill.
        let interrupted = store.db.connection().execute(
            "UPDATE messages SET state = ?1 WHERE state = ?2",
            params![
                state_name(AssistantMessageState::Interrupted),
                state_name(AssistantMessageState::Streaming)
            ],
        )?;
        if interrupted > 0 {
            forge_runtime::logging::info!(target: "assistant", "{interrupted} answer(s) were interrupted by the last exit");
        }
        Ok(store)
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self {
            db: Database::in_memory(MIGRATIONS).expect("in-memory database"),
        }
    }

    pub fn insert_conversation(&self, row: &ConversationRow) -> Result<(), AssistantError> {
        self.db.connection().execute(
            "INSERT INTO conversations (id, character_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![row.id, row.character_id, row.created_at, row.updated_at],
        )?;
        Ok(())
    }

    /// Newest activity first.
    pub fn conversations(&self) -> Result<Vec<ConversationRow>, AssistantError> {
        let mut statement = self.db.connection().prepare(
            "SELECT id, character_id, created_at, updated_at FROM conversations
             ORDER BY updated_at DESC, created_at DESC",
        )?;
        let rows = statement.query_map([], conversation_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn conversation(&self, id: &str) -> Result<Option<ConversationRow>, AssistantError> {
        Ok(self
            .db
            .connection()
            .query_row(
                "SELECT id, character_id, created_at, updated_at FROM conversations WHERE id = ?1",
                [id],
                conversation_row,
            )
            .optional()?)
    }

    pub fn delete_conversation(&self, id: &str) -> Result<bool, AssistantError> {
        let connection = self.db.connection();
        connection.execute("DELETE FROM messages WHERE conversation_id = ?1", [id])?;
        Ok(connection.execute("DELETE FROM conversations WHERE id = ?1", [id])? > 0)
    }

    /// Adds a message and moves its conversation to the top of the list.
    pub fn insert_message(&self, message: &AssistantMessage) -> Result<(), AssistantError> {
        let stats = message.stats.as_ref();
        let connection = self.db.connection();
        connection.execute(
            "INSERT INTO messages (id, conversation_id, role, text, state, error, created_at,
                                   engine_id, first_text_ms, duration_ms, pieces)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                message.id,
                message.conversation_id,
                role_name(message.role),
                message.text,
                state_name(message.state),
                message.error,
                message.created_at,
                stats.map(|s| s.engine_id.clone()),
                stats.and_then(|s| s.first_text_ms.map(|v| v as i64)),
                stats.map(|s| s.duration_ms as i64),
                stats.map(|s| s.pieces as i64),
            ],
        )?;
        connection.execute(
            "UPDATE conversations SET updated_at = ?2 WHERE id = ?1",
            params![message.conversation_id, message.created_at],
        )?;
        Ok(())
    }

    /// Saves the end of an answer: its text, state, error and timings.
    pub fn finish_message(&self, message: &AssistantMessage) -> Result<(), AssistantError> {
        let stats = message.stats.as_ref();
        self.db.connection().execute(
            "UPDATE messages SET text = ?2, state = ?3, error = ?4, engine_id = ?5,
                    first_text_ms = ?6, duration_ms = ?7, pieces = ?8
             WHERE id = ?1",
            params![
                message.id,
                message.text,
                state_name(message.state),
                message.error,
                stats.map(|s| s.engine_id.clone()),
                stats.and_then(|s| s.first_text_ms.map(|v| v as i64)),
                stats.map(|s| s.duration_ms as i64),
                stats.map(|s| s.pieces as i64),
            ],
        )?;
        Ok(())
    }

    /// A conversation's messages, oldest first.
    pub fn messages(&self, conversation_id: &str) -> Result<Vec<AssistantMessage>, AssistantError> {
        let mut statement = self.db.connection().prepare(
            "SELECT id, conversation_id, role, text, state, error, created_at,
                    engine_id, first_text_ms, duration_ms, pieces
             FROM messages WHERE conversation_id = ?1 ORDER BY seq",
        )?;
        let rows = statement.query_map([conversation_id], message_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn last_message(
        &self,
        conversation_id: &str,
    ) -> Result<Option<AssistantMessage>, AssistantError> {
        Ok(self
            .db
            .connection()
            .query_row(
                "SELECT id, conversation_id, role, text, state, error, created_at,
                        engine_id, first_text_ms, duration_ms, pieces
                 FROM messages WHERE conversation_id = ?1 ORDER BY seq DESC LIMIT 1",
                [conversation_id],
                message_row,
            )
            .optional()?)
    }
}

fn conversation_row(row: &Row<'_>) -> rusqlite::Result<ConversationRow> {
    Ok(ConversationRow {
        id: row.get(0)?,
        character_id: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

fn message_row(row: &Row<'_>) -> rusqlite::Result<AssistantMessage> {
    let engine_id: Option<String> = row.get(7)?;
    let stats = engine_id.map(|engine_id| -> rusqlite::Result<AssistantStats> {
        Ok(AssistantStats {
            engine_id,
            first_text_ms: row.get::<_, Option<i64>>(8)?.map(|v| v as u64),
            duration_ms: row.get::<_, Option<i64>>(9)?.unwrap_or(0) as u64,
            pieces: row.get::<_, Option<i64>>(10)?.unwrap_or(0) as u32,
        })
    });
    Ok(AssistantMessage {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        role: parse_role(&row.get::<_, String>(2)?),
        text: row.get(3)?,
        state: parse_state(&row.get::<_, String>(4)?),
        error: row.get(5)?,
        created_at: row.get(6)?,
        stats: stats.transpose()?,
    })
}

fn role_name(role: AssistantRole) -> &'static str {
    match role {
        AssistantRole::User => "user",
        AssistantRole::Assistant => "assistant",
    }
}

fn parse_role(name: &str) -> AssistantRole {
    match name {
        "user" => AssistantRole::User,
        _ => AssistantRole::Assistant,
    }
}

fn state_name(state: AssistantMessageState) -> &'static str {
    match state {
        AssistantMessageState::Done => "done",
        AssistantMessageState::Streaming => "streaming",
        AssistantMessageState::Stopped => "stopped",
        AssistantMessageState::Interrupted => "interrupted",
        AssistantMessageState::Refused => "refused",
        AssistantMessageState::Failed => "failed",
    }
}

fn parse_state(name: &str) -> AssistantMessageState {
    match name {
        "done" => AssistantMessageState::Done,
        "streaming" => AssistantMessageState::Streaming,
        "stopped" => AssistantMessageState::Stopped,
        "refused" => AssistantMessageState::Refused,
        "failed" => AssistantMessageState::Failed,
        _ => AssistantMessageState::Interrupted,
    }
}

impl From<StorageError> for AssistantError {
    fn from(err: StorageError) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

impl From<rusqlite::Error> for AssistantError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}
