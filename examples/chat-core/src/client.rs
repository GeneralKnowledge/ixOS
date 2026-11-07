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
//!   lock, before every change). Each subscription owns a
//!   [`forge_net::CancelHandle`]; ending the subscription cancels it, which
//!   closes the room's connection (a read blocked on a silent stream fails
//!   at once), aborts a connect in progress and wakes a reconnect backoff.
//!   So the pump thread exits promptly without waiting for a heartbeat, and
//!   other rooms' connections are untouched. [`ChatClient::live_pumps`]
//!   counts pump threads still running, so tests can prove they end.
//! * A room's stream runs while the room is **open** (`subscribe`) or
//!   **watched** (`watch`), or both. Opening marks the room read and keeps
//!   it read; watching only keeps it live, so new messages from others still
//!   count towards `unread_count` and raise `Notify`. The stream stops,
//!   with `StreamDisconnected { will_retry: false }`, when the room is
//!   neither: `unsubscribe` on a watched room only closes it.
//! * Leaving a room does not cancel a send in flight: the send finishes and
//!   its `MessageStatusChanged` is still emitted (it names the room, so the
//!   host can ignore it or update that room's list).
//! * Every message carries a client-chosen id, so retries are idempotent on
//!   the server and the stream echo of our own message is matched to the
//!   local bubble instead of appearing twice.
//! * **The outbox.** A message that cannot be sent because the server is
//!   unreachable (or answered 5xx) stays `Sending` in the store. One outbox
//!   thread per client sends what is queued, oldest first, when a stream
//!   reconnects, when streams resume, after a successful refresh, and with
//!   backoff in between. It also sends what a previous run left queued, so a
//!   message typed offline goes out after the app was killed and relaunched.
//!   The server ignores a repeated id, so a send that succeeded but whose
//!   answer was lost is never duplicated.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::Duration;

use forge_net::{Backoff, CancelHandle};

use crate::attachments::{AttachmentStore, OutgoingFile, MAX_ATTACHMENTS_PER_MESSAGE};
use crate::now_ms;
use crate::repo::{ChatRepo, OutgoingMessage, RemoteConversation, RemoteEvent};
use crate::store::{ChatSnapshot, ChatStore, StoreError, Upsert};
use crate::types::{ChatError, ChatEvent, Conversation, Message, MessageStatus};

/// Longest message body accepted, in characters.
pub const MAX_BODY_CHARS: usize = 4000;

/// Longest reaction accepted, in characters (one emoji with modifiers).
pub const MAX_REACTION_CHARS: usize = 8;

#[derive(Debug, Clone)]
pub struct ChatConfig {
    /// The local user's display name, sent as `author`.
    pub user: String,
    /// The local user's account id on the server; empty without an account.
    /// With an id, "is this message mine" compares ids, not names, so two
    /// people may share a display name.
    pub user_id: String,
    /// First reconnect delay after a stream drops; doubles up to
    /// `reconnect_max`, and resets once a stream connects and catches up.
    /// Each delay is cut by up to 20% at random (`forge_net::Backoff`).
    pub reconnect_initial: Duration,
    pub reconnect_max: Duration,
}

impl ChatConfig {
    /// Whether the local user wrote `message`: by account id when this
    /// client has one, by display name otherwise.
    pub fn is_own(&self, message: &Message) -> bool {
        if self.user_id.is_empty() {
            message.author == self.user
        } else {
            message.author_id == self.user_id
        }
    }
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            user: "me".into(),
            user_id: String::new(),
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
    /// Unix ms when this client was built: only messages sent after it
    /// raise `Notify`.
    started_at: i64,
    /// Wakes the outbox thread.
    outbox: Arc<OutboxSignal>,
    /// Attachment files on this device.
    attachments: AttachmentStore,
}

/// The outbox thread's doorbell.
#[derive(Default)]
struct OutboxSignal {
    rung: Mutex<bool>,
    bell: Condvar,
}

impl OutboxSignal {
    fn ring(&self) {
        *self.rung.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.bell.notify_all();
    }

    /// Waits until rung or `timeout` passes.
    fn wait(&self, timeout: Duration) {
        let rung = self.rung.lock().unwrap_or_else(|p| p.into_inner());
        let (mut rung, _) = self
            .bell
            .wait_timeout_while(rung, timeout, |rung| !*rung)
            .unwrap_or_else(|p| p.into_inner());
        *rung = false;
    }
}

/// First retry delay of the outbox after the server could not be reached;
/// it doubles up to [`OUTBOX_MAX_DELAY`].
pub const OUTBOX_INITIAL_DELAY: Duration = Duration::from_secs(1);
/// Longest wait between outbox attempts.
pub const OUTBOX_MAX_DELAY: Duration = Duration::from_secs(60);
/// How often an idle outbox thread checks that its client still exists.
const OUTBOX_IDLE: Duration = Duration::from_secs(30);

struct State {
    snapshot: ChatSnapshot,
    /// Running streams: rooms that are open, watched, or both.
    subscriptions: HashMap<String, Arc<Subscription>>,
    /// Rooms the host has open (`subscribe`): kept read.
    open: HashSet<String>,
    /// Rooms kept live in the background (`watch`).
    watched: HashSet<String>,
    /// Set by `pause_streams`: no stream runs, but `open` and `watched`
    /// are kept so `resume_streams` restarts them.
    paused: bool,
    shut_down: bool,
    store_error: Option<StoreError>,
    /// Ids of messages being posted right now (by a command or the outbox),
    /// so one message is never posted twice at once.
    in_flight: HashSet<String>,
    /// Attachments this client has uploaded, so a retry does not send the
    /// bytes again.
    uploaded: HashSet<String>,
}

struct Subscription {
    conversation_id: String,
    /// Cancelled when the subscription ends; closes its live connection.
    cancel: CancelHandle,
}

impl Subscription {
    fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
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
        let attachments = store.attachments();
        let client = Self {
            inner: Arc::new(Inner {
                repo,
                attachments,
                store,
                config,
                sink: Box::new(sink),
                state: Mutex::new(State {
                    snapshot,
                    subscriptions: HashMap::new(),
                    open: HashSet::new(),
                    watched: HashSet::new(),
                    paused: false,
                    shut_down: false,
                    store_error,
                    in_flight: HashSet::new(),
                    uploaded: HashSet::new(),
                }),
                live_pumps: AtomicUsize::new(0),
                next_id: AtomicU64::new(0),
                started_at: now_ms(),
                outbox: Arc::default(),
            }),
        };
        client.start_outbox();
        Ok(client)
    }

    /// Starts the outbox thread. It holds only a weak reference, so it ends
    /// when the client is dropped or shut down.
    fn start_outbox(&self) {
        let weak = Arc::downgrade(&self.inner);
        let signal = Arc::clone(&self.inner.outbox);
        let _ = thread::Builder::new()
            .name("forge-chat-outbox".into())
            .spawn(move || Inner::run_outbox(weak, signal));
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

    /// The unsent text saved for a conversation's composer.
    pub fn draft(&self, conversation_id: &str) -> Option<String> {
        self.inner
            .lock()
            .snapshot
            .drafts
            .get(conversation_id)
            .cloned()
    }

    /// Stored messages matching every word of `query` (as prefixes,
    /// ignoring case and accents), newest first: in one conversation, or in
    /// all of them with `None`. Searches what is on this device.
    pub fn search(
        &self,
        query: &str,
        conversation_id: Option<&str>,
    ) -> Result<Vec<Message>, ChatError> {
        self.inner
            .store
            .search(query, conversation_id)
            .map_err(|e| ChatError::UnsupportedOnHost {
                detail: format!("search failed: {e}"),
            })
    }

    /// Messages waiting in the outbox, oldest first.
    pub fn outbox(&self) -> Vec<Message> {
        self.inner.lock().snapshot.outbox()
    }

    /// The `since` cursor for a conversation (see [`ChatSnapshot`]).
    pub fn last_synced(&self, conversation_id: &str) -> Option<i64> {
        self.inner.lock().snapshot.last_synced(conversation_id)
    }

    /// Whether the room is open (`subscribe`).
    pub fn is_subscribed(&self, conversation_id: &str) -> bool {
        self.inner.lock().open.contains(conversation_id)
    }

    /// Whether the room is watched (`watch`).
    pub fn is_watched(&self, conversation_id: &str) -> bool {
        self.inner.lock().watched.contains(conversation_id)
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
        // The server answered: anything queued can go now.
        self.inner.outbox.ring();
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
        self.send_reply(conversation_id, body, None)
    }

    /// [`send_message`](Self::send_message) as an answer to `reply_to`, a
    /// message in the same conversation.
    pub fn send_reply(
        &self,
        conversation_id: &str,
        body: &str,
        reply_to: Option<&str>,
    ) -> Result<Message, ChatError> {
        self.send_with_files(conversation_id, body, reply_to, &[])
    }

    /// [`send_reply`](Self::send_reply) with files (photos, documents).
    /// Each file is copied into this device's attachment store at once, so
    /// the original may go away; the outbox uploads them before the message.
    /// The body may be empty when there are files.
    pub fn send_with_files(
        &self,
        conversation_id: &str,
        body: &str,
        reply_to: Option<&str>,
        files: &[OutgoingFile],
    ) -> Result<Message, ChatError> {
        validate_room(conversation_id)?;
        let body = body.trim();
        if body.is_empty() && files.is_empty() {
            return Err(ChatError::invalid("message body is empty"));
        }
        if files.len() > MAX_ATTACHMENTS_PER_MESSAGE {
            return Err(ChatError::invalid(format!(
                "a message carries at most {MAX_ATTACHMENTS_PER_MESSAGE} files"
            )));
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
            author_id: self.inner.config.user_id.clone(),
            body: body.into(),
            sent_at: now_ms(),
            status: MessageStatus::Sending,
            reply_to: reply_to.map(str::to_string),
            attachments: files
                .iter()
                .map(|file| self.inner.attachments.import(file))
                .collect::<Result<_, _>>()?,
            ..Message::default()
        };
        {
            let mut state = self.inner.lock();
            self.inner.ensure_running_locked(&state)?;
            if let Some(target) = reply_to {
                let found = state
                    .snapshot
                    .message(target)
                    .is_some_and(|m| m.conversation_id == conversation_id && !m.deleted);
                if !found {
                    return Err(ChatError::invalid(format!(
                        "no message {target:?} to reply to in {conversation_id:?}"
                    )));
                }
            }
            // A sent message clears the composer's draft.
            state.snapshot.drafts.remove(conversation_id);
            state.in_flight.insert(message.id.clone());
            state.snapshot.upsert_message(message.clone());
            self.inner.emit(ChatEvent::MessageReceived(message.clone()));
            self.inner
                .refresh_conversation(&mut state, conversation_id, 0, false);
            self.inner.persist(&mut state);
        }
        self.inner.deliver(message)
    }

    /// The local file of an attachment, if it is on this device (ours, or
    /// downloaded before).
    pub fn attachment_file(&self, attachment_id: &str) -> Option<PathBuf> {
        self.inner.attachments.local_file(attachment_id)
    }

    /// The local file of an attachment, downloading it first if needed. The
    /// bytes are checked against the id before they are kept. Blocks for
    /// the download: call it from a worker thread.
    pub fn fetch_attachment(&self, attachment_id: &str) -> Result<PathBuf, ChatError> {
        self.inner.ensure_running()?;
        if !crate::is_attachment_id(attachment_id) {
            return Err(ChatError::invalid(format!(
                "not an attachment id: {attachment_id:?}"
            )));
        }
        if let Some(path) = self.inner.attachments.local_file(attachment_id) {
            return Ok(path);
        }
        let store = &self.inner.attachments;
        std::fs::create_dir_all(store.directory()).map_err(|err| ChatError::UnsupportedOnHost {
            detail: format!("cannot create {}: {err}", store.directory().display()),
        })?;
        let partial = store.download_path(attachment_id);
        self.inner
            .repo
            .download_attachment(attachment_id, &partial)
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&partial);
            })?;
        store.accept_download(attachment_id, &partial)
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
                MessageStatus::Sending if state.in_flight.contains(message_id) => {
                    return Err(ChatError::invalid(format!(
                        "message {message_id:?} is already being sent"
                    )))
                }
                // Queued in the outbox: try it now.
                MessageStatus::Sending => {
                    state.in_flight.insert(message.id.clone());
                    drop(state);
                    return self.inner.deliver(message);
                }
                MessageStatus::Failed => {}
            }
            let message = state
                .snapshot
                .set_status(message_id, MessageStatus::Sending)
                .expect("message exists");
            state.in_flight.insert(message.id.clone());
            self.inner.emit(ChatEvent::MessageStatusChanged {
                conversation_id: message.conversation_id.clone(),
                id: message.id.clone(),
                status: MessageStatus::Sending,
            });
            self.inner
                .refresh_conversation(&mut state, &message.conversation_id, 0, false);
            self.inner.persist(&mut state);
            message
        };
        self.inner.deliver(message)
    }

    /// Marks everything stored in the room as read: locally at once
    /// (`unread_count` → 0, `marked_unread` cleared, `ConversationUpdated`),
    /// then on the server, so other devices and other people see it. An open
    /// room is marked read automatically as messages arrive.
    pub fn mark_read(&self, conversation_id: &str) -> Result<(), ChatError> {
        validate_room(conversation_id)?;
        let up_to = {
            let mut state = self.inner.lock();
            self.inner.ensure_running_locked(&state)?;
            if state.snapshot.conversation(conversation_id).is_none() {
                return Err(ChatError::NotFound {
                    conversation_id: conversation_id.into(),
                });
            }
            let up_to = self.inner.mark_read_locally(&mut state, conversation_id);
            self.inner.persist(&mut state);
            up_to
        };
        match up_to {
            Some(up_to) => self.inner.repo.mark_read(conversation_id, up_to),
            None => Ok(()),
        }
    }

    /// Shows the room as unread (`true`) until it is opened or marked read,
    /// or stops doing so. Kept on this device.
    pub fn set_marked_unread(&self, conversation_id: &str, marked: bool) -> Result<(), ChatError> {
        self.inner
            .update_local(conversation_id, |c| c.marked_unread = marked)
    }

    /// Mutes the room (no `Notify` for it) or unmutes it. Kept on this
    /// device.
    pub fn set_muted(&self, conversation_id: &str, muted: bool) -> Result<(), ChatError> {
        self.inner
            .update_local(conversation_id, |c| c.muted = muted)
    }

    /// Saves the composer's unsent text for a conversation (empty clears it).
    /// Kept on this device, across launches.
    pub fn set_draft(&self, conversation_id: &str, text: &str) -> Result<(), ChatError> {
        validate_room(conversation_id)?;
        let mut state = self.inner.lock();
        self.inner.ensure_running_locked(&state)?;
        let changed = if text.trim().is_empty() {
            state.snapshot.drafts.remove(conversation_id).is_some()
        } else {
            state
                .snapshot
                .drafts
                .insert(conversation_id.into(), text.into())
                != Some(text.into())
        };
        if changed {
            self.inner.persist(&mut state);
        }
        Ok(())
    }

    /// Replaces the text of one of our sent messages, for everyone.
    /// Returns the edited message (also announced as `MessageUpdated`).
    pub fn edit_message(&self, message_id: &str, body: &str) -> Result<Message, ChatError> {
        let body = body.trim();
        if body.is_empty() {
            return Err(ChatError::invalid("message body is empty"));
        }
        if body.chars().count() > MAX_BODY_CHARS {
            return Err(ChatError::invalid(format!(
                "message body is longer than {MAX_BODY_CHARS} characters"
            )));
        }
        let message = self.inner.own_sent_message(message_id)?;
        let edited = self
            .inner
            .repo
            .edit_message(&message.conversation_id, message_id, body)?;
        self.inner.apply_changed(edited)
    }

    /// Deletes one of our messages for everyone. A message still in the
    /// outbox, or that the server refused, is only on this device: it is
    /// removed (`MessageRemoved`) and never sent. Returns the tombstone, or
    /// `None` for a message that was removed.
    pub fn delete_message(&self, message_id: &str) -> Result<Option<Message>, ChatError> {
        {
            let mut state = self.inner.lock();
            self.inner.ensure_running_locked(&state)?;
            let message =
                state.snapshot.message(message_id).cloned().ok_or_else(|| {
                    ChatError::invalid(format!("no message with id {message_id:?}"))
                })?;
            if !self.inner.config.is_own(&message) {
                return Err(ChatError::invalid("only your own messages can be deleted"));
            }
            if message.status != MessageStatus::Sent {
                if state.in_flight.contains(message_id) {
                    return Err(ChatError::invalid(
                        "the message is being sent; try again in a moment",
                    ));
                }
                state.snapshot.remove_message(message_id);
                self.inner.emit(ChatEvent::MessageRemoved {
                    conversation_id: message.conversation_id.clone(),
                    id: message.id.clone(),
                });
                self.inner
                    .refresh_conversation(&mut state, &message.conversation_id, 0, false);
                self.inner.persist(&mut state);
                return Ok(None);
            }
        }
        let message = self.inner.own_sent_message(message_id)?;
        let deleted = self
            .inner
            .repo
            .delete_message(&message.conversation_id, message_id)?;
        self.inner.apply_changed(deleted).map(Some)
    }

    /// Adds (`on`) or removes this account's `emoji` reaction to a sent
    /// message. Returns the message (also announced as `MessageUpdated`).
    pub fn set_reaction(
        &self,
        message_id: &str,
        emoji: &str,
        on: bool,
    ) -> Result<Message, ChatError> {
        let emoji = emoji.trim();
        if emoji.is_empty() || emoji.chars().count() > MAX_REACTION_CHARS {
            return Err(ChatError::invalid("a reaction is one emoji"));
        }
        let message =
            {
                let state = self.inner.lock();
                self.inner.ensure_running_locked(&state)?;
                state.snapshot.message(message_id).cloned().ok_or_else(|| {
                    ChatError::invalid(format!("no message with id {message_id:?}"))
                })?
            };
        if message.status != MessageStatus::Sent || message.deleted {
            return Err(ChatError::invalid("only sent messages can have reactions"));
        }
        let changed = self
            .inner
            .repo
            .react(&message.conversation_id, message_id, emoji, on)?;
        self.inner.apply_changed(changed)
    }

    /// Wakes the outbox: sends whatever is queued now instead of after its
    /// backoff.
    pub fn flush_outbox(&self) {
        self.inner.outbox.ring();
    }

    /// Creates a conversation on the server, adds it to the list
    /// (`ConversationUpdated`) and returns it.
    pub fn create_conversation(&self, title: &str) -> Result<Conversation, ChatError> {
        let title = crate::validate_title(title)?;
        self.inner.ensure_running()?;
        let remote = self.inner.repo.create_conversation(&title)?;
        let mut state = self.inner.lock();
        self.inner.ensure_running_locked(&state)?;
        if state.snapshot.conversation(&remote.id).is_none() {
            state.snapshot.conversations.push(Conversation {
                id: remote.id.clone(),
                title: remote.title.clone(),
                last_message_at: remote.last_message_at,
                last_message: remote.last_message.clone(),
                read_up_to: remote.read_up_to,
                others_read_up_to: remote.others_read_up_to,
                ..Conversation::default()
            });
        }
        self.inner
            .refresh_conversation(&mut state, &remote.id, 0, true);
        self.inner.persist(&mut state);
        Ok(state
            .snapshot
            .conversation(&remote.id)
            .cloned()
            .expect("just added"))
    }

    /// Opens the room: starts its live stream on a pump thread (unless it
    /// is already watched) and marks it read (`unread_count` → 0, here and,
    /// in the background, on the server); it stays read while open. Returns at once; `StreamConnected` /
    /// `StreamDisconnected` report how the connection goes. Idempotent.
    pub fn subscribe(&self, conversation_id: &str) -> Result<(), ChatError> {
        validate_room(conversation_id)?;
        let mut state = self.inner.lock();
        self.inner.ensure_running_locked(&state)?;
        state.open.insert(conversation_id.into());
        if let Some(up_to) = self.inner.mark_read_locally(&mut state, conversation_id) {
            // Tell the server too, off this thread. (A room that is not
            // streaming yet is also marked by its pump after the catch-up.)
            let inner = Arc::clone(&self.inner);
            let room = conversation_id.to_string();
            let _ = thread::Builder::new()
                .name("forge-chat-mark-read".into())
                .spawn(move || {
                    let _ = inner.repo.mark_read(&room, up_to);
                });
        }
        self.inner.persist(&mut state);
        let started = self.start_stream_locked(&mut state, conversation_id);
        if started.is_err() {
            state.open.remove(conversation_id);
        }
        started
    }

    /// Keeps the room's stream live in the background without opening it:
    /// new messages arrive as `MessageReceived` and, when they are from
    /// someone else, count towards `unread_count`. Idempotent.
    pub fn watch(&self, conversation_id: &str) -> Result<(), ChatError> {
        validate_room(conversation_id)?;
        let mut state = self.inner.lock();
        self.inner.ensure_running_locked(&state)?;
        state.watched.insert(conversation_id.into());
        let started = self.start_stream_locked(&mut state, conversation_id);
        if started.is_err() {
            state.watched.remove(conversation_id);
        }
        started
    }

    /// Closes the room. If it is also watched the stream keeps running and
    /// the room counts unread messages again; otherwise the stream ends: on
    /// return `StreamDisconnected { will_retry: false }` has been emitted
    /// and no further event from that stream will be. Idempotent; allowed
    /// after shutdown.
    pub fn unsubscribe(&self, conversation_id: &str) -> Result<(), ChatError> {
        let mut state = self.inner.lock();
        state.open.remove(conversation_id);
        self.inner
            .stop_stream_if_unused_locked(&mut state, conversation_id);
        Ok(())
    }

    /// Stops watching the room; its stream ends unless the room is open.
    /// Idempotent; allowed after shutdown.
    pub fn unwatch(&self, conversation_id: &str) -> Result<(), ChatError> {
        let mut state = self.inner.lock();
        state.watched.remove(conversation_id);
        self.inner
            .stop_stream_if_unused_locked(&mut state, conversation_id);
        Ok(())
    }

    /// Starts the room's pump thread if it has none.
    fn start_stream_locked(
        &self,
        state: &mut State,
        conversation_id: &str,
    ) -> Result<(), ChatError> {
        if state.paused || state.subscriptions.contains_key(conversation_id) {
            return Ok(());
        }
        let subscription = Arc::new(Subscription {
            conversation_id: conversation_id.into(),
            cancel: CancelHandle::new(),
        });
        state
            .subscriptions
            .insert(conversation_id.into(), Arc::clone(&subscription));
        self.inner.live_pumps.fetch_add(1, Ordering::SeqCst);
        let inner = Arc::clone(&self.inner);
        // The pump takes the lock before its first change, so starting it
        // while we hold the lock only delays it.
        let spawned = thread::Builder::new()
            .name(format!("forge-chat-stream-{conversation_id}"))
            .spawn(move || inner.pump(subscription));
        if let Err(err) = spawned {
            self.inner.live_pumps.fetch_sub(1, Ordering::SeqCst);
            state.subscriptions.remove(conversation_id);
            return Err(ChatError::UnsupportedOnHost {
                detail: format!("could not start the stream thread: {err}"),
            });
        }
        Ok(())
    }

    /// Stops every live stream without forgetting which rooms are open or
    /// watched, as an app does when it goes to the background (iOS
    /// suspends it within seconds, and a suspended app's sockets die
    /// anyway). Each stream ends with `StreamDisconnected { will_retry:
    /// true }`, and `subscribe`/`watch` while paused only record the room.
    /// Sends in flight are not touched. Idempotent.
    pub fn pause_streams(&self) {
        let mut state = self.inner.lock();
        if state.shut_down || state.paused {
            return;
        }
        state.paused = true;
        let mut ended: Vec<_> = state.subscriptions.drain().collect();
        ended.sort_by(|a, b| a.0.cmp(&b.0));
        for (conversation_id, subscription) in ended {
            subscription.cancel.cancel();
            self.inner.emit(ChatEvent::StreamDisconnected {
                conversation_id,
                reason: "paused".into(),
                will_retry: true,
            });
        }
    }

    /// Restarts the streams of every open or watched room after
    /// [`pause_streams`](Self::pause_streams). Each catches up from its
    /// cursor, so nothing sent meanwhile is missed. Idempotent.
    pub fn resume_streams(&self) -> Result<(), ChatError> {
        let mut state = self.inner.lock();
        self.inner.ensure_running_locked(&state)?;
        if !state.paused {
            return Ok(());
        }
        state.paused = false;
        let mut rooms: Vec<String> = state.open.union(&state.watched).cloned().collect();
        rooms.sort();
        for room in rooms {
            self.start_stream_locked(&mut state, &room)?;
        }
        self.inner.outbox.ring();
        Ok(())
    }

    /// Whether streams are paused (`pause_streams`).
    pub fn is_paused(&self) -> bool {
        self.inner.lock().paused
    }

    /// Ends every subscription and refuses further commands with
    /// [`ChatError::ShutDown`]. Idempotent.
    pub fn shutdown(&self) {
        let mut state = self.inner.lock();
        if state.shut_down {
            return;
        }
        state.shut_down = true;
        self.inner.outbox.ring();
        state.open.clear();
        state.watched.clear();
        let mut ended: Vec<_> = state.subscriptions.drain().collect();
        ended.sort_by(|a, b| a.0.cmp(&b.0));
        for (conversation_id, subscription) in ended {
            subscription.cancel.cancel();
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

    /// Ends the room's stream once it is neither open nor watched.
    fn stop_stream_if_unused_locked(&self, state: &mut State, conversation_id: &str) {
        if state.open.contains(conversation_id) || state.watched.contains(conversation_id) {
            return;
        }
        if let Some(subscription) = state.subscriptions.remove(conversation_id) {
            subscription.cancel.cancel();
            self.emit(ChatEvent::StreamDisconnected {
                conversation_id: conversation_id.into(),
                reason: "unsubscribed".into(),
                will_retry: false,
            });
        }
    }

    fn persist(&self, state: &mut State) {
        if let Err(err) = self.store.save(&state.snapshot) {
            state.store_error = Some(err);
        }
    }

    /// One of our messages the server has, for an edit or a delete.
    fn own_sent_message(&self, message_id: &str) -> Result<Message, ChatError> {
        let state = self.lock();
        self.ensure_running_locked(&state)?;
        let message = state
            .snapshot
            .message(message_id)
            .cloned()
            .ok_or_else(|| ChatError::invalid(format!("no message with id {message_id:?}")))?;
        if !self.config.is_own(&message) {
            return Err(ChatError::invalid("only your own messages can be changed"));
        }
        if message.status != MessageStatus::Sent {
            return Err(ChatError::invalid("the message has not been sent yet"));
        }
        if message.deleted {
            return Err(ChatError::invalid("the message was deleted"));
        }
        Ok(message)
    }

    /// Stores a message the server just changed for us, announces it and
    /// refreshes its conversation's preview.
    fn apply_changed(&self, mut message: Message) -> Result<Message, ChatError> {
        message.status = MessageStatus::Sent;
        let mut state = self.lock();
        self.ensure_running_locked(&state)?;
        if state.snapshot.upsert_message(message.clone()) != Upsert::Unchanged {
            self.emit(ChatEvent::MessageUpdated(message.clone()));
            self.refresh_conversation(&mut state, &message.conversation_id, 0, false);
            self.persist(&mut state);
        }
        Ok(message)
    }

    /// The outbox thread: sends queued messages, oldest first, then waits
    /// for the doorbell (or the backoff after a failure). Exits when the
    /// client is shut down or dropped.
    fn run_outbox(weak: Weak<Inner>, signal: Arc<OutboxSignal>) {
        let mut backoff = Backoff::new(OUTBOX_INITIAL_DELAY, OUTBOX_MAX_DELAY);
        loop {
            let Some(inner) = weak.upgrade() else { return };
            let queued = {
                let mut state = inner.lock();
                if state.shut_down {
                    return;
                }
                if state.paused {
                    Vec::new()
                } else {
                    let queued: Vec<Message> = state
                        .snapshot
                        .outbox()
                        .into_iter()
                        .filter(|m| !state.in_flight.contains(&m.id))
                        .collect();
                    for message in &queued {
                        state.in_flight.insert(message.id.clone());
                    }
                    queued
                }
            };
            let mut blocked = false;
            let mut queued = queued.into_iter();
            for message in queued.by_ref() {
                let id = message.id.clone();
                let sent = inner.deliver(message);
                let still_queued = inner
                    .lock()
                    .snapshot
                    .message(&id)
                    .is_some_and(|m| m.status == MessageStatus::Sending);
                if sent.is_ok() && still_queued {
                    // Unreachable: keep the order, try the rest later.
                    blocked = true;
                    break;
                }
            }
            {
                let mut state = inner.lock();
                for message in queued {
                    state.in_flight.remove(&message.id);
                }
            }
            drop(inner);
            let wait = if blocked {
                backoff.next_delay()
            } else {
                backoff.reset();
                OUTBOX_IDLE
            };
            signal.wait(wait);
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

    /// Replaces the conversation list with the server's, keeping what this
    /// device knows (unread counts without server cursors, mute, marked
    /// unread, cursors that moved further). Rooms the server no longer lists
    /// are forgotten.
    fn replace_conversations(&self, state: &mut State, remote: &[RemoteConversation]) {
        let conversations: Vec<Conversation> = remote
            .iter()
            .map(|room| {
                let existing = state.snapshot.conversation(&room.id);
                let mut conversation = Conversation {
                    id: room.id.clone(),
                    title: room.title.clone(),
                    last_message_at: max_opt(
                        room.last_message_at,
                        existing.and_then(|c| c.last_message_at),
                    ),
                    unread_count: existing.map_or(0, |c| c.unread_count),
                    last_message: newest_of(
                        room.last_message.clone(),
                        existing.and_then(|c| c.last_message.clone()),
                    ),
                    read_up_to: max_opt(room.read_up_to, existing.and_then(|c| c.read_up_to)),
                    others_read_up_to: max_opt(
                        room.others_read_up_to,
                        existing.and_then(|c| c.others_read_up_to),
                    ),
                    muted: existing.is_some_and(|c| c.muted),
                    marked_unread: existing.is_some_and(|c| c.marked_unread),
                };
                self.derive(state, &mut conversation, 0);
                conversation
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
        let viewing = state.open.contains(conversation_id);
        let contiguous = match fetched_since {
            None | Some(None) => true,
            Some(Some(since)) => state
                .snapshot
                .last_synced(conversation_id)
                .is_some_and(|cursor| since <= cursor),
        };
        let (read_up_to, muted) = state
            .snapshot
            .conversation(conversation_id)
            .map_or((None, false), |c| (c.read_up_to, c.muted));
        let mut newest = None;
        let mut unread_added = 0u32;
        for mut message in messages {
            message.status = MessageStatus::Sent;
            newest = newest.max(Some(message.changed_at()));
            match state.snapshot.upsert_message(message.clone()) {
                Upsert::Inserted => {
                    let unread = !viewing
                        && !self.config.is_own(&message)
                        && read_up_to.is_none_or(|read| message.sent_at > read);
                    let notify = unread && !muted && message.sent_at >= self.started_at;
                    if unread {
                        unread_added += 1;
                    }
                    self.emit(ChatEvent::MessageReceived(message.clone()));
                    if notify {
                        self.emit(ChatEvent::Notify(message));
                    }
                }
                Upsert::StatusChanged => self.emit(ChatEvent::MessageStatusChanged {
                    conversation_id: message.conversation_id,
                    id: message.id,
                    status: MessageStatus::Sent,
                }),
                // Same id, same status, different content: edited, deleted
                // or reacted to (or the server time replaced ours).
                Upsert::Updated => self.emit(ChatEvent::MessageUpdated(message)),
                Upsert::Unchanged => {}
            }
        }
        if let (true, Some(newest)) = (contiguous, newest) {
            state.snapshot.advance_cursor(conversation_id, newest);
        }
        self.refresh_conversation(state, conversation_id, unread_added, false);
    }

    /// Recomputes what a conversation derives from the stored messages and
    /// its cursors: the preview, `last_message_at` and `unread_count`.
    /// `unread_added` only counts without a server read cursor (then the
    /// count is kept incrementally on this device).
    fn derive(&self, state: &State, conversation: &mut Conversation, unread_added: u32) {
        let messages = state.snapshot.messages(&conversation.id);
        if let Some(newest) = messages.last() {
            conversation.last_message = newest_of(
                Some(newest.clone()),
                conversation
                    .last_message
                    .take()
                    // A stored message may have changed status: prefer it.
                    .filter(|m| !messages.iter().any(|s| s.id == m.id)),
            );
        }
        let newest_sent = messages
            .iter()
            .rev()
            .find(|m| m.status == MessageStatus::Sent)
            .map(|m| m.sent_at);
        conversation.last_message_at = max_opt(conversation.last_message_at, newest_sent);
        conversation.unread_count = if state.open.contains(&conversation.id) {
            0
        } else if let Some(read) = conversation.read_up_to {
            let unread = messages
                .iter()
                .filter(|m| {
                    m.status == MessageStatus::Sent && m.sent_at > read && !self.config.is_own(m)
                })
                .count();
            u32::try_from(unread).unwrap_or(u32::MAX)
        } else {
            conversation.unread_count.saturating_add(unread_added)
        };
    }

    /// [`derive`](Self::derive)s the conversation and emits
    /// `ConversationUpdated` if anything changed (or always, with `announce`).
    fn refresh_conversation(
        &self,
        state: &mut State,
        conversation_id: &str,
        unread_added: u32,
        announce: bool,
    ) {
        let Some(before) = state.snapshot.conversation(conversation_id).cloned() else {
            return;
        };
        let mut after = before.clone();
        self.derive(state, &mut after, unread_added);
        let changed = after != before;
        if changed {
            *state
                .snapshot
                .conversation_mut(conversation_id)
                .expect("checked above") = after.clone();
        }
        if changed || announce {
            self.emit(ChatEvent::ConversationUpdated(after));
        }
    }

    /// Changes a device-only field of a conversation and announces it.
    fn update_local(
        &self,
        conversation_id: &str,
        change: impl FnOnce(&mut Conversation),
    ) -> Result<(), ChatError> {
        validate_room(conversation_id)?;
        let mut state = self.lock();
        self.ensure_running_locked(&state)?;
        let conversation = state
            .snapshot
            .conversation_mut(conversation_id)
            .ok_or_else(|| ChatError::NotFound {
                conversation_id: conversation_id.into(),
            })?;
        let before = conversation.clone();
        change(conversation);
        if *conversation != before {
            let conversation = conversation.clone();
            self.persist(&mut state);
            self.emit(ChatEvent::ConversationUpdated(conversation));
        }
        Ok(())
    }

    /// Marks the room read on this device up to its newest stored message.
    /// Returns the cursor to send to the server, if it moved.
    fn mark_read_locally(&self, state: &mut State, conversation_id: &str) -> Option<i64> {
        let newest = state
            .snapshot
            .messages(conversation_id)
            .iter()
            .rev()
            .find(|m| m.status == MessageStatus::Sent)
            .map(|m| m.sent_at);
        let conversation = state.snapshot.conversation_mut(conversation_id)?;
        let before = conversation.clone();
        let moved = newest.filter(|&newest| conversation.read_up_to.is_none_or(|r| newest > r));
        if let Some(newest) = moved {
            conversation.read_up_to = Some(newest);
        }
        conversation.marked_unread = false;
        conversation.unread_count = 0;
        if *conversation != before {
            let conversation = conversation.clone();
            self.emit(ChatEvent::ConversationUpdated(conversation));
        }
        moved
    }

    /// Applies a read cursor move from the room's stream.
    fn apply_read(&self, state: &mut State, conversation_id: &str, user_id: &str, up_to: i64) {
        let mine = user_id == self.config.user_id;
        let Some(conversation) = state.snapshot.conversation_mut(conversation_id) else {
            return;
        };
        if mine {
            if conversation.read_up_to.is_some_and(|r| r >= up_to) {
                return;
            }
            conversation.read_up_to = Some(up_to);
            // Read on another device: the request to show it unread is
            // answered.
            conversation.marked_unread = false;
        } else {
            if conversation.others_read_up_to.is_some_and(|r| r >= up_to) {
                return;
            }
            conversation.others_read_up_to = Some(up_to);
        }
        self.refresh_conversation(state, conversation_id, 0, true);
    }

    /// Applies messages from the room's pump and persists them. While the
    /// room is open, what arrived is marked read in the same step (so it
    /// can never count as unread after the room closes) and then on the
    /// server. `None` means the pump must stop.
    fn apply_from_pump(
        &self,
        subscription: &Arc<Subscription>,
        messages: Vec<Message>,
        fetched_since: Option<Option<i64>>,
    ) -> Option<()> {
        let room = &subscription.conversation_id;
        let up_to = self.with_current(subscription, |inner, state| {
            inner.apply_server_messages(state, room, messages, fetched_since);
            let up_to = if state.open.contains(room) {
                inner.mark_read_locally(state, room)
            } else {
                None
            };
            inner.persist(state);
            up_to
        })?;
        if let Some(up_to) = up_to {
            // Best effort: the next mark (or opening the room again) retries.
            let _ = self.repo.mark_read(room, up_to);
        }
        Some(())
    }

    /// Posts a message that is already stored as `Sending`, then records the
    /// outcome.
    fn deliver(&self, message: Message) -> Result<Message, ChatError> {
        let result = self.upload_attachments(&message).and_then(|()| {
            self.repo.post_message(&OutgoingMessage {
                id: message.id.clone(),
                conversation_id: message.conversation_id.clone(),
                author: message.author.clone(),
                author_id: message.author_id.clone(),
                body: message.body.clone(),
                reply_to: message.reply_to.clone(),
                attachments: message.attachments.clone(),
            })
        });
        let mut state = self.lock();
        state.in_flight.remove(&message.id);
        let shut_down = state.shut_down;
        match result {
            Ok(mut stored) => {
                stored.status = MessageStatus::Sent;
                let outcome = state.snapshot.upsert_message(stored.clone());
                if !shut_down {
                    match outcome {
                        Upsert::StatusChanged => self.emit(ChatEvent::MessageStatusChanged {
                            conversation_id: stored.conversation_id.clone(),
                            id: stored.id.clone(),
                            status: MessageStatus::Sent,
                        }),
                        Upsert::Updated => self.emit(ChatEvent::MessageUpdated(stored.clone())),
                        Upsert::Inserted => self.emit(ChatEvent::MessageReceived(stored.clone())),
                        Upsert::Unchanged => {}
                    }
                    // Our own send does not move the sync cursor: messages
                    // from others may sit between the cursor and this one.
                    // It does move the read cursor, as the server does.
                    if let Some(conversation) =
                        state.snapshot.conversation_mut(&stored.conversation_id)
                    {
                        if let Some(read) = conversation.read_up_to.as_mut() {
                            *read = (*read).max(stored.sent_at);
                        }
                    }
                    self.refresh_conversation(&mut state, &stored.conversation_id, 0, false);
                }
                self.persist(&mut state);
                Ok(stored)
            }
            Err(err) => {
                // The stream echo may have confirmed it while the response
                // was lost; then it was sent after all.
                let Some(current) = state.snapshot.message(&message.id) else {
                    // Deleted while it was being sent.
                    return Err(err);
                };
                if current.status == MessageStatus::Sent {
                    return Ok(current.clone());
                }
                if err.is_transient() {
                    // The outbox keeps it and tries again later.
                    let queued = current.clone();
                    self.persist(&mut state);
                    return Ok(queued);
                }
                state
                    .snapshot
                    .set_status(&message.id, MessageStatus::Failed);
                if shut_down {
                    self.persist(&mut state);
                    return Err(ChatError::Cancelled);
                }
                self.emit(ChatEvent::MessageStatusChanged {
                    conversation_id: message.conversation_id.clone(),
                    id: message.id,
                    status: MessageStatus::Failed,
                });
                self.refresh_conversation(&mut state, &message.conversation_id, 0, false);
                self.persist(&mut state);
                Err(err)
            }
        }
    }

    /// Uploads a message's attachments that this client has not uploaded
    /// yet, checking the server stored each under the id we computed.
    fn upload_attachments(&self, message: &Message) -> Result<(), ChatError> {
        for attachment in &message.attachments {
            if self.lock().uploaded.contains(&attachment.id) {
                continue;
            }
            let path = self.attachments.local_file(&attachment.id).ok_or_else(|| {
                ChatError::invalid(format!("{} is no longer on this device", attachment.name))
            })?;
            let stored = self.repo.upload_attachment(&path, &attachment.mime)?;
            if stored != attachment.id {
                return Err(ChatError::protocol(format!(
                    "uploaded {} as {stored}, expected {}",
                    attachment.name, attachment.id
                )));
            }
            self.lock().uploaded.insert(stored);
        }
        Ok(())
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
            state.open.remove(&subscription.conversation_id);
            state.watched.remove(&subscription.conversation_id);
            subscription.cancel.cancel();
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

    /// The pump: connect, catch up, follow the stream; on failure back off
    /// and reconnect, until the subscription ends.
    fn pump(self: Arc<Self>, subscription: Arc<Subscription>) {
        let _guard = PumpGuard(&self.live_pumps);
        let room = subscription.conversation_id.clone();
        // Jitter keeps every client from reconnecting at once after a server restart.
        let mut backoff =
            Backoff::new(self.config.reconnect_initial, self.config.reconnect_max).with_jitter(0.2);
        // One `StreamDisconnected` per outage, not one per failed attempt.
        let mut outage_reported = false;

        while !subscription.is_cancelled() {
            match self.repo.open_stream(&room, &subscription.cancel) {
                Ok(mut stream) => {
                    let connected = self.with_current(&subscription, |inner, _| {
                        inner.emit(ChatEvent::StreamConnected {
                            conversation_id: room.clone(),
                        })
                    });
                    // The server is back: send what is queued.
                    self.outbox.ring();
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
                            if self
                                .apply_from_pump(&subscription, messages, Some(since))
                                .is_none()
                            {
                                return;
                            }
                            backoff.reset();
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
            // Sleeps, waking as soon as the subscription ends.
            if !backoff.wait(&subscription.cancel) {
                return;
            }
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
                    self.apply_from_pump(subscription, vec![message], None)?;
                }
                RemoteEvent::Conversation(remote) => {
                    if remote.id != *room {
                        continue;
                    }
                    self.with_current(subscription, |inner, state| {
                        if let Some(conversation) = state.snapshot.conversation_mut(&remote.id) {
                            conversation.title = remote.title.clone();
                            conversation.last_message_at =
                                max_opt(conversation.last_message_at, remote.last_message_at);
                            conversation.last_message = newest_of(
                                conversation.last_message.take(),
                                remote.last_message.clone(),
                            );
                            inner.refresh_conversation(state, &remote.id, 0, false);
                            inner.persist(state);
                        }
                    })?;
                }
                RemoteEvent::Read { user_id, up_to } => {
                    self.with_current(subscription, |inner, state| {
                        inner.apply_read(state, room, &user_id, up_to);
                        inner.persist(state);
                    })?;
                }
            }
        }
    }
}

fn max_opt(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    a.max(b)
}

/// The later of two messages (by `sent_at`, then id); of two copies of one
/// message, the one changed last.
fn newest_of(a: Option<Message>, b: Option<Message>) -> Option<Message> {
    match (a, b) {
        (Some(a), Some(b)) => Some(
            if (b.sent_at, &b.id, b.changed_at()) > (a.sent_at, &a.id, a.changed_at()) {
                b
            } else {
                a
            },
        ),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests;
