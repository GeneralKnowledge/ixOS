//! The chat domain: what crosses (or will cross) the FFI boundary.

use forge_net::NetError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One chat room as the conversation list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    /// `sent_at` of the newest message the server knows of, in Unix
    /// milliseconds. `None` for an empty room.
    pub last_message_at: Option<i64>,
    /// Messages from other people that arrived while this conversation was
    /// not open (not subscribed). Tracked on the device; reset by
    /// [`crate::ChatClient::subscribe`].
    pub unread_count: u32,
}

/// One chat message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Chosen by the sender's device and kept by the server, so a retry of
    /// the same message is idempotent and the stream echo of our own message
    /// can be matched to the local bubble.
    pub id: String,
    pub conversation_id: String,
    /// Display name of the author. The local user's own messages carry
    /// [`crate::ChatConfig::user`].
    pub author: String,
    pub body: String,
    /// Unix milliseconds. Device time while `Sending`/`Failed`, server time
    /// once `Sent`. The server keeps it strictly increasing within a room,
    /// which is what makes it usable as the `since` cursor.
    pub sent_at: i64,
    pub status: MessageStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    /// Written locally; the server has not confirmed it yet.
    Sending,
    /// The server has it.
    Sent,
    /// The last attempt failed. `ChatClient::retry_message` tries again.
    Failed,
}

/// Something the host should render. All events go through one sink per
/// [`crate::ChatClient`]; the host filters by `conversation_id`.
///
/// Every event is emitted after the change it describes is in the store,
/// so re-reading the store never contradicts an event already seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    /// A conversation's title, `last_message_at` or `unread_count` changed.
    ConversationUpdated(Conversation),
    /// A message the host has not been told about yet: someone else's
    /// message from the stream or a catch-up fetch, or the local user's own
    /// message at the moment it is written (status `Sending`).
    MessageReceived(Message),
    /// Someone else's message just counted towards `unread_count` (its room
    /// is not open: it arrived on a watched room's stream or in a refresh's
    /// prefetch), and it was sent after this client started.
    /// Follows its `MessageReceived`. Hosts show a local notification for
    /// it; the history a room catches up on when first watched, and what
    /// arrived while the app was not running, never notify.
    Notify(Message),
    /// A known message changed status (`Sending` → `Sent` / `Failed`, or
    /// `Failed` → `Sending` on retry).
    MessageStatusChanged { id: String, status: MessageStatus },
    /// The live stream for a conversation is open. Emitted again after every
    /// reconnect.
    StreamConnected { conversation_id: String },
    /// The live stream closed, or could not be opened. `reason` is a
    /// diagnostic, not UI copy. With `will_retry` the client keeps trying
    /// (with backoff) and a `StreamConnected` follows when it succeeds;
    /// without it the subscription is over (unsubscribed, shut down, or the
    /// room does not exist) and no more stream events arrive for it.
    StreamDisconnected {
        conversation_id: String,
        reason: String,
        will_retry: bool,
    },
}

/// Every error a chat command returns. `reason`/`detail` fields are
/// diagnostics, not UI copy. Field names avoid `message`, which clashes
/// with `Throwable.message` in the Kotlin bindings.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ChatError {
    /// The server could not be reached, the connection broke, or it timed out.
    #[error("network error: {reason}")]
    Network { reason: String },
    /// The server answered with a non-2xx status (other than 404 on a room).
    #[error("server returned HTTP {status}: {reason}")]
    Http { status: u16, reason: String },
    /// The server's answer was not what this build expects.
    #[error("protocol error: {reason}")]
    Protocol { reason: String },
    /// The command was abandoned before it took effect. Not a failure.
    #[error("operation was cancelled")]
    Cancelled,
    #[error("conversation not found: {conversation_id}")]
    NotFound { conversation_id: String },
    /// This host cannot do what was asked (no transport in this build, or a
    /// transport that is not set up on this platform).
    #[error("not supported on this host: {detail}")]
    UnsupportedOnHost { detail: String },
    /// The caller passed something unusable. Nothing was sent.
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    /// The client was shut down; it accepts no more commands.
    #[error("the chat client has been shut down")]
    ShutDown,
}

impl ChatError {
    pub(crate) fn protocol(reason: impl ToString) -> Self {
        Self::Protocol {
            reason: reason.to_string(),
        }
    }

    pub(crate) fn invalid(reason: impl Into<String>) -> Self {
        Self::InvalidInput {
            reason: reason.into(),
        }
    }

    /// Network-class errors are the ones worth retrying later.
    pub fn is_network(&self) -> bool {
        matches!(self, Self::Network { .. })
    }
}

impl From<NetError> for ChatError {
    fn from(error: NetError) -> Self {
        match error {
            NetError::Unavailable => ChatError::UnsupportedOnHost {
                detail: "no HTTP transport in this build".into(),
            },
            NetError::InvalidRequest { reason } => ChatError::InvalidInput { reason },
            NetError::Offline { reason } | NetError::Transport { reason } => {
                ChatError::Network { reason }
            }
            NetError::Timeout => ChatError::Network {
                reason: "request timed out".into(),
            },
            NetError::Cancelled => ChatError::Cancelled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_errors_map_onto_chat_errors() {
        assert!(matches!(
            ChatError::from(NetError::Unavailable),
            ChatError::UnsupportedOnHost { .. }
        ));
        assert!(ChatError::from(NetError::Timeout).is_network());
        assert!(ChatError::from(NetError::Offline { reason: "x".into() }).is_network());
        assert!(ChatError::from(NetError::Transport {
            reason: "tls".into()
        })
        .is_network());
        assert_eq!(ChatError::from(NetError::Cancelled), ChatError::Cancelled);
        assert!(matches!(
            ChatError::from(NetError::InvalidRequest { reason: "u".into() }),
            ChatError::InvalidInput { .. }
        ));
    }

    #[test]
    fn status_serialises_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&MessageStatus::Sending).unwrap(),
            "\"sending\""
        );
    }
}
