//! `ForgeChatApp`: the Chat example's FFI surface over `chat_core::ChatClient`.
//!
//! Concurrency contract (compare `crate::weather::app`):
//!
//! * Commands that touch the network are `async`. Each runs the blocking
//!   core command on its own short-lived worker thread and resolves when it
//!   returns, so hosts never block and no async runtime is needed.
//! * They are not cancellable from the host, on purpose: leaving a room must
//!   not cancel a send already on the wire (see `docs/chat-demo.md`). A host
//!   that drops the future just ignores the answer; the store and the event
//!   stream still record the outcome. `shutdown()` is the one way to abandon
//!   them, and a send that then fails reports `Cancelled`.
//! * `subscribe` / `unsubscribe` (a room's live connection) are explicit and
//!   synchronous: both return at once. After `unsubscribe` returns no more
//!   events from that room's stream are queued, and `live_streams()` shows
//!   when its Rust thread has ended.
//! * `events()` hands out pull-based subscriptions, one per consumer, each
//!   with its own bounded queue (`CHAT_QUEUE_CAPACITY`, then
//!   `ChatEvent::EventsDropped`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use chat_core::{ChatClient, ChatConfig, ChatRepo, ChatStore, HttpRepo};
use forge_net::Request;

use super::events::{ChatEventHub, ChatEventSubscription};
use super::types::{ChatAppConfig, ChatConversation, ChatError, ChatMessage};
use crate::signal::Completion;

/// The Chat example's app instance. Thread-safe; share it freely.
#[derive(uniffi::Object)]
pub struct ForgeChatApp {
    client: ChatClient,
    hub: Arc<ChatEventHub>,
    shut_down: AtomicBool,
}

#[uniffi::export]
impl ForgeChatApp {
    /// Opens the app against the chat server in `config.base_url`.
    /// `storage_directory` holds the chat store and is created if missing;
    /// an empty string keeps everything in memory.
    ///
    /// Fails with `InvalidInput` for a bad URL or timeout, with
    /// `UnsupportedOnHost` for an `https` URL on Android (see
    /// [`check_host_supports`]), and with `Storage` when the stored data was
    /// written by a newer build (it is left alone).
    /// A corrupt store does not fail: the app starts empty and
    /// `store_problem()` says why.
    #[uniffi::constructor]
    pub fn new(storage_directory: String, config: ChatAppConfig) -> Result<Arc<Self>, ChatError> {
        Request::get(&config.base_url)
            .validate()
            .map_err(|_| ChatError::InvalidInput {
                reason: format!("base URL must be http(s): {:?}", config.base_url),
            })?;
        check_host_supports(&config.base_url, cfg!(target_os = "android"))?;
        let timeout = match config.request_timeout_ms {
            Some(0) => {
                return Err(ChatError::InvalidInput {
                    reason: "request timeout must be > 0".into(),
                })
            }
            Some(ms) => Duration::from_millis(u64::from(ms)),
            None => chat_core::DEFAULT_TIMEOUT,
        };
        let store = if storage_directory.is_empty() {
            ChatStore::in_memory()
        } else {
            let directory = PathBuf::from(&storage_directory);
            std::fs::create_dir_all(&directory).map_err(|err| ChatError::Storage {
                reason: format!("create {storage_directory:?}: {err}"),
            })?;
            ChatStore::in_directory(directory)
        };
        let repo = HttpRepo::with_ureq(config.base_url).timeout(timeout);
        let core_config = ChatConfig {
            user: config.user.unwrap_or_else(|| ChatConfig::default().user),
            ..ChatConfig::default()
        };
        Self::build(Arc::new(repo), store, core_config).map(Arc::new)
    }

    /// A new event stream. Every consumer (the list screen, the chat
    /// screen) takes its own; each sees every event from now on.
    pub fn events(&self) -> Result<Arc<ChatEventSubscription>, ChatError> {
        self.hub.subscribe().map(Arc::new)
    }

    /// Lists rooms from the server, prefetching new messages for rooms
    /// that are not open (they count as unread).
    pub async fn list_conversations(&self) -> Result<Vec<ChatConversation>, ChatError> {
        let client = self.running_client()?;
        run("list", move || {
            client
                .list_conversations()
                .map(|list| list.into_iter().map(Into::into).collect())
        })
        .await
    }

    /// Fetches messages (all, or newer than `since`, Unix ms), merges them
    /// into the store and returns the room's full stored list, oldest first.
    pub async fn load_messages(
        &self,
        conversation_id: String,
        since: Option<i64>,
    ) -> Result<Vec<ChatMessage>, ChatError> {
        let client = self.running_client()?;
        run("load", move || {
            client
                .load_messages(&conversation_id, since)
                .map(|list| list.into_iter().map(Into::into).collect())
        })
        .await
    }

    /// `load_messages(id, last_synced(id))`: what to call when a chat opens.
    pub async fn sync_messages(
        &self,
        conversation_id: String,
    ) -> Result<Vec<ChatMessage>, ChatError> {
        let client = self.running_client()?;
        run("sync", move || {
            client
                .sync_messages(&conversation_id)
                .map(|list| list.into_iter().map(Into::into).collect())
        })
        .await
    }

    /// Sends a message. It is announced as `Sending` at once, then becomes
    /// `Sent` or `Failed` (`MessageStatusChanged`). Resolves with the final
    /// message, or the error that failed it.
    pub async fn send_message(
        &self,
        conversation_id: String,
        body: String,
    ) -> Result<ChatMessage, ChatError> {
        let client = self.running_client()?;
        run("send", move || {
            client.send_message(&conversation_id, &body).map(Into::into)
        })
        .await
    }

    /// Sends a `Failed` message again with the same id.
    pub async fn retry_message(&self, message_id: String) -> Result<ChatMessage, ChatError> {
        let client = self.running_client()?;
        run("retry", move || {
            client.retry_message(&message_id).map(Into::into)
        })
        .await
    }

    /// Opens the room's live connection (or reuses its watch) and marks it
    /// read; it stays read while open. Returns at once;
    /// `StreamConnected` / `StreamDisconnected` events follow. Idempotent.
    pub fn subscribe(&self, conversation_id: String) -> Result<(), ChatError> {
        self.client.subscribe(&conversation_id).map_err(Into::into)
    }

    /// Closes the room. If it is not watched, its live connection ends: on
    /// return `StreamDisconnected { will_retry: false }` is queued and
    /// nothing more from that room's stream will be. A watched room stays
    /// live and counts unread messages again. Idempotent.
    pub fn unsubscribe(&self, conversation_id: String) -> Result<(), ChatError> {
        self.client
            .unsubscribe(&conversation_id)
            .map_err(Into::into)
    }

    /// Keeps the room live in the background without opening it: new
    /// messages arrive as `MessageReceived`, and new ones from others count
    /// towards `unread_count` and raise `Notify`. Returns at once.
    /// Idempotent.
    pub fn watch(&self, conversation_id: String) -> Result<(), ChatError> {
        self.client.watch(&conversation_id).map_err(Into::into)
    }

    /// Stops watching the room. Its stream ends unless the room is also
    /// open (`subscribe`). Idempotent.
    pub fn unwatch(&self, conversation_id: String) -> Result<(), ChatError> {
        self.client.unwatch(&conversation_id).map_err(Into::into)
    }

    pub fn is_watched(&self, conversation_id: String) -> bool {
        self.client.is_watched(&conversation_id)
    }

    /// Conversations as stored, for a first render before the network.
    pub fn cached_conversations(&self) -> Vec<ChatConversation> {
        self.client
            .cached_conversations()
            .into_iter()
            .map(Into::into)
            .collect()
    }

    /// A room's messages as stored, oldest first.
    pub fn cached_messages(&self, conversation_id: String) -> Vec<ChatMessage> {
        self.client
            .cached_messages(&conversation_id)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    /// The room's `since` cursor (server time, Unix ms).
    pub fn last_synced(&self, conversation_id: String) -> Option<i64> {
        self.client.last_synced(&conversation_id)
    }

    pub fn is_subscribed(&self, conversation_id: String) -> bool {
        self.client.is_subscribed(&conversation_id)
    }

    /// Room connection threads still running. Reaches 0 shortly after every
    /// room is unsubscribed: the proof that leaving a room stops Rust work.
    pub fn live_streams(&self) -> u32 {
        u32::try_from(self.client.live_pumps()).unwrap_or(u32::MAX)
    }

    /// The last persistence problem, if any. Never fails a command.
    pub fn store_problem(&self) -> Option<String> {
        self.client.store_error().map(|e| e.to_string())
    }

    /// Closes every room and every event stream (queued events are still
    /// delivered first). Later commands fail with `ShutDown`. Idempotent.
    pub fn shutdown(&self) {
        self.shut_down.store(true, Ordering::SeqCst);
        self.client.shutdown();
        self.hub.close();
    }

    pub fn is_shut_down(&self) -> bool {
        self.shut_down.load(Ordering::SeqCst)
    }
}

impl ForgeChatApp {
    pub(crate) fn build(
        repo: Arc<dyn ChatRepo>,
        store: ChatStore,
        config: ChatConfig,
    ) -> Result<Self, ChatError> {
        let hub = ChatEventHub::new();
        let sink_hub = Arc::clone(&hub);
        let client = ChatClient::open(repo, store, config, move |event: chat_core::ChatEvent| {
            sink_hub.publish(event.into())
        })?;
        Ok(Self {
            client,
            hub,
            shut_down: AtomicBool::new(false),
        })
    }

    fn running_client(&self) -> Result<ChatClient, ChatError> {
        if self.is_shut_down() {
            return Err(ChatError::ShutDown);
        }
        Ok(self.client.clone())
    }
}

/// HTTPS on Android is refused up front, as a typed error.
///
/// `UreqClient` checks certificates with `rustls-platform-verifier`, which on
/// Android only works after a JNI init with the app `Context`. No host does
/// that init yet, so an `https` server would fail every request with a TLS
/// error that looks like "server down". Plain `http` (the mock server) never
/// touches the verifier. See "Android and TLS" in `docs/chat-demo.md`.
pub(crate) fn check_host_supports(base_url: &str, android: bool) -> Result<(), ChatError> {
    let https = base_url
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"));
    if android && https {
        return Err(ChatError::UnsupportedOnHost {
            detail: "https on Android needs the platform certificate verifier, which this \
                     build does not initialise; use an http:// server (the mock server)"
                .into(),
        });
    }
    Ok(())
}

/// Runs a blocking core command on a worker thread and awaits its result.
async fn run<T, F>(name: &str, command: F) -> Result<T, ChatError>
where
    T: Clone + Send + 'static,
    F: FnOnce() -> Result<T, chat_core::ChatError> + Send + 'static,
{
    let completion: Arc<Completion<Result<T, ChatError>>> = Completion::new();
    let worker = Arc::clone(&completion);
    thread::Builder::new()
        .name(format!("forge-chat-{name}"))
        .spawn(move || {
            worker.complete(command().map_err(Into::into));
        })
        .map_err(|err| ChatError::Internal {
            reason: format!("could not start {name} thread: {err}"),
        })?;
    completion.wait().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::events::CHAT_QUEUE_CAPACITY;
    use crate::chat::types::{ChatEvent, ChatMessageStatus};
    use crate::signal::block_on;
    use chat_core::InMemoryRepo;
    use std::time::Instant;

    fn app() -> (Arc<InMemoryRepo>, ForgeChatApp) {
        let repo = Arc::new(InMemoryRepo::with_rooms(&[("general", "General")]));
        let config = ChatConfig {
            reconnect_initial: Duration::from_millis(10),
            reconnect_max: Duration::from_millis(40),
            ..ChatConfig::default()
        };
        let app = ForgeChatApp::build(repo.clone(), ChatStore::in_memory(), config).unwrap();
        (repo, app)
    }

    /// Pulls events until `pred` matches (or 5 s pass).
    fn next_matching(
        sub: &ChatEventSubscription,
        pred: impl Fn(&ChatEvent) -> bool,
    ) -> Vec<ChatEvent> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            let event = block_on(sub.next()).expect("stream ended early");
            let done = pred(&event);
            seen.push(event);
            if done {
                return seen;
            }
        }
        panic!("no matching event; saw {seen:#?}");
    }

    #[test]
    fn constructor_validates_input() {
        let config = |base: &str, timeout| ChatAppConfig {
            base_url: base.into(),
            user: None,
            request_timeout_ms: timeout,
        };
        assert!(matches!(
            ForgeChatApp::new(String::new(), config("localhost:8787", None)),
            Err(ChatError::InvalidInput { .. })
        ));
        assert!(matches!(
            ForgeChatApp::new(String::new(), config("http://127.0.0.1:8787", Some(0))),
            Err(ChatError::InvalidInput { .. })
        ));
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b").to_string_lossy().into_owned();
        let app =
            ForgeChatApp::new(nested.clone(), config("http://127.0.0.1:8787/", Some(500))).unwrap();
        assert!(std::path::Path::new(&nested).is_dir());
        app.shutdown();
    }

    #[test]
    fn https_on_android_is_unsupported_not_a_tls_failure() {
        for url in ["https://chat.example", "HTTPS://chat.example"] {
            assert!(matches!(
                check_host_supports(url, true),
                Err(ChatError::UnsupportedOnHost { .. })
            ));
            assert_eq!(check_host_supports(url, false), Ok(()));
        }
        assert_eq!(check_host_supports("http://10.0.2.2:8787", true), Ok(()));
    }

    #[test]
    fn newer_store_fails_with_storage_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(chat_core::store::STORE_FILENAME),
            br#"{"schema_version": 42}"#,
        )
        .unwrap();
        let result = ForgeChatApp::new(
            dir.path().to_string_lossy().into_owned(),
            ChatAppConfig {
                base_url: "http://127.0.0.1:8787".into(),
                user: None,
                request_timeout_ms: None,
            },
        );
        assert!(matches!(result, Err(ChatError::Storage { .. })));
    }

    #[test]
    fn unreachable_server_is_a_network_error() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let app = ForgeChatApp::new(
            String::new(),
            ChatAppConfig {
                base_url: format!("http://127.0.0.1:{port}"),
                user: Some("ana".into()),
                request_timeout_ms: Some(2000),
            },
        )
        .unwrap();
        assert!(matches!(
            block_on(app.list_conversations()),
            Err(ChatError::Network { .. })
        ));
        let err = block_on(app.send_message("general".into(), "hi".into())).unwrap_err();
        assert!(matches!(err, ChatError::Network { .. }), "{err:?}");
        let failed = app.cached_messages("general".into());
        assert_eq!(failed[0].author, "ana");
        assert_eq!(failed[0].status, ChatMessageStatus::Failed);
    }

    #[test]
    fn commands_and_events_flow_through_the_ffi_types() {
        let (repo, app) = app();
        let events = app.events().unwrap();
        let rooms = block_on(app.list_conversations()).unwrap();
        assert_eq!(rooms[0].id, "general");

        app.subscribe("general".into()).unwrap();
        next_matching(&events, |e| matches!(e, ChatEvent::StreamConnected { .. }));

        let sent = block_on(app.send_message("general".into(), "hello".into())).unwrap();
        assert_eq!(sent.status, ChatMessageStatus::Sent);
        let seen = next_matching(&events, |e| {
            *e == ChatEvent::MessageStatusChanged {
                conversation_id: "general".into(),
                id: sent.id.clone(),
                status: ChatMessageStatus::Sent,
            }
        });
        assert!(seen.iter().any(|e| matches!(
            e,
            ChatEvent::MessageReceived { message } if message.id == sent.id && message.status == ChatMessageStatus::Sending
        )));

        let incoming = repo.inject("general", "curl", "from outside");
        next_matching(
            &events,
            |e| matches!(e, ChatEvent::MessageReceived { message } if message.id == incoming.id),
        );
        assert_eq!(app.last_synced("general".into()), Some(incoming.sent_at));

        app.unsubscribe("general".into()).unwrap();
        next_matching(&events, |e| {
            matches!(
                e,
                ChatEvent::StreamDisconnected {
                    will_retry: false,
                    ..
                }
            )
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.live_streams() > 0 {
            assert!(Instant::now() < deadline, "room thread did not end");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(repo.open_stream_count(), 0);
    }

    #[test]
    fn watched_rooms_report_unread_through_the_ffi() {
        let (repo, app) = app();
        let events = app.events().unwrap();
        block_on(app.list_conversations()).unwrap();
        app.watch("general".into()).unwrap();
        assert!(app.is_watched("general".into()));
        assert!(!app.is_subscribed("general".into()));
        next_matching(&events, |e| matches!(e, ChatEvent::StreamConnected { .. }));

        let ping = repo.inject("general", "curl", "ping");
        next_matching(
            &events,
            |e| matches!(e, ChatEvent::Notify { message } if message.id == ping.id && message.author == "curl"),
        );
        next_matching(
            &events,
            |e| matches!(e, ChatEvent::ConversationUpdated { conversation } if conversation.unread_count == 1),
        );

        app.unwatch("general".into()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.live_streams() > 0 {
            assert!(Instant::now() < deadline, "room thread did not end");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn errors_map_to_the_ffi_error() {
        let (repo, app) = app();
        assert!(matches!(
            block_on(app.load_messages("nope".into(), None)),
            Err(ChatError::NotFound { .. })
        ));
        assert!(matches!(
            block_on(app.send_message("general".into(), " ".into())),
            Err(ChatError::InvalidInput { .. })
        ));
        repo.set_offline(true);
        let err = block_on(app.send_message("general".into(), "x".into())).unwrap_err();
        assert!(matches!(err, ChatError::Network { .. }));
        repo.set_offline(false);
        let id = app.cached_messages("general".into())[0].id.clone();
        assert_eq!(
            block_on(app.retry_message(id)).unwrap().status,
            ChatMessageStatus::Sent
        );
    }

    #[test]
    fn a_slow_consumer_gets_events_dropped_then_newest_events() {
        let (repo, app) = app();
        let events = app.events().unwrap();
        app.subscribe("general".into()).unwrap();
        next_matching(&events, |e| matches!(e, ChatEvent::StreamConnected { .. }));
        let burst = CHAT_QUEUE_CAPACITY * 2;
        let ids: Vec<String> = (0..burst)
            .map(|i| repo.inject("general", "bot", &format!("#{i}")).id)
            .collect();
        // Let the pump apply everything without anyone reading.
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.last_synced("general".into()) != Some(repo.messages("general")[burst - 1].sent_at)
        {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        let first = block_on(events.next()).unwrap();
        let ChatEvent::EventsDropped { count } = first else {
            panic!("expected EventsDropped, got {first:?}")
        };
        assert!(count as usize >= burst - CHAT_QUEUE_CAPACITY);
        // What survives is the newest events, ending with the last message.
        let rest = next_matching(
            &events,
            |e| matches!(e, ChatEvent::MessageReceived { message } if message.id == ids[burst - 1]),
        );
        assert!(rest.len() <= CHAT_QUEUE_CAPACITY);
        // The store is the source of truth after a drop.
        assert_eq!(
            app.cached_messages("general".into())
                .last()
                .map(|m| m.id.clone()),
            ids.last().cloned()
        );
        app.shutdown();
    }

    #[test]
    fn shutdown_closes_rooms_and_streams() {
        let (_repo, app) = app();
        let events = app.events().unwrap();
        app.subscribe("general".into()).unwrap();
        app.shutdown();
        app.shutdown();
        let mut rest = Vec::new();
        while let Some(event) = block_on(events.next()) {
            rest.push(event);
        }
        assert!(rest.contains(&ChatEvent::StreamDisconnected {
            conversation_id: "general".into(),
            reason: "shut down".into(),
            will_retry: false,
        }));
        assert!(app.is_shut_down());
        assert!(matches!(app.events(), Err(ChatError::ShutDown)));
        assert!(matches!(
            block_on(app.list_conversations()),
            Err(ChatError::ShutDown)
        ));
        assert!(matches!(
            app.subscribe("general".into()),
            Err(ChatError::ShutDown)
        ));
    }
}
