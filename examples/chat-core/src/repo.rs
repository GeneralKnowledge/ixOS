//! The server side of chat, as the client sees it.

use forge_net::CancelHandle;

use crate::types::{ChatError, Message};

/// A room as the server describes it. The client adds `unread_count`,
/// `muted` and `marked_unread` (see [`crate::Conversation`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteConversation {
    pub id: String,
    pub title: String,
    pub last_message_at: Option<i64>,
    /// The newest message, when the server sends it.
    pub last_message: Option<Message>,
    /// The caller's read cursor; `None` from servers without cursors and in
    /// stream events (which go to everyone).
    pub read_up_to: Option<i64>,
    /// How far any other account has read; `None` as for `read_up_to`.
    pub others_read_up_to: Option<i64>,
}

/// A message the client wants the server to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMessage {
    /// Client-chosen id. Posting the same id twice must not create a second
    /// message: the server answers with the one it already has.
    pub id: String,
    pub conversation_id: String,
    pub author: String,
    /// The sender's account id; empty without an account. Servers with
    /// accounts take both from the token instead.
    pub author_id: String,
    pub body: String,
}

/// What a live stream delivers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteEvent {
    /// A message was stored in the room, including our own (the echo).
    Message(Message),
    /// The room's metadata changed.
    Conversation(RemoteConversation),
    /// An account's read cursor in this room moved to `up_to`.
    Read { user_id: String, up_to: i64 },
    /// Nothing happened. Servers send these periodically; the client uses
    /// them to notice cancellation while the stream is otherwise quiet.
    Heartbeat,
}

/// A live, per-room stream of [`RemoteEvent`]s.
pub trait RemoteStream: Send {
    /// Blocks until the next event. `None` when the server closed the
    /// stream; `Some(Err(_))` when it broke (the stream is then finished).
    fn next_event(&mut self) -> Option<Result<RemoteEvent, ChatError>>;
}

/// The chat backend. Blocking, like `forge_net::HttpClient`: callers run it
/// on worker threads, never the UI thread.
///
/// Every returned [`Message`] has status `Sent` (the server has it).
pub trait ChatRepo: Send + Sync {
    fn list_conversations(&self) -> Result<Vec<RemoteConversation>, ChatError>;

    /// Messages in the room, oldest first. With `since`, only messages whose
    /// `sent_at` is strictly greater. An unknown room is
    /// [`ChatError::NotFound`].
    fn load_messages(
        &self,
        conversation_id: &str,
        since: Option<i64>,
    ) -> Result<Vec<Message>, ChatError>;

    /// Stores `message` (idempotent by id) and returns the server's copy.
    fn post_message(&self, message: &OutgoingMessage) -> Result<Message, ChatError>;

    /// Opens the room's live stream. Returns once the server accepted it, so
    /// anything stored after this returns is delivered on the stream.
    ///
    /// `cancel` ends the stream from another thread: opening fails with
    /// [`ChatError::Cancelled`], and a [`RemoteStream::next_event`] blocked
    /// on a quiet connection returns promptly (`Some(Err(Cancelled))` or
    /// `None`) without waiting for a heartbeat.
    fn open_stream(
        &self,
        conversation_id: &str,
        cancel: &CancelHandle,
    ) -> Result<Box<dyn RemoteStream>, ChatError>;

    /// Moves this account's read cursor in the room forward to `up_to`
    /// (servers never move it back). Servers without read cursors accept
    /// and ignore it.
    fn mark_read(&self, conversation_id: &str, up_to: i64) -> Result<(), ChatError> {
        let _ = (conversation_id, up_to);
        Ok(())
    }

    /// Creates an empty conversation titled `title`.
    fn create_conversation(&self, title: &str) -> Result<RemoteConversation, ChatError> {
        let _ = title;
        Err(ChatError::UnsupportedOnHost {
            detail: "this server cannot create conversations".into(),
        })
    }
}

impl<T: ChatRepo + ?Sized> ChatRepo for std::sync::Arc<T> {
    fn list_conversations(&self) -> Result<Vec<RemoteConversation>, ChatError> {
        (**self).list_conversations()
    }

    fn load_messages(
        &self,
        conversation_id: &str,
        since: Option<i64>,
    ) -> Result<Vec<Message>, ChatError> {
        (**self).load_messages(conversation_id, since)
    }

    fn post_message(&self, message: &OutgoingMessage) -> Result<Message, ChatError> {
        (**self).post_message(message)
    }

    fn open_stream(
        &self,
        conversation_id: &str,
        cancel: &CancelHandle,
    ) -> Result<Box<dyn RemoteStream>, ChatError> {
        (**self).open_stream(conversation_id, cancel)
    }

    fn mark_read(&self, conversation_id: &str, up_to: i64) -> Result<(), ChatError> {
        (**self).mark_read(conversation_id, up_to)
    }

    fn create_conversation(&self, title: &str) -> Result<RemoteConversation, ChatError> {
        (**self).create_conversation(title)
    }
}
