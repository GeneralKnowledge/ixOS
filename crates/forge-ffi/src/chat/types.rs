//! Records, enums and the error the Chat example sends across the FFI
//! boundary. They mirror `chat_core`'s types one to one (see
//! `docs/chat-demo.md`), with two FFI-only additions: `ChatEvent::EventsDropped`
//! and `ChatError::Storage` / `ChatError::Internal`.
//!
//! Names carry a `Chat` prefix because every example's API shares one
//! generated module (`ForgeFFI` in Swift, `uniffi.forge_ffi` in Kotlin).
//! Error fields avoid `message`, which clashes with `Throwable.message` in
//! Kotlin.

/// A chat room as the conversation list shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChatConversation {
    pub id: String,
    pub title: String,
    /// Newest message time, Unix milliseconds. `None` for an empty room.
    pub last_message_at: Option<i64>,
    /// Messages from others that arrived while the room was not open.
    pub unread_count: u32,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatMessageStatus {
    /// Written locally; the server has not confirmed it yet.
    Sending,
    Sent,
    /// The last attempt failed; `retry_message` tries again.
    Failed,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    /// Chosen by the sending device; stable across retries.
    pub id: String,
    pub conversation_id: String,
    /// Display name. The local user's messages carry `ChatAppConfig.user`.
    pub author: String,
    pub body: String,
    /// Unix milliseconds: device time until `Sent`, then server time.
    pub sent_at: i64,
    pub status: ChatMessageStatus,
}

/// What the host should render. One stream per app instance, for every
/// room; filter by `conversation_id` (or, for status changes, message id).
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum ChatEvent {
    ConversationUpdated {
        conversation: ChatConversation,
    },
    /// A message the host has not seen: someone else's, or our own at the
    /// moment it is written (status `Sending`). Upsert by `id`.
    MessageReceived {
        message: ChatMessage,
    },
    MessageStatusChanged {
        id: String,
        status: ChatMessageStatus,
    },
    StreamConnected {
        conversation_id: String,
    },
    /// With `will_retry` the client is reconnecting on its own; without it
    /// the subscription is over (unsubscribed, shut down, or no such room).
    StreamDisconnected {
        conversation_id: String,
        reason: String,
        will_retry: bool,
    },
    /// This subscriber fell behind and `count` older events were discarded.
    /// Re-read `cached_conversations()` and, for the open room,
    /// `cached_messages(id)` instead of relying on the missed events.
    EventsDropped {
        count: u64,
    },
}

/// Settings for [`crate::ForgeChatApp::new`].
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChatAppConfig {
    /// The chat server, e.g. `http://127.0.0.1:8787` (or
    /// `http://10.0.2.2:8787` from the Android emulator).
    pub base_url: String,
    /// The local user's display name. Defaults to `"me"`.
    pub user: Option<String>,
    /// Timeout for list/load/send and for opening a stream. Default 10 s.
    pub request_timeout_ms: Option<u32>,
}

/// Every error the Chat API returns. `reason`/`detail` are diagnostics, not
/// UI copy.
#[derive(uniffi::Error, Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ChatError {
    /// The server could not be reached, the connection broke, or it timed out.
    #[error("network error: {reason}")]
    Network { reason: String },
    #[error("server returned HTTP {status}: {reason}")]
    Http { status: u16, reason: String },
    /// The server's answer was not what this build expects.
    #[error("protocol error: {reason}")]
    Protocol { reason: String },
    /// Abandoned before it took effect (the app shut down mid-send).
    #[error("operation was cancelled")]
    Cancelled,
    #[error("conversation not found: {conversation_id}")]
    NotFound { conversation_id: String },
    /// This host cannot do what was asked.
    #[error("not supported on this host: {detail}")]
    UnsupportedOnHost { detail: String },
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    /// The stored chat data cannot be used (written by a newer build).
    #[error("storage error: {reason}")]
    Storage { reason: String },
    #[error("the chat app instance has been shut down")]
    ShutDown,
    /// A bug, not an expected condition.
    #[error("internal failure: {reason}")]
    Internal { reason: String },
}

impl From<chat_core::Conversation> for ChatConversation {
    fn from(c: chat_core::Conversation) -> Self {
        Self {
            id: c.id,
            title: c.title,
            last_message_at: c.last_message_at,
            unread_count: c.unread_count,
        }
    }
}

impl From<chat_core::MessageStatus> for ChatMessageStatus {
    fn from(status: chat_core::MessageStatus) -> Self {
        match status {
            chat_core::MessageStatus::Sending => Self::Sending,
            chat_core::MessageStatus::Sent => Self::Sent,
            chat_core::MessageStatus::Failed => Self::Failed,
        }
    }
}

impl From<chat_core::Message> for ChatMessage {
    fn from(m: chat_core::Message) -> Self {
        Self {
            id: m.id,
            conversation_id: m.conversation_id,
            author: m.author,
            body: m.body,
            sent_at: m.sent_at,
            status: m.status.into(),
        }
    }
}

impl From<chat_core::ChatEvent> for ChatEvent {
    fn from(event: chat_core::ChatEvent) -> Self {
        use chat_core::ChatEvent as Core;
        match event {
            Core::ConversationUpdated(c) => Self::ConversationUpdated {
                conversation: c.into(),
            },
            Core::MessageReceived(m) => Self::MessageReceived { message: m.into() },
            Core::MessageStatusChanged { id, status } => Self::MessageStatusChanged {
                id,
                status: status.into(),
            },
            Core::StreamConnected { conversation_id } => Self::StreamConnected { conversation_id },
            Core::StreamDisconnected {
                conversation_id,
                reason,
                will_retry,
            } => Self::StreamDisconnected {
                conversation_id,
                reason,
                will_retry,
            },
        }
    }
}

impl From<chat_core::ChatError> for ChatError {
    fn from(error: chat_core::ChatError) -> Self {
        use chat_core::ChatError as Core;
        match error {
            Core::Network { reason } => Self::Network { reason },
            Core::Http { status, reason } => Self::Http { status, reason },
            Core::Protocol { reason } => Self::Protocol { reason },
            Core::Cancelled => Self::Cancelled,
            Core::NotFound { conversation_id } => Self::NotFound { conversation_id },
            Core::UnsupportedOnHost { detail } => Self::UnsupportedOnHost { detail },
            Core::InvalidInput { reason } => Self::InvalidInput { reason },
            Core::ShutDown => Self::ShutDown,
        }
    }
}

impl From<chat_core::StoreError> for ChatError {
    fn from(error: chat_core::StoreError) -> Self {
        Self::Storage {
            reason: error.to_string(),
        }
    }
}
