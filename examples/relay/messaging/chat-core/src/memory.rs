//! In-memory [`ChatRepo`]: the test double, and the reference for what the
//! mock server must do.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use forge_net::{AbortRegistration, CancelHandle};

use crate::now_ms;
use crate::repo::{ChatRepo, OutgoingMessage, RemoteConversation, RemoteEvent, RemoteStream};
use crate::types::{ChatError, Message, MessageStatus};

/// How often an idle in-memory stream yields [`RemoteEvent::Heartbeat`].
pub const MEMORY_HEARTBEAT: Duration = Duration::from_millis(20);

/// A thread-safe in-memory chat server.
///
/// * `sent_at` is server time and strictly increasing per room.
/// * Posting an id that exists returns the stored message unchanged.
/// * [`set_offline(true)`](Self::set_offline) makes every call fail with
///   [`ChatError::Network`] and closes every open stream with an error, like
///   killing the server process.
#[derive(Default)]
pub struct InMemoryRepo {
    state: Mutex<State>,
    open_streams: Arc<AtomicUsize>,
    /// Set by [`InMemoryRepo::set_heartbeats`]`(false)`.
    silent: AtomicBool,
}

#[derive(Default)]
struct State {
    offline: bool,
    /// Set by [`InMemoryRepo::set_rejecting`]: posts fail with HTTP 400.
    rejecting: bool,
    /// Set by [`InMemoryRepo::with_read_cursors`]: the account id this
    /// repo's client reads as. `None`: a server without read cursors.
    reader: Option<String>,
    next_room: usize,
    rooms: Vec<Room>,
    listeners: Vec<(String, Sender<Result<RemoteEvent, ChatError>>)>,
    /// Calls made, for assertions: `"list"`, `"load <id> <since>"`, ...
    calls: Vec<String>,
    /// Uploaded attachments by id (their SHA-256).
    blobs: HashMap<String, Vec<u8>>,
}

#[derive(Default)]
struct Room {
    id: String,
    title: String,
    messages: Vec<Message>,
    /// Read cursors by account id.
    reads: BTreeMap<String, i64>,
    /// The last time handed out for a post or a change.
    clock: i64,
}

impl Room {
    fn remote(&self, reader: Option<&str>) -> RemoteConversation {
        RemoteConversation {
            id: self.id.clone(),
            title: self.title.clone(),
            last_message_at: self.messages.last().map(|m| m.sent_at),
            last_message: self.messages.last().cloned(),
            read_up_to: reader.map(|me| self.reads.get(me).copied().unwrap_or(i64::MIN)),
            others_read_up_to: reader.and_then(|me| {
                self.reads
                    .iter()
                    .filter(|(who, _)| who.as_str() != me)
                    .map(|(_, up_to)| *up_to)
                    .max()
            }),
        }
    }

    /// Moves `user_id`'s cursor forward; the new cursor if it moved.
    fn mark_read(&mut self, user_id: &str, up_to: i64) -> Option<i64> {
        let up_to = up_to.min(self.messages.last()?.sent_at);
        let cursor = self.reads.entry(user_id.into()).or_insert(i64::MIN);
        (up_to > *cursor).then(|| {
            *cursor = up_to;
            up_to
        })
    }

    /// A time later than every earlier post or change in the room.
    fn tick(&mut self) -> i64 {
        self.clock = now_ms().max(self.clock.saturating_add(1));
        self.clock
    }

    /// Changes message `id` and stamps it, as the server does.
    fn change(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut Message, i64) -> Result<(), ChatError>,
    ) -> Result<Message, ChatError> {
        let now = self.tick();
        let message = self
            .messages
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or_else(|| ChatError::Http {
                status: 404,
                reason: format!("no message {id}"),
            })?;
        let mut changed = message.clone();
        change(&mut changed, now)?;
        changed.updated_at = Some(now);
        *message = changed.clone();
        Ok(changed)
    }
}

fn refused(status: u16, reason: &str) -> ChatError {
    ChatError::Http {
        status,
        reason: reason.into(),
    }
}

fn offline() -> ChatError {
    ChatError::Network {
        reason: "connection refused (in-memory server is offline)".into(),
    }
}

impl InMemoryRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// A repo with these `(id, title)` rooms, all empty.
    pub fn with_rooms(rooms: &[(&str, &str)]) -> Self {
        let repo = Self::new();
        {
            let mut state = repo.lock();
            for (id, title) in rooms {
                state.rooms.push(Room {
                    id: (*id).into(),
                    title: (*title).into(),
                    ..Room::default()
                });
            }
        }
        repo
    }

    /// Keeps read cursors like the chat server, with this repo's client
    /// reading as `reader_id` (its `ChatConfig::user_id`). Without it the
    /// repo is a server without cursors.
    pub fn with_read_cursors(self, reader_id: &str) -> Self {
        self.lock().reader = Some(reader_id.into());
        self
    }

    /// Someone else (`user_id`) read the room up to `up_to`: moves their
    /// cursor and broadcasts it.
    pub fn inject_read(&self, conversation_id: &str, user_id: &str, up_to: i64) {
        let mut state = self.lock();
        let room = state
            .rooms
            .iter_mut()
            .find(|r| r.id == conversation_id)
            .expect("a room that exists");
        if let Some(up_to) = room.mark_read(user_id, up_to) {
            broadcast(
                &mut state,
                conversation_id,
                RemoteEvent::Read {
                    user_id: user_id.into(),
                    up_to,
                },
            );
        }
    }

    /// An account's read cursor, as the server holds it.
    pub fn read_cursor(&self, conversation_id: &str, user_id: &str) -> Option<i64> {
        let state = self.lock();
        let room = state.rooms.iter().find(|r| r.id == conversation_id)?;
        room.reads.get(user_id).copied()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Takes the server down (`true`) or brings it back (`false`).
    pub fn set_offline(&self, offline_now: bool) {
        let mut state = self.lock();
        state.offline = offline_now;
        if offline_now {
            for (_, listener) in state.listeners.drain(..) {
                let _ = listener.send(Err(ChatError::Network {
                    reason: "connection reset (in-memory server went offline)".into(),
                }));
            }
        }
    }

    /// Makes the server refuse new posts (`true`, HTTP 400) or accept them
    /// again. Unlike being offline, a refusal is final: the client marks the
    /// message `Failed` instead of keeping it in the outbox.
    pub fn set_rejecting(&self, rejecting: bool) {
        self.lock().rejecting = rejecting;
    }

    /// Stores a message from someone else, as `curl -X POST` against the mock
    /// server would, and broadcasts it. Works while offline too (the message
    /// is then only seen on the next fetch), so tests can model "it arrived
    /// while we were disconnected".
    pub fn inject(&self, conversation_id: &str, author: &str, body: &str) -> Message {
        let mut state = self.lock();
        let room = state
            .rooms
            .iter_mut()
            .find(|r| r.id == conversation_id)
            .expect("inject into a room that exists");
        let message = Message {
            id: format!("srv-{}-{}", room.id, room.messages.len() + 1),
            conversation_id: conversation_id.into(),
            author: author.into(),
            // Injected authors stand for accounts named after them.
            author_id: author.into(),
            body: body.into(),
            sent_at: room.tick(),
            status: MessageStatus::Sent,
            ..Message::default()
        };
        room.messages.push(message.clone());
        broadcast(
            &mut state,
            conversation_id,
            RemoteEvent::Message(message.clone()),
        );
        message
    }

    /// Someone else (`user_id`) reacts to a message: changes it on the
    /// server and broadcasts it.
    pub fn inject_reaction(
        &self,
        conversation_id: &str,
        message_id: &str,
        user_id: &str,
        emoji: &str,
    ) {
        self.inject_change(conversation_id, message_id, |m, _| {
            m.reactions
                .entry(emoji.into())
                .or_default()
                .push(user_id.into());
            Ok(())
        });
    }

    /// The author edits a message elsewhere.
    pub fn inject_edit(&self, conversation_id: &str, message_id: &str, body: &str) {
        self.inject_change(conversation_id, message_id, |m, now| {
            m.body = body.into();
            m.edited_at = Some(now);
            Ok(())
        });
    }

    /// The author deletes a message elsewhere.
    pub fn inject_delete(&self, conversation_id: &str, message_id: &str) {
        self.inject_change(conversation_id, message_id, |m, _| {
            tombstone(m);
            Ok(())
        });
    }

    fn inject_change(
        &self,
        conversation_id: &str,
        message_id: &str,
        change: impl FnOnce(&mut Message, i64) -> Result<(), ChatError>,
    ) {
        let mut state = self.lock();
        let room = state
            .rooms
            .iter_mut()
            .find(|r| r.id == conversation_id)
            .expect("a room that exists");
        let message = room
            .change(message_id, change)
            .expect("a message that exists");
        broadcast(&mut state, conversation_id, RemoteEvent::Message(message));
    }

    /// Applies one of the client's changes: checks the room, the author
    /// (when this repo knows who the client is) and that it is not deleted.
    fn client_change(
        &self,
        conversation_id: &str,
        message_id: &str,
        call: String,
        authored: bool,
        change: impl FnOnce(&mut Message, i64, &str) -> Result<(), ChatError>,
    ) -> Result<Message, ChatError> {
        let mut state = self.lock();
        state.calls.push(call);
        if state.offline {
            return Err(offline());
        }
        let actor = state.reader.clone().unwrap_or_else(|| "me".into());
        let check_author = state.reader.is_some();
        let room = state
            .rooms
            .iter_mut()
            .find(|r| r.id == conversation_id)
            .ok_or_else(|| ChatError::NotFound {
                conversation_id: conversation_id.into(),
            })?;
        let message = room.change(message_id, |m, now| {
            if m.deleted {
                return Err(refused(409, "the message was deleted"));
            }
            if authored && check_author && m.author_id != actor {
                return Err(refused(403, "only its author can change a message"));
            }
            change(m, now, &actor)
        })?;
        broadcast(
            &mut state,
            conversation_id,
            RemoteEvent::Message(message.clone()),
        );
        Ok(message)
    }

    /// Streams currently held open by clients. Drops to zero once every
    /// client stream has been dropped, which is how tests prove that an
    /// unsubscribe really ended the Rust-side pump.
    /// With `false`, idle streams stay silent instead of yielding
    /// [`RemoteEvent::Heartbeat`] every [`MEMORY_HEARTBEAT`], so tests can
    /// prove cancellation does not depend on heartbeats. Applies to streams
    /// opened afterwards.
    pub fn set_heartbeats(&self, on: bool) {
        self.silent.store(!on, Ordering::SeqCst);
    }

    pub fn open_stream_count(&self) -> usize {
        self.open_streams.load(Ordering::SeqCst)
    }

    /// Every call made so far, as short strings.
    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    /// Messages the server holds for a room.
    pub fn messages(&self, conversation_id: &str) -> Vec<Message> {
        self.lock()
            .rooms
            .iter()
            .find(|r| r.id == conversation_id)
            .map(|r| r.messages.clone())
            .unwrap_or_default()
    }
}

/// A deleted message, as the server keeps it.
fn tombstone(message: &mut Message) {
    message.deleted = true;
    message.body.clear();
    message.reactions.clear();
    message.attachments.clear();
}

/// Adds or removes `user_id`'s `emoji`, dropping an emoji nobody uses.
pub(crate) fn toggle_reaction(message: &mut Message, emoji: &str, user_id: &str, on: bool) {
    if on {
        let users = message.reactions.entry(emoji.into()).or_default();
        if !users.iter().any(|u| u == user_id) {
            users.push(user_id.into());
        }
    } else if let Some(users) = message.reactions.get_mut(emoji) {
        users.retain(|u| u != user_id);
        if users.is_empty() {
            message.reactions.remove(emoji);
        }
    }
}

fn broadcast(state: &mut State, conversation_id: &str, event: RemoteEvent) {
    state.listeners.retain(|(room, listener)| {
        room != conversation_id || listener.send(Ok(event.clone())).is_ok()
    });
}

impl ChatRepo for InMemoryRepo {
    fn list_conversations(&self) -> Result<Vec<RemoteConversation>, ChatError> {
        let mut state = self.lock();
        state.calls.push("list".into());
        if state.offline {
            return Err(offline());
        }
        let reader = state.reader.clone();
        Ok(state
            .rooms
            .iter()
            .map(|room| room.remote(reader.as_deref()))
            .collect())
    }

    fn load_messages(
        &self,
        conversation_id: &str,
        since: Option<i64>,
    ) -> Result<Vec<Message>, ChatError> {
        let mut state = self.lock();
        state.calls.push(match since {
            Some(since) => format!("load {conversation_id} {since}"),
            None => format!("load {conversation_id}"),
        });
        if state.offline {
            return Err(offline());
        }
        let room = state
            .rooms
            .iter()
            .find(|r| r.id == conversation_id)
            .ok_or_else(|| ChatError::NotFound {
                conversation_id: conversation_id.into(),
            })?;
        Ok(room
            .messages
            .iter()
            .filter(|m| since.is_none_or(|since| m.changed_at() > since))
            .cloned()
            .collect())
    }

    fn post_message(&self, outgoing: &OutgoingMessage) -> Result<Message, ChatError> {
        let mut state = self.lock();
        state
            .calls
            .push(format!("post {} {}", outgoing.conversation_id, outgoing.id));
        if state.offline {
            return Err(offline());
        }
        if state.rejecting {
            return Err(refused(400, "the server refuses new messages"));
        }
        let cursors = state.reader.is_some();
        let State { rooms, blobs, .. } = &mut *state;
        let room = rooms
            .iter_mut()
            .find(|r| r.id == outgoing.conversation_id)
            .ok_or_else(|| ChatError::NotFound {
                conversation_id: outgoing.conversation_id.clone(),
            })?;
        if let Some(existing) = room.messages.iter().find(|m| m.id == outgoing.id) {
            return Ok(existing.clone());
        }
        let message = Message {
            id: outgoing.id.clone(),
            conversation_id: outgoing.conversation_id.clone(),
            author: outgoing.author.clone(),
            author_id: outgoing.author_id.clone(),
            body: outgoing.body.clone(),
            sent_at: 0,
            status: MessageStatus::Sent,
            reply_to: outgoing.reply_to.clone(),
            attachments: outgoing.attachments.clone(),
            ..Message::default()
        };
        if message
            .attachments
            .iter()
            .any(|a| !blobs.contains_key(&a.id))
        {
            return Err(refused(400, "an attachment was not uploaded first"));
        }
        if let Some(target) = &message.reply_to {
            if !room.messages.iter().any(|m| &m.id == target) {
                return Err(refused(400, "reply_to is not a message in this room"));
            }
        }
        let message = Message {
            sent_at: room.tick(),
            ..message
        };
        room.messages.push(message.clone());
        let read = match cursors && !message.author_id.is_empty() {
            true => room.mark_read(&message.author_id, message.sent_at),
            false => None,
        };
        broadcast(
            &mut state,
            &outgoing.conversation_id,
            RemoteEvent::Message(message.clone()),
        );
        if let Some(up_to) = read {
            broadcast(
                &mut state,
                &outgoing.conversation_id,
                RemoteEvent::Read {
                    user_id: message.author_id.clone(),
                    up_to,
                },
            );
        }
        Ok(message)
    }

    fn mark_read(&self, conversation_id: &str, up_to: i64) -> Result<(), ChatError> {
        let mut state = self.lock();
        state.calls.push(format!("read {conversation_id} {up_to}"));
        if state.offline {
            return Err(offline());
        }
        let Some(reader) = state.reader.clone() else {
            return Ok(());
        };
        let room = state
            .rooms
            .iter_mut()
            .find(|r| r.id == conversation_id)
            .ok_or_else(|| ChatError::NotFound {
                conversation_id: conversation_id.into(),
            })?;
        if let Some(up_to) = room.mark_read(&reader, up_to) {
            broadcast(
                &mut state,
                conversation_id,
                RemoteEvent::Read {
                    user_id: reader,
                    up_to,
                },
            );
        }
        Ok(())
    }

    fn create_conversation(&self, title: &str) -> Result<RemoteConversation, ChatError> {
        let title = crate::validate_title(title)?;
        let mut state = self.lock();
        state.calls.push(format!("create {title}"));
        if state.offline {
            return Err(offline());
        }
        state.next_room += 1;
        let room = Room {
            id: format!("room-{}", state.next_room),
            title,
            ..Room::default()
        };
        let remote = room.remote(state.reader.as_deref());
        state.rooms.push(room);
        Ok(remote)
    }

    fn edit_message(
        &self,
        conversation_id: &str,
        message_id: &str,
        body: &str,
    ) -> Result<Message, ChatError> {
        let body = body.trim().to_string();
        if body.is_empty() {
            return Err(ChatError::invalid("message body is empty"));
        }
        self.client_change(
            conversation_id,
            message_id,
            format!("edit {conversation_id} {message_id}"),
            true,
            |m, now, _| {
                m.body = body;
                m.edited_at = Some(now);
                Ok(())
            },
        )
    }

    fn delete_message(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<Message, ChatError> {
        self.client_change(
            conversation_id,
            message_id,
            format!("delete {conversation_id} {message_id}"),
            true,
            |m, _, _| {
                tombstone(m);
                Ok(())
            },
        )
    }

    fn react(
        &self,
        conversation_id: &str,
        message_id: &str,
        emoji: &str,
        on: bool,
    ) -> Result<Message, ChatError> {
        self.client_change(
            conversation_id,
            message_id,
            format!("react {conversation_id} {message_id} {emoji} {on}"),
            false,
            |m, _, actor| {
                toggle_reaction(m, emoji, actor, on);
                Ok(())
            },
        )
    }

    fn upload_attachment(&self, path: &std::path::Path, mime: &str) -> Result<String, ChatError> {
        let mut state = self.lock();
        state.calls.push(format!("upload {mime}"));
        if state.offline {
            return Err(offline());
        }
        let bytes = std::fs::read(path).map_err(|e| ChatError::invalid(e.to_string()))?;
        let id = {
            use sha2::Digest;
            sha2::Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        state.blobs.insert(id.clone(), bytes);
        Ok(id)
    }

    fn download_attachment(
        &self,
        id: &str,
        destination: &std::path::Path,
    ) -> Result<(), ChatError> {
        let mut state = self.lock();
        state.calls.push(format!("download {id}"));
        if state.offline {
            return Err(offline());
        }
        let bytes = state
            .blobs
            .get(id)
            .ok_or_else(|| refused(404, "no such attachment"))?;
        std::fs::write(destination, bytes).map_err(|e| ChatError::invalid(e.to_string()))
    }

    fn open_stream(
        &self,
        conversation_id: &str,
        cancel: &CancelHandle,
    ) -> Result<Box<dyn RemoteStream>, ChatError> {
        if cancel.is_cancelled() {
            return Err(ChatError::Cancelled);
        }
        let mut state = self.lock();
        state.calls.push(format!("stream {conversation_id}"));
        if state.offline {
            return Err(offline());
        }
        if !state.rooms.iter().any(|r| r.id == conversation_id) {
            return Err(ChatError::NotFound {
                conversation_id: conversation_id.into(),
            });
        }
        let (tx, rx) = mpsc::channel();
        state.listeners.push((conversation_id.into(), tx.clone()));
        drop(state);
        // Cancelling wakes a blocked `next_event` the way shutting a socket
        // down wakes a blocked read.
        let registration = cancel.on_cancel(move || {
            let _ = tx.send(Err(ChatError::Cancelled));
        });
        self.open_streams.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(MemoryStream {
            rx,
            open_streams: Arc::clone(&self.open_streams),
            heartbeat: (!self.silent.load(Ordering::SeqCst)).then_some(MEMORY_HEARTBEAT),
            finished: false,
            _registration: registration,
        }))
    }
}

struct MemoryStream {
    rx: Receiver<Result<RemoteEvent, ChatError>>,
    open_streams: Arc<AtomicUsize>,
    /// `None`: block until an event, like a server that never pings.
    heartbeat: Option<Duration>,
    finished: bool,
    _registration: AbortRegistration,
}

impl RemoteStream for MemoryStream {
    fn next_event(&mut self) -> Option<Result<RemoteEvent, ChatError>> {
        if self.finished {
            return None;
        }
        let next = match self.heartbeat {
            Some(heartbeat) => self.rx.recv_timeout(heartbeat),
            None => self.rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match next {
            Ok(Ok(event)) => Some(Ok(event)),
            Ok(Err(err)) => {
                self.finished = true;
                Some(Err(err))
            }
            Err(RecvTimeoutError::Timeout) => Some(Ok(RemoteEvent::Heartbeat)),
            Err(RecvTimeoutError::Disconnected) => {
                self.finished = true;
                None
            }
        }
    }
}

impl Drop for MemoryStream {
    fn drop(&mut self) {
        self.open_streams.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outgoing(id: &str, body: &str) -> OutgoingMessage {
        OutgoingMessage {
            id: id.into(),
            conversation_id: "general".into(),
            author: "me".into(),
            author_id: String::new(),
            body: body.into(),
            reply_to: None,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn post_is_idempotent_and_sent_at_increases() {
        let repo = InMemoryRepo::with_rooms(&[("general", "General")]);
        let a = repo.post_message(&outgoing("a", "one")).unwrap();
        let b = repo.post_message(&outgoing("b", "two")).unwrap();
        let again = repo.post_message(&outgoing("a", "changed")).unwrap();
        assert!(b.sent_at > a.sent_at);
        assert_eq!(again, a);
        assert_eq!(repo.messages("general").len(), 2);
    }

    #[test]
    fn since_is_exclusive() {
        let repo = InMemoryRepo::with_rooms(&[("general", "General")]);
        let a = repo.inject("general", "ana", "hi");
        let b = repo.inject("general", "ana", "there");
        let got = repo.load_messages("general", Some(a.sent_at)).unwrap();
        assert_eq!(got, vec![b]);
    }

    #[test]
    fn unknown_room_is_not_found() {
        let repo = InMemoryRepo::with_rooms(&[("general", "General")]);
        assert!(matches!(
            repo.load_messages("nope", None),
            Err(ChatError::NotFound { .. })
        ));
        assert!(matches!(
            repo.open_stream("nope", &CancelHandle::new()).map(|_| ()),
            Err(ChatError::NotFound { .. })
        ));
    }

    #[test]
    fn cancel_wakes_a_silent_stream() {
        let repo = InMemoryRepo::with_rooms(&[("general", "General")]);
        repo.set_heartbeats(false);
        let cancel = CancelHandle::new();
        let mut stream = repo.open_stream("general", &cancel).unwrap();
        let reader = std::thread::spawn(move || stream.next_event());
        std::thread::sleep(Duration::from_millis(30));
        cancel.cancel();
        assert_eq!(reader.join().unwrap(), Some(Err(ChatError::Cancelled)));
        assert_eq!(repo.open_stream_count(), 0);
        assert_eq!(
            repo.open_stream("general", &cancel).map(|_| ()),
            Err(ChatError::Cancelled)
        );
    }

    #[test]
    fn stream_delivers_heartbeats_messages_and_offline_errors() {
        let repo = InMemoryRepo::with_rooms(&[("general", "General")]);
        let mut stream = repo.open_stream("general", &CancelHandle::new()).unwrap();
        assert_eq!(repo.open_stream_count(), 1);
        assert_eq!(stream.next_event(), Some(Ok(RemoteEvent::Heartbeat)));
        let m = repo.inject("general", "ana", "hi");
        assert_eq!(stream.next_event(), Some(Ok(RemoteEvent::Message(m))));
        repo.set_offline(true);
        assert!(matches!(
            stream.next_event(),
            Some(Err(ChatError::Network { .. }))
        ));
        assert_eq!(stream.next_event(), None);
        drop(stream);
        assert_eq!(repo.open_stream_count(), 0);
        assert!(repo.list_conversations().is_err());
    }
}
