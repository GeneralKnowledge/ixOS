//! What crosses the FFI boundary: records, events and errors, and their
//! conversions from chat-core.

/// The signed-in account, without its token (the token never leaves Rust
/// and the Keychain).
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChatAccount {
    pub user_id: String,
    pub display_name: String,
    /// The chat server this account lives on.
    pub server_url: String,
}

/// One conversation as the list shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChatConversation {
    pub id: String,
    pub title: String,
    /// Newest message the server knows of, Unix ms. `None` for an empty room.
    pub last_message_at: Option<i64>,
    /// Messages from others that arrived while the room was not open.
    pub unread_count: u32,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatMessageStatus {
    /// Written on this device; the server has not confirmed it yet.
    Sending,
    /// The server has it.
    Sent,
    /// The last attempt failed; `retry_message` tries again.
    Failed,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    /// Chosen by the sending device; stable across retries.
    pub id: String,
    pub conversation_id: String,
    /// The author's display name.
    pub author: String,
    /// The author's account id; empty for messages from clients without an
    /// account.
    pub author_id: String,
    /// Whether the signed-in account wrote it. Decided in Rust (by account
    /// id), so the host never compares names.
    pub is_mine: bool,
    pub body: String,
    /// Unix ms: device time while sending or failed, server time once sent.
    pub sent_at: i64,
    pub status: ChatMessageStatus,
}

/// Where the app is in its lifecycle, as the host reports it with
/// `ChatCore::set_phase`.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppPhase {
    Launching,
    Active,
    Inactive,
    Background,
    Terminated,
}

impl From<AppPhase> for forge_runtime::Phase {
    fn from(phase: AppPhase) -> Self {
        match phase {
            AppPhase::Launching => Self::Launching,
            AppPhase::Active => Self::Active,
            AppPhase::Inactive => Self::Inactive,
            AppPhase::Background => Self::Background,
            AppPhase::Terminated => Self::Terminated,
        }
    }
}

impl From<forge_runtime::Phase> for AppPhase {
    fn from(phase: forge_runtime::Phase) -> Self {
        match phase {
            forge_runtime::Phase::Launching => Self::Launching,
            forge_runtime::Phase::Active => Self::Active,
            forge_runtime::Phase::Inactive => Self::Inactive,
            forge_runtime::Phase::Background => Self::Background,
            forge_runtime::Phase::Terminated => Self::Terminated,
        }
    }
}

/// Everything the host renders from. All events of a `ChatCore` go to every
/// `ChatEvents` stream; the host filters by `conversation_id`.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum ChatEvent {
    /// Signed in, signed out, or a different account.
    AccountChanged {
        account: Option<ChatAccount>,
    },
    /// The server no longer accepts this device's account (its data was
    /// reset, or the URL points at another server). The account stays
    /// until the user signs up again or signs out.
    AccountRejected {
        reason: String,
    },
    ConversationUpdated {
        conversation: ChatConversation,
    },
    /// A message the host has not seen yet, or a new version of one (hosts
    /// upsert by id).
    MessageReceived {
        message: ChatMessage,
    },
    /// Someone else's new message in a room that is not open: show a local
    /// notification.
    Notify {
        message: ChatMessage,
    },
    MessageStatusChanged {
        conversation_id: String,
        id: String,
        status: ChatMessageStatus,
    },
    StreamConnected {
        conversation_id: String,
    },
    /// `will_retry`: Rust is reconnecting (or the app is in the background
    /// and reconnects on resume). Without it the room's stream is over.
    StreamDisconnected {
        conversation_id: String,
        reason: String,
        will_retry: bool,
    },
    /// The host reported a new phase. `resumed` is true on the way back from
    /// the background: refresh the conversation list then.
    PhaseChanged {
        phase: AppPhase,
        resumed: bool,
    },
    /// This consumer fell behind and missed `count` events; re-read the
    /// cached conversations and messages.
    EventsDropped {
        count: u64,
    },
}

/// Every error a command returns. `reason`/`detail` are diagnostics, not UI
/// copy. (No field is called `message`: it clashes with Kotlin's
/// `Throwable.message`.)
#[derive(uniffi::Error, Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ChatError {
    #[error("network error: {reason}")]
    Network { reason: String },
    #[error("certificate rejected: {reason}")]
    Certificate { reason: String },
    #[error("server returned HTTP {status}: {reason}")]
    Http { status: u16, reason: String },
    #[error("unexpected response: {reason}")]
    BadResponse { reason: String },
    #[error("operation was cancelled")]
    Cancelled,
    #[error("conversation not found: {conversation_id}")]
    NotFound { conversation_id: String },
    #[error("not supported on this host: {detail}")]
    UnsupportedOnHost { detail: String },
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    /// No account yet: call `sign_up` first.
    #[error("not signed in")]
    NotSignedIn,
    /// The server rejected the account token (HTTP 401).
    #[error("account rejected: {reason}")]
    Unauthorized { reason: String },
    /// The Keychain or the chat store could not be used.
    #[error("storage error: {reason}")]
    Storage { reason: String },
    #[error("the chat core has been shut down")]
    ShutDown,
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

/// Converts with the signed-in account's id, which decides `is_mine`.
pub(crate) fn message(m: chat_core::Message, user_id: &str) -> ChatMessage {
    ChatMessage {
        is_mine: !user_id.is_empty() && m.author_id == user_id,
        id: m.id,
        conversation_id: m.conversation_id,
        author: m.author,
        author_id: m.author_id,
        body: m.body,
        sent_at: m.sent_at,
        status: m.status.into(),
    }
}

pub(crate) fn event(event: chat_core::ChatEvent, user_id: &str) -> ChatEvent {
    use chat_core::ChatEvent as Core;
    match event {
        Core::ConversationUpdated(c) => ChatEvent::ConversationUpdated {
            conversation: c.into(),
        },
        Core::MessageReceived(m) => ChatEvent::MessageReceived {
            message: message(m, user_id),
        },
        Core::Notify(m) => ChatEvent::Notify {
            message: message(m, user_id),
        },
        Core::MessageStatusChanged {
            conversation_id,
            id,
            status,
        } => ChatEvent::MessageStatusChanged {
            conversation_id,
            id,
            status: status.into(),
        },
        Core::StreamConnected { conversation_id } => ChatEvent::StreamConnected { conversation_id },
        Core::StreamDisconnected {
            conversation_id,
            reason,
            will_retry,
        } => ChatEvent::StreamDisconnected {
            conversation_id,
            reason,
            will_retry,
        },
    }
}

impl From<chat_core::ChatError> for ChatError {
    fn from(error: chat_core::ChatError) -> Self {
        use chat_core::ChatError as Core;
        match error {
            Core::Network { reason } => Self::Network { reason },
            Core::Certificate { reason } => Self::Certificate { reason },
            Core::Http { status, reason } => Self::Http { status, reason },
            Core::Protocol { reason } => Self::BadResponse { reason },
            Core::Cancelled => Self::Cancelled,
            Core::NotFound { conversation_id } => Self::NotFound { conversation_id },
            Core::UnsupportedOnHost { detail } => Self::UnsupportedOnHost { detail },
            Core::InvalidInput { reason } => Self::InvalidInput { reason },
            Core::ShutDown => Self::ShutDown,
            Core::Unauthorized { reason } => Self::Unauthorized { reason },
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

impl From<crate::native::NativeServiceError> for ChatError {
    fn from(error: crate::native::NativeServiceError) -> Self {
        Self::Storage {
            reason: error.to_string(),
        }
    }
}
