//! What crosses the FFI boundary: records, events and errors, and their
//! conversions from chat-core.

use crate::AppPhase;

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
    /// Messages from others this account has not read (by the server's read
    /// cursor). 0 while the room is open.
    pub unread_count: u32,
    /// Show the unread dot: unread messages, or the user marked it unread.
    pub shows_unread: bool,
    /// The newest message, for the preview line (ours too, while sending).
    pub last_message: Option<ChatMessage>,
    /// How far anyone else has read, Unix ms. One of our sent messages with
    /// `sent_at` up to it has been read; `ChatMessage::is_read` says so.
    pub others_read_up_to: Option<i64>,
    /// No notifications for this room (this device only).
    pub muted: bool,
    /// The user swiped it to unread (this device only).
    pub marked_unread: bool,
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
    /// The message this one answers (same conversation), by id. The host
    /// finds it among the room's messages to quote it.
    pub reply_to: Option<String>,
    /// Its author changed the text after sending.
    pub edited: bool,
    /// Deleted for everyone: show a placeholder; `body` is empty.
    pub deleted: bool,
    /// Reactions, most popular first (then by emoji).
    pub reactions: Vec<ChatReaction>,
    /// Photos and documents it carries, in order (M3). Fetch the bytes with
    /// `ChatCore::fetch_attachment`.
    pub attachments: Vec<ChatAttachment>,
}

/// A file a message carries.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChatAttachment {
    /// The SHA-256 of its bytes (lowercase hex): how to fetch it.
    pub id: String,
    /// The file name as the sender had it, e.g. `IMG_0042.jpg`.
    pub name: String,
    /// Its media type, e.g. `image/jpeg` or `application/pdf`.
    pub mime: String,
    pub size: u64,
    /// A picture the app can show inline.
    pub is_image: bool,
}

/// A file the user picked to send.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChatOutgoingFile {
    /// Where it is now (Rust copies it at once; it may be deleted after
    /// `send_files` returns).
    pub path: String,
    /// The name to show; empty takes the file name.
    pub name: String,
    /// Its media type; empty means `application/octet-stream`.
    pub mime: String,
}

impl From<ChatOutgoingFile> for chat_core::OutgoingFile {
    fn from(file: ChatOutgoingFile) -> Self {
        Self {
            path: file.path.into(),
            name: file.name,
            mime: file.mime,
        }
    }
}

impl From<chat_core::Attachment> for ChatAttachment {
    fn from(a: chat_core::Attachment) -> Self {
        Self {
            is_image: a.is_image(),
            id: a.id,
            name: a.name,
            mime: a.mime,
            size: a.size,
        }
    }
}

/// One emoji on a message and who put it there.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChatReaction {
    pub emoji: String,
    pub count: u32,
    /// The signed-in account is one of them (tapping removes it).
    pub mine: bool,
}

/// Where one of our messages is, as its bubble shows it.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatDelivery {
    Sending,
    Sent,
    /// Someone else in the room has read it (their read cursor passed it).
    Read,
    Failed,
}

/// The storage engine compiled into the app, e.g. "SQLite 3.50.2"
/// (shown in Settings).
#[uniffi::export]
pub fn storage_engine() -> String {
    chat_core::storage_engine()
}

/// The delivery state of `message` in `conversation`: `Read` once another
/// account's read cursor has passed one of our sent messages. Only
/// meaningful for our own messages.
#[uniffi::export]
pub fn chat_delivery(message: ChatMessage, conversation: Option<ChatConversation>) -> ChatDelivery {
    match message.status {
        ChatMessageStatus::Sending => ChatDelivery::Sending,
        ChatMessageStatus::Failed => ChatDelivery::Failed,
        ChatMessageStatus::Sent => {
            let read = conversation
                .and_then(|c| c.others_read_up_to)
                .is_some_and(|up_to| message.sent_at <= up_to);
            if read {
                ChatDelivery::Read
            } else {
                ChatDelivery::Sent
            }
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
    /// An existing message was edited, deleted for everyone or reacted to
    /// (by anyone). Replace it by id; it is never unread news.
    MessageUpdated {
        message: ChatMessage,
    },
    /// A message that was never sent was deleted on this device: drop it.
    MessageRemoved {
        conversation_id: String,
        id: String,
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

/// Converts with the signed-in account's id, which decides the preview's
/// `is_mine`.
pub(crate) fn conversation(c: chat_core::Conversation, user_id: &str) -> ChatConversation {
    ChatConversation {
        shows_unread: c.shows_unread(),
        id: c.id,
        title: c.title,
        last_message_at: c.last_message_at,
        unread_count: c.unread_count,
        last_message: c.last_message.map(|m| message(m, user_id)),
        others_read_up_to: c.others_read_up_to,
        muted: c.muted,
        marked_unread: c.marked_unread,
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
    let mut reactions: Vec<ChatReaction> = m
        .reactions
        .iter()
        .filter(|(_, people)| !people.is_empty())
        .map(|(emoji, people)| ChatReaction {
            emoji: emoji.clone(),
            count: u32::try_from(people.len()).unwrap_or(u32::MAX),
            mine: !user_id.is_empty() && people.iter().any(|p| p == user_id),
        })
        .collect();
    // Stable: the BTreeMap order breaks ties.
    reactions.sort_by_key(|r| std::cmp::Reverse(r.count));
    ChatMessage {
        is_mine: !user_id.is_empty() && m.author_id == user_id,
        reply_to: m.reply_to,
        edited: m.edited_at.is_some(),
        deleted: m.deleted,
        reactions,
        attachments: m.attachments.into_iter().map(Into::into).collect(),
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
            conversation: conversation(c, user_id),
        },
        Core::MessageReceived(m) => ChatEvent::MessageReceived {
            message: message(m, user_id),
        },
        Core::Notify(m) => ChatEvent::Notify {
            message: message(m, user_id),
        },
        Core::MessageUpdated(m) => ChatEvent::MessageUpdated {
            message: message(m, user_id),
        },
        Core::MessageRemoved {
            conversation_id,
            id,
        } => ChatEvent::MessageRemoved {
            conversation_id,
            id,
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
