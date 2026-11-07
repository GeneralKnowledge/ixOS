//! In-memory [`ChatRepo`]: the test double, and the reference for what the
//! mock server must do.

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
    rooms: Vec<Room>,
    listeners: Vec<(String, Sender<Result<RemoteEvent, ChatError>>)>,
    /// Calls made, for assertions: `"list"`, `"load <id> <since>"`, ...
    calls: Vec<String>,
}

struct Room {
    id: String,
    title: String,
    messages: Vec<Message>,
}

impl Room {
    fn remote(&self) -> RemoteConversation {
        RemoteConversation {
            id: self.id.clone(),
            title: self.title.clone(),
            last_message_at: self.messages.last().map(|m| m.sent_at),
        }
    }

    fn next_sent_at(&self) -> i64 {
        let last = self.messages.last().map_or(i64::MIN, |m| m.sent_at);
        now_ms().max(last.saturating_add(1))
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
                    messages: Vec::new(),
                });
            }
        }
        repo
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
            sent_at: room.next_sent_at(),
            status: MessageStatus::Sent,
        };
        room.messages.push(message.clone());
        broadcast(
            &mut state,
            conversation_id,
            RemoteEvent::Message(message.clone()),
        );
        message
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
        Ok(state.rooms.iter().map(Room::remote).collect())
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
            .filter(|m| since.is_none_or(|since| m.sent_at > since))
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
        let room = state
            .rooms
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
            sent_at: room.next_sent_at(),
            status: MessageStatus::Sent,
        };
        room.messages.push(message.clone());
        broadcast(
            &mut state,
            &outgoing.conversation_id,
            RemoteEvent::Message(message.clone()),
        );
        Ok(message)
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
