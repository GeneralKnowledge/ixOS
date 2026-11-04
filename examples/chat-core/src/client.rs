//! [`ChatClient`]: commands, per-room stream pumps, the store, and one
//! event sink.
//!
//! Concurrency contract (the part the FFI layer will rely on):
//!
//! * Commands are blocking and run on the caller's thread. The FFI layer
//!   runs each on a worker thread, as Weather does, so hosts never block.
//! * All state lives behind one mutex, and **every event is emitted while
//!   holding it**, right after the store change it describes. So events are
//!   totally ordered, and a host that re-reads the store after an event never
//!   sees older data than the event.
//! * `subscribe` starts one pump thread per room. `unsubscribe` removes the
//!   subscription and emits `StreamDisconnected { will_retry: false }` under
//!   the lock; after it returns **no event from that pump is ever emitted**
//!   (each pump checks it is still the current subscription, under the same
//!   lock, before every change). The pump thread itself stops at its next
//!   wake-up: the next stream event or heartbeat, or the end of a backoff
//!   slice. A blocked read cannot be interrupted (ureq has no cancel), which
//!   is why the server sends heartbeats. [`ChatClient::live_pumps`] counts
//!   pump threads still running, so tests can prove they end.
//! * Leaving a room does not cancel a send in flight: the send finishes and
//!   its `MessageStatusChanged` is still emitted (the host filters by room).
//! * Every message carries a client-chosen id, so retries are idempotent on
//!   the server and the stream echo of our own message is matched to the
//!   local bubble instead of appearing twice.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crate::now_ms;
use crate::repo::{ChatRepo, OutgoingMessage, RemoteConversation, RemoteEvent};
use crate::store::{ChatSnapshot, ChatStore, StoreError, Upsert};
use crate::types::{ChatError, ChatEvent, Conversation, Message, MessageStatus};

/// Longest message body accepted, in characters.
pub const MAX_BODY_CHARS: usize = 4000;

/// How long a pump sleeps between checks while backing off.
const BACKOFF_SLICE: Duration = Duration::from_millis(10);

#[derive(Debug, Clone)]
pub struct ChatConfig {
    /// The local user's display name, sent as `author`.
    pub user: String,
    /// First reconnect delay after a stream drops; doubles up to
    /// `reconnect_max`, and resets once a stream connects and catches up.
    pub reconnect_initial: Duration,
    pub reconnect_max: Duration,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            user: "me".into(),
            reconnect_initial: Duration::from_millis(500),
            reconnect_max: Duration::from_secs(5),
        }
    }
}

/// Receives every [`ChatEvent`]. Called with the client's lock held, so it
/// must be quick and must not call back into the client: push onto a queue
/// and return (the FFI layer's bounded subscription queue does exactly that).
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: ChatEvent);
}

impl<F: Fn(ChatEvent) + Send + Sync + 'static> EventSink for F {
    fn emit(&self, event: ChatEvent) {
        self(event)
    }
}

/// The chat engine. Cheap to clone; clones share one instance.
#[derive(Clone)]
pub struct ChatClient {
    inner: Arc<Inner>,
}

struct Inner {
    repo: Arc<dyn ChatRepo>,
    store: ChatStore,
    config: ChatConfig,
    sink: Box<dyn EventSink>,
    state: Mutex<State>,
    live_pumps: AtomicUsize,
    next_id: AtomicU64,
}

struct State {
    snapshot: ChatSnapshot,
    subscriptions: HashMap<String, Arc<Subscription>>,
    shut_down: bool,
    store_error: Option<StoreError>,
}

struct Subscription {
    conversation_id: String,
    cancelled: AtomicBool,
}

impl Subscription {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Decrements the live-pump count when a pump thread ends, however it ends.
struct PumpGuard<'a>(&'a AtomicUsize);

impl Drop for PumpGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn validate_room(conversation_id: &str) -> Result<(), ChatError> {
    if conversation_id.trim().is_empty() {
        return Err(ChatError::invalid("conversation id is empty"));
    }
    Ok(())
}

impl ChatClient {
    /// Opens the store and builds a client.
    ///
    /// A store written by a *newer* build fails with
    /// [`StoreError::UnsupportedVersion`] (it is never overwritten). A corrupt
    /// store is set aside: the client starts empty, since the server is the
    /// source of truth, and [`store_error`](Self::store_error) reports why.
    pub fn open(
        repo: Arc<dyn ChatRepo>,
        store: ChatStore,
        config: ChatConfig,
        sink: impl EventSink,
    ) -> Result<Self, StoreError> {
        let (snapshot, store_error) = match store.load() {
            Ok(snapshot) => (snapshot, None),
            Err(err @ StoreError::UnsupportedVersion { .. }) => return Err(err),
            Err(err) => (ChatSnapshot::new(), Some(err)),
        };
        Ok(Self {
            inner: Arc::new(Inner {
                repo,
                store,
                config,
                sink: Box::new(sink),
                state: Mutex::new(State {
                    snapshot,
                    subscriptions: HashMap::new(),
                    shut_down: false,
                    store_error,
                }),
                live_pumps: AtomicUsize::new(0),
                next_id: AtomicU64::new(0),
            }),
        })
    }

    pub fn config(&self) -> &ChatConfig {
        &self.inner.config
    }

    // ----- Reads (local, immediate) -------------------------------------

    /// Conversations as last stored, for a first render before the network.
    pub fn cached_conversations(&self) -> Vec<Conversation> {
        self.inner.lock().snapshot.conversations.clone()
    }

    /// Messages of a conversation as stored, oldest first.
    pub fn cached_messages(&self, conversation_id: &str) -> Vec<Message> {
        self.inner
            .lock()
            .snapshot
            .messages(conversation_id)
            .to_vec()
    }

    /// The `since` cursor for a conversation (see [`ChatSnapshot`]).
    pub fn last_synced(&self, conversation_id: &str) -> Option<i64> {
        self.inner.lock().snapshot.last_synced(conversation_id)
    }

    pub fn is_subscribed(&self, conversation_id: &str) -> bool {
        self.inner
            .lock()
            .subscriptions
            .contains_key(conversation_id)
    }

    /// Pump threads still running (subscribed, or unsubscribed but not yet
    /// woken). Drops to zero after every subscription has ended.
    pub fn live_pumps(&self) -> usize {
        self.inner.live_pumps.load(Ordering::SeqCst)
    }

    /// The last persistence problem (a corrupt store at open, or a failed
    /// save). Persistence failures never fail a command.
    pub fn store_error(&self) -> Option<StoreError> {
        self.inner.lock().store_error.clone()
    }

    // ----- Commands -------------------------------------------------------

    /// Lists conversations from the server. For each room the server says
    /// has newer messages than we have, and that is not open, the new
    /// messages are fetched too: they count towards `unread_count` and are
    /// announced with `MessageReceived`. A failed prefetch is skipped, not
    /// fatal.
    pub fn list_conversations(&self) -> Result<Vec<Conversation>, ChatError> {
        self.inner.ensure_running()?;
        let remote = self.inner.repo.list_conversations()?;

        let to_fetch: Vec<(String, Option<i64>)> = {
            let state = self.inner.lock();
            remote
                .iter()
                .filter(|room| !state.subscriptions.contains_key(&room.id))
                .filter_map(|room| {
                    let cursor = state.snapshot.last_synced(&room.id);
                    let newer = match (room.last_message_at, cursor) {
                        (Some(server), Some(cursor)) => server > cursor,
                        (Some(_), None) => true,
                        (None, _) => false,
                    };
                    newer.then(|| (room.id.clone(), cursor))
                })
                .collect()
        };
        let fetched: Vec<(String, Option<i64>, Vec<Message>)> = to_fetch
            .into_iter()
            .filter_map(|(id, since)| {
                let messages = self.inner.repo.load_messages(&id, since).ok()?;
                Some((id, since, messages))
            })
            .collect();

        let mut state = self.inner.lock();
        self.inner.ensure_running_locked(&state)?;
        self.inner.replace_conversations(&mut state, &remote);
        for (id, since, messages) in fetched {
            self.inner
                .apply_server_messages(&mut state, &id, messages, Some(since));
        }
        self.inner.persist(&mut state);
        Ok(state.snapshot.conversations.clone())
    }

    /// Fetches a conversation's messages (`since`: only newer than that
    /// server time; `None`: all), merges them into the store, and returns
    /// the conversation's full stored list, oldest first, including local
    /// `Sending`/`Failed` messages. New messages are also announced with
    /// `MessageReceived`. Pass [`last_synced`](Self::last_synced) as `since`
    /// for an incremental sync, or use [`sync_messages`](Self::sync_messages).
    pub fn load_messages(
        &self,
        conversation_id: &str,
        since: Option<i64>,
    ) -> Result<Vec<Message>, ChatError> {
        validate_room(conversation_id)?;
        self.inner.ensure_running()?;
        let messages = self.inner.repo.load_messages(conversation_id, since)?;
        let mut state = self.inner.lock();
        self.inner.ensure_running_locked(&state)?;
        self.inner
            .apply_server_messages(&mut state, conversation_id, messages, Some(since));
        self.inner.persist(&mut state);
        Ok(state.snapshot.messages(conversation_id).to_vec())
    }

    /// `load_messages(id, last_synced(id))`.
    pub fn sync_messages(&self, conversation_id: &str) -> Result<Vec<Message>, ChatError> {
        let since = self.last_synced(conversation_id);
        self.load_messages(conversation_id, since)
    }

    /// Sends a message. It is stored and announced (`MessageReceived`,
    /// status `Sending`) before the network call, then becomes `Sent` or
    /// `Failed` (`MessageStatusChanged`). Returns the final message, or the
    /// error that failed it. If the client is shut down while the request
    /// is in flight and it fails, the error is `Cancelled` and the message
    /// stays `Failed` in the store for the next run.
    pub fn send_message(&self, conversation_id: &str, body: &str) -> Result<Message, ChatError> {
        validate_room(conversation_id)?;
        let body = body.trim();
        if body.is_empty() {
            return Err(ChatError::invalid("message body is empty"));
        }
        if body.chars().count() > MAX_BODY_CHARS {
            return Err(ChatError::invalid(format!(
                "message body is longer than {MAX_BODY_CHARS} characters"
            )));
        }
        let message = Message {
            id: self.inner.new_message_id(),
            conversation_id: conversation_id.into(),
            author: self.inner.config.user.clone(),
            body: body.into(),
            sent_at: now_ms(),
            status: MessageStatus::Sending,
        };
        {
            let mut state = self.inner.lock();
            self.inner.ensure_running_locked(&state)?;
            state.snapshot.upsert_message(message.clone());
            self.inner.persist(&mut state);
            self.inner.emit(ChatEvent::MessageReceived(message.clone()));
        }
        self.inner.deliver(message)
    }

    /// Sends a `Failed` message again, with the same id (the server ignores
    /// a duplicate). A message that is already `Sent` is returned as is.
    pub fn retry_message(&self, message_id: &str) -> Result<Message, ChatError> {
        let message = {
            let mut state = self.inner.lock();
            self.inner.ensure_running_locked(&state)?;
            let message =
                state.snapshot.message(message_id).cloned().ok_or_else(|| {
                    ChatError::invalid(format!("no message with id {message_id:?}"))
                })?;
            match message.status {
                MessageStatus::Sent => return Ok(message),
                MessageStatus::Sending => {
                    return Err(ChatError::invalid(format!(
                        "message {message_id:?} is already being sent"
                    )))
                }
                MessageStatus::Failed => {}
            }
            let message = state
                .snapshot
                .set_status(message_id, MessageStatus::Sending)
                .expect("message exists");
            self.inner.persist(&mut state);
            self.inner.emit(ChatEvent::MessageStatusChanged {
                id: message.id.clone(),
                status: MessageStatus::Sending,
            });
            message
        };
        self.inner.deliver(message)
    }

    /// Opens the room's live stream on a pump thread, and marks the room
    /// read (`unread_count` → 0). Returns at once; `StreamConnected` /
    /// `StreamDisconnected` report how the connection goes. Idempotent.
    pub fn subscribe(&self, conversation_id: &str) -> Result<(), ChatError> {
        validate_room(conversation_id)?;
        let subscription = {
            let mut state = self.inner.lock();
            self.inner.ensure_running_locked(&state)?;
            if state.subscriptions.contains_key(conversation_id) {
                return Ok(());
            }
            let subscription = Arc::new(Subscription {
                conversation_id: conversation_id.into(),
                cancelled: AtomicBool::new(false),
            });
            state
                .subscriptions
                .insert(conversation_id.into(), Arc::clone(&subscription));
            if let Some(conversation) = state.snapshot.conversation_mut(conversation_id) {
                if conversation.unread_count > 0 {
                    conversation.unread_count = 0;
                    let conversation = conversation.clone();
                    self.inner.persist(&mut state);
                    self.inner
                        .emit(ChatEvent::ConversationUpdated(conversation));
                }
            }
            subscription
        };
        self.inner.live_pumps.fetch_add(1, Ordering::SeqCst);
        let inner = Arc::clone(&self.inner);
        let spawned = thread::Builder::new()
            .name(format!("forge-chat-stream-{conversation_id}"))
            .spawn(move || inner.pump(subscription));
        if let Err(err) = spawned {
            self.inner.live_pumps.fetch_sub(1, Ordering::SeqCst);
            let mut state = self.inner.lock();
            state.subscriptions.remove(conversation_id);
            return Err(ChatError::UnsupportedOnHost {
                detail: format!("could not start the stream thread: {err}"),
            });
        }
        Ok(())
    }

    /// Ends the room's subscription. On return, `StreamDisconnected
    /// { will_retry: false }` has been emitted and no further event from
    /// that subscription will be. Idempotent; allowed after shutdown.
    pub fn unsubscribe(&self, conversation_id: &str) -> Result<(), ChatError> {
        let mut state = self.inner.lock();
        if let Some(subscription) = state.subscriptions.remove(conversation_id) {
            subscription.cancelled.store(true, Ordering::SeqCst);
            self.inner.emit(ChatEvent::StreamDisconnected {
                conversation_id: conversation_id.into(),
                reason: "unsubscribed".into(),
                will_retry: false,
            });
        }
        Ok(())
    }

    /// Ends every subscription and refuses further commands with
    /// [`ChatError::ShutDown`]. Idempotent.
    pub fn shutdown(&self) {
        let mut state = self.inner.lock();
        if state.shut_down {
            return;
        }
        state.shut_down = true;
        let mut ended: Vec<_> = state.subscriptions.drain().collect();
        ended.sort_by(|a, b| a.0.cmp(&b.0));
        for (conversation_id, subscription) in ended {
            subscription.cancelled.store(true, Ordering::SeqCst);
            self.inner.emit(ChatEvent::StreamDisconnected {
                conversation_id,
                reason: "shut down".into(),
                will_retry: false,
            });
        }
        self.inner.persist(&mut state);
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn ensure_running(&self) -> Result<(), ChatError> {
        self.ensure_running_locked(&self.lock())
    }

    fn ensure_running_locked(&self, state: &State) -> Result<(), ChatError> {
        if state.shut_down {
            Err(ChatError::ShutDown)
        } else {
            Ok(())
        }
    }

    fn emit(&self, event: ChatEvent) {
        self.sink.emit(event);
    }

    fn persist(&self, state: &mut State) {
        if let Err(err) = self.store.save(&state.snapshot) {
            state.store_error = Some(err);
        }
    }

    /// Unique across runs: time, process and a counter.
    fn new_message_id(&self) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = self.next_id.fetch_add(1, Ordering::Relaxed);
        format!("m-{nanos:x}-{:x}-{n:x}", std::process::id())
    }

    /// Replaces the conversation list with the server's, keeping local
    /// `unread_count`. Rooms the server no longer lists are forgotten.
    fn replace_conversations(&self, state: &mut State, remote: &[RemoteConversation]) {
        let conversations: Vec<Conversation> = remote
            .iter()
            .map(|room| {
                let existing = state.snapshot.conversation(&room.id);
                Conversation {
                    id: room.id.clone(),
                    title: room.title.clone(),
                    last_message_at: max_opt(
                        room.last_message_at,
                        existing.and_then(|c| c.last_message_at),
                    ),
                    unread_count: existing.map_or(0, |c| c.unread_count),
                }
            })
            .collect();
        let listed: Vec<&str> = conversations.iter().map(|c| c.id.as_str()).collect();
        state
            .snapshot
            .messages
            .retain(|id, _| listed.contains(&id.as_str()));
        state
            .snapshot
            .last_synced
            .retain(|id, _| listed.contains(&id.as_str()));
        state.snapshot.conversations = conversations;
    }

    /// Merges server messages for one room, emitting one event per change.
    ///
    /// `fetched_since` is `Some(since)` when `messages` is the complete
    /// answer to a fetch with that `since`, and `None` for a single pushed
    /// message from a stream that has been open since the last catch-up.
    /// Either way the cursor only advances when no gap can be left behind:
    /// a fetch whose `since` is past our cursor does not move it.
    fn apply_server_messages(
        &self,
        state: &mut State,
        conversation_id: &str,
        messages: Vec<Message>,
        fetched_since: Option<Option<i64>>,
    ) {
        let viewing = state.subscriptions.contains_key(conversation_id);
        let contiguous = match fetched_since {
            None | Some(None) => true,
            Some(Some(since)) => state
                .snapshot
                .last_synced(conversation_id)
                .is_some_and(|cursor| since <= cursor),
        };
        let mut newest = None;
        let mut unread_added = 0u32;
        for mut message in messages {
            message.status = MessageStatus::Sent;
            newest = newest.max(Some(message.sent_at));
            match state.snapshot.upsert_message(message.clone()) {
                Upsert::Inserted => {
                    if !viewing && message.author != self.config.user {
                        unread_added += 1;
                    }
                    self.emit(ChatEvent::MessageReceived(message));
                }
                Upsert::StatusChanged => self.emit(ChatEvent::MessageStatusChanged {
                    id: message.id,
                    status: MessageStatus::Sent,
                }),
                // Same id, same status, different content (e.g. the server
                // time replaced ours): re-announce; hosts upsert by id.
                Upsert::Updated => self.emit(ChatEvent::MessageReceived(message)),
                Upsert::Unchanged => {}
            }
        }
        if let (true, Some(newest)) = (contiguous, newest) {
            state.snapshot.advance_cursor(conversation_id, newest);
        }
        self.touch_conversation(state, conversation_id, newest, unread_added);
    }

    /// Bumps `last_message_at` / `unread_count`; emits `ConversationUpdated`
    /// when either changed.
    fn touch_conversation(
        &self,
        state: &mut State,
        conversation_id: &str,
        newest: Option<i64>,
        unread_added: u32,
    ) {
        let Some(conversation) = state.snapshot.conversation_mut(conversation_id) else {
            return;
        };
        let before = conversation.clone();
        conversation.last_message_at = max_opt(conversation.last_message_at, newest);
        conversation.unread_count = conversation.unread_count.saturating_add(unread_added);
        if *conversation != before {
            let conversation = conversation.clone();
            self.emit(ChatEvent::ConversationUpdated(conversation));
        }
    }

    /// Posts a message that is already stored as `Sending`, then records the
    /// outcome.
    fn deliver(&self, message: Message) -> Result<Message, ChatError> {
        let result = self.repo.post_message(&OutgoingMessage {
            id: message.id.clone(),
            conversation_id: message.conversation_id.clone(),
            author: message.author.clone(),
            body: message.body.clone(),
        });
        let mut state = self.lock();
        let shut_down = state.shut_down;
        match result {
            Ok(mut stored) => {
                stored.status = MessageStatus::Sent;
                let outcome = state.snapshot.upsert_message(stored.clone());
                if !shut_down {
                    match outcome {
                        Upsert::StatusChanged => self.emit(ChatEvent::MessageStatusChanged {
                            id: stored.id.clone(),
                            status: MessageStatus::Sent,
                        }),
                        Upsert::Updated | Upsert::Inserted => {
                            self.emit(ChatEvent::MessageReceived(stored.clone()))
                        }
                        Upsert::Unchanged => {}
                    }
                    // Our own send does not move the cursor: messages from
                    // others may sit between the cursor and this one.
                    self.touch_conversation(
                        &mut state,
                        &stored.conversation_id,
                        Some(stored.sent_at),
                        0,
                    );
                }
                self.persist(&mut state);
                Ok(stored)
            }
            Err(err) => {
                // The stream echo may have confirmed it while the response
                // was lost; then it was sent after all.
                if let Some(current) = state.snapshot.message(&message.id) {
                    if current.status == MessageStatus::Sent {
                        return Ok(current.clone());
                    }
                }
                state
                    .snapshot
                    .set_status(&message.id, MessageStatus::Failed);
                self.persist(&mut state);
                if shut_down {
                    return Err(ChatError::Cancelled);
                }
                self.emit(ChatEvent::MessageStatusChanged {
                    id: message.id,
                    status: MessageStatus::Failed,
                });
                Err(err)
            }
        }
    }

    /// Runs `f` under the lock only if `subscription` is still the room's
    /// current subscription. `None` means the pump must stop.
    fn with_current<R>(
        &self,
        subscription: &Arc<Subscription>,
        f: impl FnOnce(&Self, &mut State) -> R,
    ) -> Option<R> {
        let mut state = self.lock();
        let current = !state.shut_down
            && !subscription.is_cancelled()
            && state
                .subscriptions
                .get(&subscription.conversation_id)
                .is_some_and(|s| Arc::ptr_eq(s, subscription));
        current.then(|| f(self, &mut state))
    }

    /// Ends a subscription from inside its pump (the room does not exist).
    fn end_subscription(&self, subscription: &Arc<Subscription>, reason: String) {
        self.with_current(subscription, |inner, state| {
            state.subscriptions.remove(&subscription.conversation_id);
            subscription.cancelled.store(true, Ordering::SeqCst);
            inner.emit(ChatEvent::StreamDisconnected {
                conversation_id: subscription.conversation_id.clone(),
                reason,
                will_retry: false,
            });
        });
    }

    fn disconnected(&self, subscription: &Arc<Subscription>, reason: String) -> Option<()> {
        self.with_current(subscription, |inner, _| {
            inner.emit(ChatEvent::StreamDisconnected {
                conversation_id: subscription.conversation_id.clone(),
                reason,
                will_retry: true,
            })
        })
    }

    /// Sleeps for `delay`, waking early if the subscription ends. `false`
    /// if it ended.
    fn backoff(&self, subscription: &Subscription, delay: Duration) -> bool {
        let deadline = Instant::now() + delay;
        while Instant::now() < deadline {
            if subscription.is_cancelled() {
                return false;
            }
            thread::sleep(BACKOFF_SLICE.min(deadline.saturating_duration_since(Instant::now())));
        }
        !subscription.is_cancelled()
    }

    /// The pump: connect, catch up, follow the stream; on failure back off
    /// and reconnect, until the subscription ends.
    fn pump(self: Arc<Self>, subscription: Arc<Subscription>) {
        let _guard = PumpGuard(&self.live_pumps);
        let room = subscription.conversation_id.clone();
        let mut delay = self.config.reconnect_initial;
        // One `StreamDisconnected` per outage, not one per failed attempt.
        let mut outage_reported = false;

        while !subscription.is_cancelled() {
            match self.repo.open_stream(&room) {
                Ok(mut stream) => {
                    let connected = self.with_current(&subscription, |inner, _| {
                        inner.emit(ChatEvent::StreamConnected {
                            conversation_id: room.clone(),
                        })
                    });
                    if connected.is_none() {
                        return;
                    }

                    // Catch up *after* the stream is open, so nothing stored
                    // in between is missed (duplicates are merged by id).
                    let Some(since) = self
                        .with_current(&subscription, |_, state| state.snapshot.last_synced(&room))
                    else {
                        return;
                    };
                    let reason = match self.repo.load_messages(&room, since) {
                        Ok(messages) => {
                            let applied = self.with_current(&subscription, |inner, state| {
                                inner.apply_server_messages(state, &room, messages, Some(since));
                                inner.persist(state);
                            });
                            if applied.is_none() {
                                return;
                            }
                            delay = self.config.reconnect_initial;
                            match self.follow(&subscription, stream.as_mut()) {
                                Some(reason) => reason,
                                None => return,
                            }
                        }
                        Err(ChatError::NotFound { .. }) => {
                            self.end_subscription(&subscription, "conversation not found".into());
                            return;
                        }
                        Err(err) => format!("catch-up failed: {err}"),
                    };
                    drop(stream);
                    if self.disconnected(&subscription, reason).is_none() {
                        return;
                    }
                    outage_reported = true;
                }
                Err(ChatError::NotFound { .. }) => {
                    self.end_subscription(&subscription, "conversation not found".into());
                    return;
                }
                Err(err) => {
                    if !outage_reported {
                        if self.disconnected(&subscription, err.to_string()).is_none() {
                            return;
                        }
                        outage_reported = true;
                    }
                }
            }
            if !self.backoff(&subscription, delay) {
                return;
            }
            delay = (delay * 2).min(self.config.reconnect_max);
        }
    }

    /// Applies stream events until the stream ends (`Some(reason)`) or the
    /// subscription does (`None`).
    fn follow(
        &self,
        subscription: &Arc<Subscription>,
        stream: &mut dyn crate::repo::RemoteStream,
    ) -> Option<String> {
        let room = &subscription.conversation_id;
        loop {
            if subscription.is_cancelled() {
                return None;
            }
            let event = match stream.next_event() {
                None => return Some("stream closed by server".into()),
                Some(Err(err)) => return Some(err.to_string()),
                Some(Ok(event)) => event,
            };
            match event {
                RemoteEvent::Heartbeat => {}
                RemoteEvent::Message(message) => {
                    if message.conversation_id != *room {
                        continue;
                    }
                    self.with_current(subscription, |inner, state| {
                        inner.apply_server_messages(state, room, vec![message], None);
                        inner.persist(state);
                    })?;
                }
                RemoteEvent::Conversation(remote) => {
                    self.with_current(subscription, |inner, state| {
                        if let Some(conversation) = state.snapshot.conversation_mut(&remote.id) {
                            let before = conversation.clone();
                            conversation.title = remote.title.clone();
                            conversation.last_message_at =
                                max_opt(conversation.last_message_at, remote.last_message_at);
                            if *conversation != before {
                                let conversation = conversation.clone();
                                inner.persist(state);
                                inner.emit(ChatEvent::ConversationUpdated(conversation));
                            }
                        }
                    })?;
                }
            }
        }
    }
}

fn max_opt(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    a.max(b)
}

#[cfg(test)]
mod tests;
