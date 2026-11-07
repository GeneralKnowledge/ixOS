use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::memory::InMemoryRepo;

const WAIT: Duration = Duration::from_secs(5);

struct Harness {
    repo: Arc<InMemoryRepo>,
    client: ChatClient,
    events: Receiver<ChatEvent>,
}

fn fast_config() -> ChatConfig {
    ChatConfig {
        user: "me".into(),
        user_id: String::new(),
        reconnect_initial: Duration::from_millis(10),
        reconnect_max: Duration::from_millis(40),
    }
}

fn harness_with(repo: Arc<InMemoryRepo>, store: ChatStore) -> Harness {
    let (tx, events) = mpsc::channel();
    let client = ChatClient::open(repo.clone(), store, fast_config(), move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    Harness {
        repo,
        client,
        events,
    }
}

fn harness() -> Harness {
    harness_with(
        Arc::new(InMemoryRepo::with_rooms(&[
            ("general", "General"),
            ("random", "Random"),
        ])),
        ChatStore::in_memory(),
    )
}

impl Harness {
    /// Waits for the first event matching `pred`, returning it and every
    /// event before it.
    fn wait_for(&self, what: &str, mut pred: impl FnMut(&ChatEvent) -> bool) -> Vec<ChatEvent> {
        let deadline = Instant::now() + WAIT;
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(event) => {
                    let done = pred(&event);
                    seen.push(event);
                    if done {
                        return seen;
                    }
                }
                Err(_) => panic!("timed out waiting for {what}; saw {seen:#?}"),
            }
        }
    }

    fn drain(&self) -> Vec<ChatEvent> {
        self.events.try_iter().collect()
    }

    fn wait_connected(&self, room: &str) {
        self.wait_for("StreamConnected", |e| {
            matches!(e, ChatEvent::StreamConnected { conversation_id } if conversation_id == room)
        });
    }
}

fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The one `Failed` message in a room. (Not "the last one": a local
/// message's device time can sort before a server message from the same
/// millisecond.)
fn failed_in(client: &ChatClient, room: &str) -> Message {
    let failed: Vec<_> = client
        .cached_messages(room)
        .into_iter()
        .filter(|m| m.status == MessageStatus::Failed)
        .collect();
    assert_eq!(failed.len(), 1, "{failed:?}");
    failed.into_iter().next().unwrap()
}

fn status_event(message: &Message, status: MessageStatus) -> ChatEvent {
    ChatEvent::MessageStatusChanged {
        conversation_id: message.conversation_id.clone(),
        id: message.id.clone(),
        status,
    }
}

#[test]
fn send_goes_sending_then_sent() {
    let h = harness();
    h.client.list_conversations().unwrap();
    let sent = h.client.send_message("general", "  hello  ").unwrap();
    assert_eq!(sent.status, MessageStatus::Sent);
    assert_eq!(sent.body, "hello");
    assert_eq!(sent.author, "me");

    let events = h.drain();
    let ChatEvent::MessageReceived(first) = &events[0] else {
        panic!("{events:?}")
    };
    assert_eq!(first.status, MessageStatus::Sending);
    assert_eq!(first.id, sent.id);
    // The list's preview shows the message while it is being sent...
    assert!(
        matches!(&events[1], ChatEvent::ConversationUpdated(c) if c.last_message.as_ref().is_some_and(|m| m.id == sent.id && m.status == MessageStatus::Sending))
    );
    assert_eq!(events[2], status_event(&sent, MessageStatus::Sent));
    // ...and once it is sent.
    assert!(
        matches!(&events[3], ChatEvent::ConversationUpdated(c) if c.last_message_at == Some(sent.sent_at) && c.last_message.as_ref() == Some(&sent))
    );
    assert_eq!(h.client.cached_messages("general"), vec![sent]);
    // Our own send does not advance the cursor.
    assert_eq!(h.client.last_synced("general"), None);
}

#[test]
fn bad_input_is_rejected_before_the_network() {
    let h = harness();
    for (room, body) in [("general", "   "), ("", "hi")] {
        assert!(matches!(
            h.client.send_message(room, body),
            Err(ChatError::InvalidInput { .. })
        ));
    }
    let long = "x".repeat(MAX_BODY_CHARS + 1);
    assert!(matches!(
        h.client.send_message("general", &long),
        Err(ChatError::InvalidInput { .. })
    ));
    assert!(matches!(
        h.client.retry_message("nope"),
        Err(ChatError::InvalidInput { .. })
    ));
    assert!(h.repo.calls().is_empty());
    assert!(h.drain().is_empty());
}

#[test]
fn refused_send_is_failed_and_retry_is_idempotent() {
    let h = harness();
    h.repo.set_rejecting(true);
    let err = h.client.send_message("general", "hi").unwrap_err();
    assert!(
        matches!(err, ChatError::Http { status: 400, .. }),
        "{err:?}"
    );
    let failed = failed_in(&h.client, "general");
    let events = h.drain();
    assert_eq!(
        events.last(),
        Some(&status_event(&failed, MessageStatus::Failed))
    );
    assert!(h.client.outbox().is_empty(), "a refusal is not queued");

    // Still refused: retry fails again, back to Failed.
    assert!(h.client.retry_message(&failed.id).is_err());
    assert_eq!(
        h.drain(),
        vec![
            status_event(&failed, MessageStatus::Sending),
            status_event(&failed, MessageStatus::Failed),
        ]
    );

    h.repo.set_rejecting(false);
    let sent = h.client.retry_message(&failed.id).unwrap();
    assert_eq!(sent.id, failed.id);
    assert_eq!(sent.status, MessageStatus::Sent);
    // Retrying a sent message is a no-op that returns it.
    assert_eq!(h.client.retry_message(&failed.id).unwrap(), sent);
    assert_eq!(h.repo.messages("general").len(), 1);
}

#[test]
fn offline_sends_wait_in_the_outbox_and_go_out_in_order() {
    let h = harness();
    h.repo.set_offline(true);
    let first = h.client.send_message("general", "one").unwrap();
    let second = h.client.send_message("general", "two").unwrap();
    assert_eq!(first.status, MessageStatus::Sending);
    assert_eq!(second.status, MessageStatus::Sending);
    let queued: Vec<_> = h.client.outbox().into_iter().map(|m| m.body).collect();
    assert_eq!(queued, ["one", "two"]);
    assert!(
        !h.drain().iter().any(|e| matches!(
            e,
            ChatEvent::MessageStatusChanged {
                status: MessageStatus::Failed,
                ..
            }
        )),
        "nothing fails while the server is only unreachable"
    );

    h.repo.set_offline(false);
    h.client.flush_outbox();
    h.wait_for("the second message sent", |e| {
        *e == status_event(&second, MessageStatus::Sent)
    });
    assert!(h.client.outbox().is_empty());
    let on_server: Vec<_> = h
        .repo
        .messages("general")
        .into_iter()
        .map(|m| m.body)
        .collect();
    assert_eq!(on_server, ["one", "two"], "sent once each, in order");
}

#[test]
fn a_queued_message_survives_a_relaunch_and_is_sent_then() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Arc::new(InMemoryRepo::with_rooms(&[("general", "General")]));
    let queued = {
        let h = harness_with(repo.clone(), ChatStore::in_directory(dir.path()));
        h.client.list_conversations().unwrap();
        repo.set_offline(true);
        let queued = h.client.send_message("general", "typed offline").unwrap();
        assert_eq!(queued.status, MessageStatus::Sending);
        // The app goes away with the message still queued.
        h.client.shutdown();
        queued
    };

    repo.set_offline(false);
    let h = harness_with(repo.clone(), ChatStore::in_directory(dir.path()));
    // The new run sends it by itself, with the same id.
    h.wait_for("the queued message sent", |e| {
        *e == status_event(&queued, MessageStatus::Sent)
    });
    let on_server = repo.messages("general");
    assert_eq!(on_server.len(), 1);
    assert_eq!(on_server[0].id, queued.id);
}

#[test]
fn subscribe_delivers_external_messages_and_unsubscribe_stops_the_pump() {
    let h = harness();
    h.client.subscribe("general").unwrap();
    h.client.subscribe("general").unwrap(); // idempotent
    h.wait_connected("general");
    assert_eq!(h.client.live_pumps(), 1);
    assert_eq!(h.repo.open_stream_count(), 1);

    let incoming = h.repo.inject("general", "ana", "hi from curl");
    h.wait_for(
        "the injected message",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.id == incoming.id),
    );
    assert_eq!(h.client.last_synced("general"), Some(incoming.sent_at));

    h.client.unsubscribe("general").unwrap();
    assert_eq!(
        h.drain().last(),
        Some(&ChatEvent::StreamDisconnected {
            conversation_id: "general".into(),
            reason: "unsubscribed".into(),
            will_retry: false,
        })
    );
    assert!(!h.client.is_subscribed("general"));

    // The Rust side really stops: the pump thread exits and drops its stream.
    wait_until("the pump has exited", || h.client.live_pumps() == 0);
    wait_until("the stream is dropped", || h.repo.open_stream_count() == 0);

    // And nothing more is emitted for that room.
    h.repo.inject("general", "ana", "after leaving");
    std::thread::sleep(Duration::from_millis(50));
    assert!(h.drain().is_empty());
    h.client.unsubscribe("general").unwrap(); // idempotent
}

#[test]
fn stream_reconnects_and_catches_up_with_since() {
    let h = harness();
    let before = h.repo.inject("general", "ana", "before");
    h.client.subscribe("general").unwrap();
    h.wait_for(
        "catch-up of the old message",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.id == before.id),
    );

    h.repo.set_offline(true);
    h.wait_for("disconnect with retry", |e| {
        matches!(
            e,
            ChatEvent::StreamDisconnected {
                will_retry: true,
                ..
            }
        )
    });
    let missed = h.repo.inject("general", "ana", "while you were away");
    std::thread::sleep(Duration::from_millis(60)); // several failed attempts
    h.repo.set_offline(false);

    let events = h.wait_for(
        "the missed message",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.id == missed.id),
    );
    // One disconnect per outage, not one per failed attempt.
    let disconnects = events
        .iter()
        .filter(|e| matches!(e, ChatEvent::StreamDisconnected { .. }))
        .count();
    assert_eq!(disconnects, 0, "{events:#?}");
    assert!(events
        .iter()
        .any(|e| matches!(e, ChatEvent::StreamConnected { .. })));
    // The catch-up after reconnecting asked only for what was missed.
    assert!(
        h.repo
            .calls()
            .contains(&format!("load general {}", before.sent_at)),
        "{:?}",
        h.repo.calls()
    );
    h.client.shutdown();
}

#[test]
fn own_message_echo_is_merged_not_duplicated() {
    let h = harness();
    h.client.subscribe("general").unwrap();
    h.wait_connected("general");
    let sent = h.client.send_message("general", "echo me").unwrap();
    std::thread::sleep(Duration::from_millis(60)); // let the echo arrive
    let events = h.drain();
    let received = events
        .iter()
        .filter(|e| matches!(e, ChatEvent::MessageReceived(m) if m.id == sent.id))
        .count();
    let sent_changes = events
        .iter()
        .filter(|e| **e == status_event(&sent, MessageStatus::Sent))
        .count();
    assert_eq!((received, sent_changes), (1, 1), "{events:#?}");
    assert_eq!(h.client.cached_messages("general").len(), 1);
    // The echo came through the stream, so it does advance the cursor.
    wait_until("cursor advanced", || {
        h.client.last_synced("general") == Some(sent.sent_at)
    });
    h.client.shutdown();
}

#[test]
fn unread_counts_messages_that_arrive_while_a_room_is_closed() {
    let h = harness();
    h.client.list_conversations().unwrap();
    h.repo.inject("random", "ana", "one");
    h.repo.inject("random", "bo", "two");
    let mine = h.client.send_message("random", "mine").unwrap();
    h.repo.inject("random", "ana", "three");
    h.drain();

    let list = h.client.list_conversations().unwrap();
    let random = list.iter().find(|c| c.id == "random").unwrap();
    assert_eq!(random.unread_count, 3, "own messages do not count");
    assert!(random.last_message_at > Some(mine.sent_at));
    let received = h
        .drain()
        .into_iter()
        .filter(|e| matches!(e, ChatEvent::MessageReceived(_)))
        .count();
    assert_eq!(received, 3, "prefetched messages are announced");
    assert_eq!(h.client.cached_messages("random").len(), 4);

    // Rooms without news are not fetched.
    let loads = |calls: Vec<String>| calls.iter().filter(|c| c.starts_with("load")).count();
    let before = loads(h.repo.calls());
    h.client.list_conversations().unwrap();
    assert_eq!(loads(h.repo.calls()), before);

    // Opening the room marks it read.
    h.client.subscribe("random").unwrap();
    let events = h.wait_for("unread reset", |e| {
        matches!(e, ChatEvent::ConversationUpdated(c) if c.id == "random" && c.unread_count == 0)
    });
    assert_eq!(events.len(), 1);
    h.client.shutdown();
}

#[test]
fn watched_rooms_stay_live_and_count_unread() {
    let h = harness();
    h.client.list_conversations().unwrap();
    h.client.watch("random").unwrap();
    h.client.watch("random").unwrap(); // idempotent
    h.wait_connected("random");
    assert!(h.client.is_watched("random"));
    assert!(!h.client.is_subscribed("random"));

    // Someone else's message arrives live, counts as unread and notifies,
    // right after its MessageReceived.
    let incoming = h.repo.inject("random", "ana", "while watching");
    let events = h.wait_for("the unread bump", |e| {
        matches!(e, ChatEvent::ConversationUpdated(c) if c.id == "random" && c.unread_count == 1)
    });
    let received = events
        .iter()
        .position(|e| matches!(e, ChatEvent::MessageReceived(m) if m.id == incoming.id))
        .expect("MessageReceived");
    assert!(matches!(&events[received + 1], ChatEvent::Notify(m) if m.id == incoming.id));

    // Our own messages never count or notify.
    h.client.send_message("random", "mine").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert!(!h.drain().iter().any(|e| matches!(e, ChatEvent::Notify(_))));
    let random = h
        .client
        .cached_conversations()
        .into_iter()
        .find(|c| c.id == "random")
        .unwrap();
    assert_eq!(random.unread_count, 1);

    // Opening a watched room reuses its stream and marks it read.
    h.client.subscribe("random").unwrap();
    assert_eq!(h.repo.open_stream_count(), 1);
    h.repo.inject("random", "ana", "while open");
    h.wait_for(
        "the open-room message",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.body == "while open"),
    );
    let random = h
        .client
        .cached_conversations()
        .into_iter()
        .find(|c| c.id == "random")
        .unwrap();
    assert_eq!(random.unread_count, 0, "an open room stays read");
    assert!(
        !h.drain().iter().any(|e| matches!(e, ChatEvent::Notify(_))),
        "an open room never notifies"
    );

    // Closing it keeps the watch: no disconnect, and unread counts again.
    h.drain();
    h.client.unsubscribe("random").unwrap();
    assert!(h.drain().is_empty(), "the stream keeps running");
    assert_eq!(h.client.live_pumps(), 1);
    h.repo.inject("random", "ana", "after closing");
    h.wait_for("unread again", |e| {
        matches!(e, ChatEvent::ConversationUpdated(c) if c.id == "random" && c.unread_count == 1)
    });

    // Unwatching ends it for real.
    h.client.unwatch("random").unwrap();
    assert_eq!(
        h.drain().last(),
        Some(&ChatEvent::StreamDisconnected {
            conversation_id: "random".into(),
            reason: "unsubscribed".into(),
            will_retry: false,
        })
    );
    wait_until("the pump has exited", || h.client.live_pumps() == 0);
    h.client.unwatch("random").unwrap(); // idempotent
}

#[test]
fn only_messages_sent_after_start_notify() {
    let repo = Arc::new(InMemoryRepo::with_rooms(&[("random", "Random")]));
    repo.inject("random", "ana", "history");
    std::thread::sleep(Duration::from_millis(5));
    let h = harness_with(repo, ChatStore::in_memory());
    // The refresh prefetches the history: unread, but no notification.
    h.client.list_conversations().unwrap();
    h.client.watch("random").unwrap();
    h.wait_connected("random");
    let fresh = h.repo.inject("random", "bo", "fresh");
    let events = h.wait_for("the fresh notification", |e| {
        matches!(e, ChatEvent::Notify(_))
    });
    let notified: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ChatEvent::Notify(m) => Some(m.id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(notified, [fresh.id], "catch-up history does not notify");
    let random = &h.client.cached_conversations()[0];
    assert_eq!(random.unread_count, 2, "but it still counts as unread");
    h.client.shutdown();
}

#[test]
fn watching_many_rooms_and_leaving_one_leaves_the_others_live() {
    let h = harness();
    h.client.watch("general").unwrap();
    h.client.watch("random").unwrap();
    h.client.subscribe("general").unwrap();
    // Either room may connect first; wait for both.
    let connected = std::cell::RefCell::new(std::collections::HashSet::new());
    h.wait_for("both rooms connected", |e| {
        if let ChatEvent::StreamConnected { conversation_id } = e {
            connected.borrow_mut().insert(conversation_id.clone());
        }
        connected.borrow().len() == 2
    });
    assert_eq!(h.client.live_pumps(), 2);

    h.client.unwatch("random").unwrap();
    h.client.unsubscribe("general").unwrap(); // still watched
    wait_until("random's pump has exited", || h.client.live_pumps() == 1);
    let incoming = h.repo.inject("general", "ana", "still live");
    h.wait_for(
        "general still delivers",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.id == incoming.id),
    );

    h.client.shutdown();
    assert!(!h.client.is_watched("general"));
    assert_eq!(h.client.watch("general"), Err(ChatError::ShutDown));
    wait_until("every pump has exited", || h.client.live_pumps() == 0);
}

#[test]
fn load_messages_merges_and_respects_the_cursor() {
    let h = harness();
    let a = h.repo.inject("general", "ana", "a");
    let b = h.repo.inject("general", "ana", "b");

    // A fetch starting past our (missing) cursor must not create a gap.
    let got = h.client.load_messages("general", Some(a.sent_at)).unwrap();
    assert_eq!(got, vec![b.clone()]);
    assert_eq!(h.client.last_synced("general"), None);

    let got = h.client.load_messages("general", None).unwrap();
    assert_eq!(got, vec![a, b.clone()]);
    assert_eq!(h.client.last_synced("general"), Some(b.sent_at));

    let c = h.repo.inject("general", "ana", "c");
    let got = h.client.sync_messages("general").unwrap();
    assert_eq!(got.last(), Some(&c));
    assert!(h
        .repo
        .calls()
        .contains(&format!("load general {}", b.sent_at)));

    assert!(matches!(
        h.client.load_messages("nope", None),
        Err(ChatError::NotFound { .. })
    ));
}

#[test]
fn subscribing_to_a_missing_room_ends_the_subscription() {
    let h = harness();
    h.client.subscribe("nope").unwrap();
    h.wait_for("terminal disconnect", |e| {
        matches!(e, ChatEvent::StreamDisconnected { will_retry: false, reason, .. } if reason == "conversation not found")
    });
    wait_until("pump exited", || h.client.live_pumps() == 0);
    assert!(!h.client.is_subscribed("nope"));
}

#[test]
fn shutdown_ends_everything_and_refuses_commands() {
    let h = harness();
    h.client.subscribe("general").unwrap();
    h.client.subscribe("random").unwrap();
    h.wait_connected("general");
    h.client.shutdown();
    h.client.shutdown(); // idempotent
    let disconnects: Vec<_> = h
        .drain()
        .into_iter()
        .filter_map(|e| match e {
            ChatEvent::StreamDisconnected {
                conversation_id,
                reason,
                will_retry: false,
            } if reason == "shut down" => Some(conversation_id),
            _ => None,
        })
        .collect();
    assert_eq!(disconnects, ["general", "random"]);
    wait_until("pumps exited", || h.client.live_pumps() == 0);
    assert_eq!(h.client.list_conversations(), Err(ChatError::ShutDown));
    assert_eq!(
        h.client.send_message("general", "x"),
        Err(ChatError::ShutDown)
    );
    assert_eq!(h.client.subscribe("general"), Err(ChatError::ShutDown));
    assert_eq!(h.client.unsubscribe("general"), Ok(()));
}

#[test]
fn store_survives_restart_and_failed_messages_stay_retryable() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Arc::new(InMemoryRepo::with_rooms(&[("general", "General")]));
    let failed_id = {
        let h = harness_with(repo.clone(), ChatStore::in_directory(dir.path()));
        h.client.list_conversations().unwrap();
        repo.inject("general", "ana", "hello");
        h.client.sync_messages("general").unwrap();
        repo.set_rejecting(true);
        h.client.send_message("general", "lost").unwrap_err();
        let id = failed_in(&h.client, "general").id;
        h.client.shutdown();
        id
    };

    repo.set_rejecting(false);
    let h = harness_with(repo.clone(), ChatStore::in_directory(dir.path()));
    assert_eq!(h.client.cached_conversations().len(), 1);
    let messages = h.client.cached_messages("general");
    assert_eq!(messages.len(), 2);
    assert_eq!(failed_in(&h.client, "general").id, failed_id);
    assert!(h.client.last_synced("general").is_some());
    assert_eq!(
        h.client.retry_message(&failed_id).unwrap().status,
        MessageStatus::Sent
    );
    assert!(h.client.store_error().is_none());
}

#[test]
fn corrupt_store_starts_empty_but_newer_store_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(crate::store::DATABASE_FILENAME);
    let repo = Arc::new(InMemoryRepo::new());

    // A damaged database is set aside (kept) and the client starts empty.
    std::fs::write(&path, b"garbage, not SQLite").unwrap();
    {
        let h = harness_with(repo.clone(), ChatStore::in_directory(dir.path()));
        assert!(matches!(
            h.client.store_error(),
            Some(StoreError::Corrupt { .. })
        ));
        assert!(h.client.cached_conversations().is_empty());
        h.client.shutdown();
    }
    let kept = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
    assert!(kept, "the damaged file is kept for inspection");

    // A database from a newer build is refused and left alone.
    let connection = forge_storage::rusqlite::Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    let before = std::fs::read(&path).unwrap();
    let opened = ChatClient::open(
        repo,
        ChatStore::in_directory(dir.path()),
        fast_config(),
        |_| {},
    );
    assert!(matches!(
        opened,
        Err(StoreError::UnsupportedVersion { found: 99, .. })
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn a_json_store_from_before_m2_is_imported_once() {
    let dir = tempfile::tempdir().unwrap();
    let json = serde_json::json!({
        "schema_version": 1,
        "conversations": [{"id": "general", "title": "General", "last_message_at": 5, "unread_count": 0}],
        "messages": {"general": [{"id": "old", "conversation_id": "general", "author": "ana",
                                  "body": "from the JSON days", "sent_at": 5, "status": "sent"}]},
        "last_synced": {"general": 5}
    });
    let path = dir.path().join(crate::store::STORE_FILENAME);
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    let repo = Arc::new(InMemoryRepo::with_rooms(&[("general", "General")]));
    {
        let h = harness_with(repo.clone(), ChatStore::in_directory(dir.path()));
        assert!(h.client.store_error().is_none());
        assert_eq!(
            h.client.cached_messages("general")[0].body,
            "from the JSON days"
        );
        assert_eq!(h.client.last_synced("general"), Some(5));
        assert_eq!(h.client.search("json", None).unwrap().len(), 1);
        h.client.shutdown();
    }
    assert!(!path.exists(), "imported once, then set aside");
    let h = harness_with(repo, ChatStore::in_directory(dir.path()));
    assert_eq!(h.client.cached_messages("general").len(), 1);
}

#[test]
fn high_frequency_stream_delivers_everything_once_in_order() {
    const N: usize = 1000;
    let h = harness();
    h.client.subscribe("general").unwrap();
    h.wait_connected("general");
    let injected: Vec<String> = (0..N)
        .map(|i| h.repo.inject("general", "bot", &format!("#{i}")).id)
        .collect();
    let last = injected.last().unwrap().clone();
    let events = h.wait_for(
        "the last message",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.id == last),
    );
    let received: Vec<String> = events
        .into_iter()
        .filter_map(|e| match e {
            ChatEvent::MessageReceived(m) => Some(m.id),
            _ => None,
        })
        .collect();
    assert_eq!(received, injected);
    assert_eq!(
        h.client.cached_messages("general").len(),
        crate::store::MAX_MESSAGES_PER_CONVERSATION
    );
    h.client.shutdown();
}

/// The demo script from docs/chat-demo.md, against the in-memory server.
#[test]
fn killer_demo_interaction() {
    let h = harness();
    h.client.list_conversations().unwrap();

    // 1. Open a chat, send a message.
    h.client.subscribe("general").unwrap();
    h.wait_connected("general");
    h.client.sync_messages("general").unwrap();
    let first = h.client.send_message("general", "first").unwrap();
    assert_eq!(first.status, MessageStatus::Sent);

    // 2. Navigate back to the list. Leaving does not cancel anything.
    h.client.unsubscribe("general").unwrap();
    h.client.list_conversations().unwrap();
    h.client.subscribe("general").unwrap();
    h.wait_connected("general");

    // 3. Kill the server.
    h.repo.set_offline(true);
    h.wait_for("stream drop", |e| {
        matches!(
            e,
            ChatEvent::StreamDisconnected {
                will_retry: true,
                ..
            }
        )
    });

    // 4. Send again: it waits in the outbox, still Sending.
    let queued = h.client.send_message("general", "second").unwrap();
    assert_eq!(queued.status, MessageStatus::Sending);

    // 5. Restart the server: the stream reconnects and the outbox sends it.
    h.repo.set_offline(false);
    let events = h.wait_for("Sent once the server is back", |e| {
        *e == status_event(&queued, MessageStatus::Sent)
    });
    if !events
        .iter()
        .any(|e| matches!(e, ChatEvent::StreamConnected { .. }))
    {
        h.wait_connected("general");
    }

    // 6. A message from curl appears via the stream.
    let external = h.repo.inject("general", "curl", "hello from curl");
    let started = Instant::now();
    h.wait_for(
        "external message",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.id == external.id),
    );
    assert!(started.elapsed() < Duration::from_secs(1));

    let bodies: Vec<_> = h
        .client
        .cached_messages("general")
        .into_iter()
        .map(|m| (m.body, m.status))
        .collect();
    assert_eq!(
        bodies,
        vec![
            ("first".to_string(), MessageStatus::Sent),
            ("second".to_string(), MessageStatus::Sent),
            ("hello from curl".to_string(), MessageStatus::Sent),
        ]
    );
    h.client.shutdown();
    wait_until("pumps exited", || h.client.live_pumps() == 0);
}

/// Harness whose server never sends heartbeats: an idle stream blocks in
/// `next_event` until something cancels it.
fn silent_harness() -> Harness {
    let repo = Arc::new(InMemoryRepo::with_rooms(&[
        ("general", "General"),
        ("random", "Random"),
    ]));
    repo.set_heartbeats(false);
    harness_with(repo, ChatStore::in_memory())
}

/// Well under any heartbeat interval a real server would use (the mock
/// server's default is 15 s), so passing means the cancel woke the pump.
const PROMPT: Duration = Duration::from_secs(2);

fn wait_promptly(what: &str, cond: impl Fn() -> bool) {
    let started = Instant::now();
    while !cond() {
        assert!(started.elapsed() < PROMPT, "not prompt: {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn unsubscribe_ends_a_silent_stream_without_a_heartbeat() {
    let h = silent_harness();
    h.client.subscribe("general").unwrap();
    h.wait_connected("general");
    // Let the pump finish catch-up and block in next_event.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(h.repo.open_stream_count(), 1);

    h.client.unsubscribe("general").unwrap();
    wait_promptly("the pump exits", || h.client.live_pumps() == 0);
    wait_promptly("the stream closes", || h.repo.open_stream_count() == 0);
}

#[test]
fn cancelling_one_room_leaves_the_other_streaming() {
    let h = silent_harness();
    h.client.subscribe("general").unwrap();
    h.client.subscribe("random").unwrap();
    wait_until("both streams are open", || h.repo.open_stream_count() == 2);
    // Both pumps are past catch-up once both have connected.
    let mut connected = 0;
    h.wait_for("both rooms connected", |e| {
        if matches!(e, ChatEvent::StreamConnected { .. }) {
            connected += 1;
        }
        connected == 2
    });

    h.client.unsubscribe("general").unwrap();
    wait_promptly("general's pump exits", || h.client.live_pumps() == 1);
    assert_eq!(h.repo.open_stream_count(), 1);
    assert!(h.client.is_subscribed("random"));

    let incoming = h.repo.inject("random", "ana", "still here");
    let seen = h.wait_for(
        "the message in the room still open",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.id == incoming.id),
    );
    // No disconnect or retry for the room that stayed open.
    assert!(
        !seen.iter().any(|e| matches!(
            e,
            ChatEvent::StreamDisconnected { conversation_id, .. } if conversation_id == "random"
        )),
        "{seen:#?}"
    );
    h.client.unsubscribe("random").unwrap();
    wait_promptly("random's pump exits", || h.client.live_pumps() == 0);
}

#[test]
fn shutdown_ends_every_silent_stream_promptly() {
    let h = silent_harness();
    h.client.subscribe("general").unwrap();
    h.client.subscribe("random").unwrap();
    wait_until("both streams are open", || h.repo.open_stream_count() == 2);
    // Both pumps are past catch-up once both have connected.
    let mut connected = 0;
    h.wait_for("both rooms connected", |e| {
        if matches!(e, ChatEvent::StreamConnected { .. }) {
            connected += 1;
        }
        connected == 2
    });

    h.client.shutdown();
    wait_promptly("every pump exits", || h.client.live_pumps() == 0);
    wait_promptly("every stream closes", || h.repo.open_stream_count() == 0);
}

#[test]
fn unsubscribe_wakes_a_pump_in_a_long_backoff() {
    let repo = Arc::new(InMemoryRepo::with_rooms(&[("general", "General")]));
    repo.set_offline(true);
    let (tx, events) = mpsc::channel();
    let config = ChatConfig {
        reconnect_initial: Duration::from_secs(60),
        reconnect_max: Duration::from_secs(60),
        ..fast_config()
    };
    let client = ChatClient::open(repo.clone(), ChatStore::in_memory(), config, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let h = Harness {
        repo,
        client,
        events,
    };
    h.client.subscribe("general").unwrap();
    h.wait_for("the outage", |e| {
        matches!(
            e,
            ChatEvent::StreamDisconnected {
                will_retry: true,
                ..
            }
        )
    });
    h.client.unsubscribe("general").unwrap();
    wait_promptly("the pump leaves its 60 s backoff", || {
        h.client.live_pumps() == 0
    });
}

#[test]
fn repeated_subscribe_cycles_leak_no_threads_or_streams() {
    let h = silent_harness();
    for _ in 0..25 {
        h.client.subscribe("general").unwrap();
        h.wait_connected("general");
        h.client.unsubscribe("general").unwrap();
    }
    wait_promptly("every pump exits", || h.client.live_pumps() == 0);
    wait_promptly("every stream closes", || h.repo.open_stream_count() == 0);
}

#[test]
fn pause_stops_streams_and_resume_catches_up() {
    let h = harness();
    h.client.list_conversations().unwrap();
    h.client.subscribe("general").unwrap();
    h.client.watch("random").unwrap();
    // Both rooms connect in either order: wait for the pair in one pass, so
    // an early "random" is not consumed while waiting for "general".
    let mut pending = vec!["general", "random"];
    h.wait_for("both rooms connected", |e| {
        if let ChatEvent::StreamConnected { conversation_id } = e {
            pending.retain(|room| room != conversation_id);
        }
        pending.is_empty()
    });

    h.client.pause_streams();
    assert!(h.client.is_paused());
    wait_until("pumps end", || h.client.live_pumps() == 0);
    wait_until("streams close", || h.repo.open_stream_count() == 0);
    let paused: Vec<_> = h
        .drain()
        .into_iter()
        .filter(|e| matches!(e, ChatEvent::StreamDisconnected { reason, will_retry: true, .. } if reason == "paused"))
        .collect();
    assert_eq!(paused.len(), 2, "{paused:?}");
    // The rooms are still open/watched, and a new open only records intent.
    assert!(h.client.is_subscribed("general"));
    assert!(h.client.is_watched("random"));

    // Sent while the app was away.
    let missed = h.repo.inject("random", "ana", "while you were away");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(h.client.live_pumps(), 0);

    h.client.resume_streams().unwrap();
    h.wait_for(
        "the missed message",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.id == missed.id),
    );
    wait_until("both rooms live", || h.client.live_pumps() == 2);
    h.client.resume_streams().unwrap();
    assert_eq!(h.client.live_pumps(), 2);
    h.client.shutdown();
}

#[test]
fn own_messages_are_recognised_by_account_id() {
    let repo = Arc::new(InMemoryRepo::with_rooms(&[("general", "General")]));
    let (tx, events) = mpsc::channel();
    let config = ChatConfig {
        user: "Sam".into(),
        user_id: "u-me".into(),
        ..fast_config()
    };
    let client = ChatClient::open(repo.clone(), ChatStore::in_memory(), config, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let h = Harness {
        repo,
        client,
        events,
    };
    h.client.list_conversations().unwrap();
    let mine = h.client.send_message("general", "hi").unwrap();
    assert_eq!(mine.author_id, "u-me");
    assert!(h.client.config().is_own(&mine));
    h.client.watch("general").unwrap();
    h.wait_connected("general");
    // Someone else with the same display name is not us.
    let other = h.repo.inject("general", "Sam", "same name, other person");
    h.wait_for(
        "the other Sam's message",
        |e| matches!(e, ChatEvent::Notify(m) if m.id == other.id),
    );
    assert!(!h.client.config().is_own(&other));
    h.client.shutdown();
}

// ----- M1: read cursors, previews, mute, marked unread, new rooms ----------

/// A harness whose server keeps read cursors; the client reads as "" (its
/// `fast_config` account id).
fn cursor_harness() -> Harness {
    harness_with(
        Arc::new(
            InMemoryRepo::with_rooms(&[("general", "General"), ("random", "Random")])
                .with_read_cursors(""),
        ),
        ChatStore::in_memory(),
    )
}

fn conversation(client: &ChatClient, id: &str) -> Conversation {
    client
        .cached_conversations()
        .into_iter()
        .find(|c| c.id == id)
        .unwrap()
}

#[test]
fn server_cursors_drive_unread_previews_and_mark_read() {
    let h = cursor_harness();
    h.client.list_conversations().unwrap();
    h.repo.inject("general", "ana", "one");
    let two = h.repo.inject("general", "ana", "two");

    let listed = h.client.list_conversations().unwrap();
    let general = listed.iter().find(|c| c.id == "general").unwrap();
    assert_eq!(general.unread_count, 2);
    assert!(general.shows_unread());
    assert_eq!(general.last_message.as_ref().unwrap().body, "two");
    assert_eq!(general.read_up_to, Some(i64::MIN));

    h.client.mark_read("general").unwrap();
    let general = conversation(&h.client, "general");
    assert_eq!(general.unread_count, 0);
    assert_eq!(general.read_up_to, Some(two.sent_at));
    assert_eq!(h.repo.read_cursor("general", ""), Some(two.sent_at));
    // A relaunch or refresh takes the server's word for it.
    let listed = h.client.list_conversations().unwrap();
    assert_eq!(listed[0].unread_count, 0);
    assert!(matches!(
        h.client.mark_read("nope"),
        Err(ChatError::NotFound { .. })
    ));
}

#[test]
fn an_open_room_is_marked_read_on_the_server_as_messages_arrive() {
    let h = cursor_harness();
    h.client.list_conversations().unwrap();
    h.client.subscribe("general").unwrap();
    h.wait_connected("general");
    let hello = h.repo.inject("general", "ana", "hello");
    wait_until("the server has our cursor", || {
        h.repo.read_cursor("general", "") == Some(hello.sent_at)
    });
    h.client.unsubscribe("general").unwrap();
    assert_eq!(conversation(&h.client, "general").unread_count, 0);
}

#[test]
fn others_reading_marks_our_messages_read() {
    let h = cursor_harness();
    h.client.list_conversations().unwrap();
    h.client.watch("general").unwrap();
    h.wait_connected("general");
    let sent = h
        .client
        .send_message("general", "did you see this?")
        .unwrap();
    assert!(!conversation(&h.client, "general").is_read_by_others(&sent));

    h.repo.inject_read("general", "ana", sent.sent_at);
    h.wait_for("others read it", |e| {
        matches!(e, ChatEvent::ConversationUpdated(c) if c.others_read_up_to == Some(sent.sent_at))
    });
    assert!(conversation(&h.client, "general").is_read_by_others(&sent));
    // Our own cursor moved when we wrote it, so it is not unread for us.
    assert_eq!(
        conversation(&h.client, "general").read_up_to,
        Some(sent.sent_at)
    );
}

#[test]
fn reading_on_another_device_clears_unread_here() {
    let h = cursor_harness();
    h.client.list_conversations().unwrap();
    h.client.watch("general").unwrap();
    h.wait_connected("general");
    let hello = h.repo.inject("general", "ana", "hello");
    h.wait_for(
        "an unread message",
        |e| matches!(e, ChatEvent::ConversationUpdated(c) if c.unread_count == 1),
    );
    h.client.set_marked_unread("general", true).unwrap();
    // The same account reads it elsewhere.
    h.repo.inject_read("general", "", hello.sent_at);
    h.wait_for("read elsewhere", |e| {
        matches!(e, ChatEvent::ConversationUpdated(c) if c.unread_count == 0 && !c.marked_unread)
    });
}

#[test]
fn muted_rooms_count_unread_but_do_not_notify() {
    let h = cursor_harness();
    h.client.list_conversations().unwrap();
    h.client.set_muted("general", true).unwrap();
    assert!(conversation(&h.client, "general").muted);
    h.client.watch("general").unwrap();
    h.wait_connected("general");
    h.drain();
    let quiet = h.repo.inject("general", "ana", "shh");
    let seen = h.wait_for(
        "the unread count",
        |e| matches!(e, ChatEvent::ConversationUpdated(c) if c.unread_count == 1),
    );
    assert!(seen
        .iter()
        .any(|e| matches!(e, ChatEvent::MessageReceived(m) if m.id == quiet.id)));
    std::thread::sleep(Duration::from_millis(50));
    assert!(!seen
        .iter()
        .chain(h.drain().iter())
        .any(|e| matches!(e, ChatEvent::Notify(_))));

    h.client.set_muted("general", false).unwrap();
    let loud = h.repo.inject("general", "ana", "hey");
    h.wait_for(
        "a notification",
        |e| matches!(e, ChatEvent::Notify(m) if m.id == loud.id),
    );
}

#[test]
fn marked_unread_lasts_until_the_room_is_opened() {
    let h = harness();
    h.client.list_conversations().unwrap();
    h.client.set_marked_unread("random", true).unwrap();
    let random = conversation(&h.client, "random");
    assert!(random.marked_unread && random.shows_unread());
    assert_eq!(random.unread_count, 0);
    h.client.subscribe("random").unwrap();
    assert!(!conversation(&h.client, "random").marked_unread);
    h.client.set_marked_unread("random", true).unwrap();
    h.client.mark_read("random").unwrap();
    assert!(!conversation(&h.client, "random").marked_unread);
    assert!(matches!(
        h.client.set_muted("nope", true),
        Err(ChatError::NotFound { .. })
    ));
}

#[test]
fn new_conversations_are_created_on_the_server_and_listed() {
    let h = cursor_harness();
    h.client.list_conversations().unwrap();
    h.drain();
    let room = h.client.create_conversation("  Weekend plans ").unwrap();
    assert_eq!(room.title, "Weekend plans");
    assert!(matches!(
        &h.drain()[..],
        [ChatEvent::ConversationUpdated(c)] if c.id == room.id
    ));
    assert_eq!(h.client.cached_conversations().last().unwrap().id, room.id);
    // It is a real room: messages go through.
    h.client.send_message(&room.id, "first!").unwrap();
    let listed = h.client.list_conversations().unwrap();
    assert_eq!(
        listed.last().unwrap().last_message.as_ref().unwrap().body,
        "first!"
    );
    for bad in ["", "   ", &"x".repeat(61)] {
        assert!(matches!(
            h.client.create_conversation(bad),
            Err(ChatError::InvalidInput { .. })
        ));
    }
}

#[test]
fn a_failed_send_shows_in_the_preview() {
    let h = harness();
    h.client.list_conversations().unwrap();
    h.repo.set_rejecting(true);
    assert!(h.client.send_message("general", "lost").is_err());
    let preview = conversation(&h.client, "general").last_message.unwrap();
    assert_eq!(preview.body, "lost");
    assert_eq!(preview.status, MessageStatus::Failed);
}

// ----- M2: replies, edits, deletions, reactions, drafts ------------------

fn updated(events: &[ChatEvent], id: &str) -> Option<Message> {
    events.iter().rev().find_map(|e| match e {
        ChatEvent::MessageUpdated(m) if m.id == id => Some(m.clone()),
        _ => None,
    })
}

#[test]
fn replies_point_at_a_message_in_the_same_room() {
    let h = cursor_harness();
    h.client.list_conversations().unwrap();
    let question = h.repo.inject("general", "ana", "Lake on Saturday?");
    h.client.sync_messages("general").unwrap();
    let answer = h
        .client
        .send_reply("general", "Yes!", Some(&question.id))
        .unwrap();
    assert_eq!(answer.status, MessageStatus::Sent);
    assert_eq!(answer.reply_to.as_deref(), Some(question.id.as_str()));
    assert_eq!(
        h.repo
            .messages("general")
            .last()
            .unwrap()
            .reply_to
            .as_deref(),
        Some(question.id.as_str())
    );
    for (room, target) in [("random", question.id.as_str()), ("general", "nope")] {
        assert!(matches!(
            h.client.send_reply(room, "x", Some(target)),
            Err(ChatError::InvalidInput { .. })
        ));
    }
}

#[test]
fn edits_deletions_and_reactions_reach_the_server_and_the_store() {
    let h = cursor_harness();
    h.client.list_conversations().unwrap();
    let mine = h.client.send_message("general", "Lunch at 12?").unwrap();
    h.drain();

    let edited = h.client.edit_message(&mine.id, "Lunch at 1?").unwrap();
    assert_eq!(edited.body, "Lunch at 1?");
    assert!(edited.edited_at.is_some());
    assert_eq!(updated(&h.drain(), &mine.id).unwrap().body, "Lunch at 1?");
    assert_eq!(
        conversation(&h.client, "general")
            .last_message
            .unwrap()
            .body,
        "Lunch at 1?"
    );

    let reacted = h.client.set_reaction(&mine.id, "👍", true).unwrap();
    assert_eq!(reacted.reactions["👍"], [""]);
    let cleared = h.client.set_reaction(&mine.id, "👍", false).unwrap();
    assert!(cleared.reactions.is_empty());

    let gone = h.client.delete_message(&mine.id).unwrap().unwrap();
    assert!(gone.deleted);
    assert!(gone.body.is_empty());
    assert!(h.client.cached_messages("general")[0].deleted);
    assert!(h.client.search("lunch", None).unwrap().is_empty());

    // A deleted message cannot be changed, and other people's never can.
    assert!(h.client.edit_message(&mine.id, "again").is_err());
    let theirs = h.repo.inject("general", "ana", "mine, not yours");
    h.client.sync_messages("general").unwrap();
    assert!(matches!(
        h.client.edit_message(&theirs.id, "hijacked"),
        Err(ChatError::InvalidInput { .. })
    ));
    assert!(h.client.delete_message(&theirs.id).is_err());
    // Reacting to someone else's message is fine.
    assert!(h.client.set_reaction(&theirs.id, "❤️", true).is_ok());
    assert!(h
        .client
        .set_reaction(&theirs.id, "not one emoji", true)
        .is_err());
}

#[test]
fn changes_by_others_arrive_live_and_in_catch_up_without_counting_unread() {
    let h = cursor_harness();
    h.client.list_conversations().unwrap();
    let message = h.repo.inject("general", "ana", "draft plan");
    h.client.subscribe("general").unwrap();
    h.wait_connected("general");
    h.wait_for(
        "the message",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.id == message.id),
    );

    // Live, on the open room's stream.
    h.repo.inject_edit("general", &message.id, "final plan");
    let events = h.wait_for(
        "the edit",
        |e| matches!(e, ChatEvent::MessageUpdated(m) if m.body == "final plan"),
    );
    assert!(!events.iter().any(|e| matches!(e, ChatEvent::Notify(_))));
    h.client.unsubscribe("general").unwrap();
    h.drain();

    // While nobody watches: picked up by the next catch-up, because the
    // server returns messages *changed* since the cursor.
    h.repo.inject_reaction("general", &message.id, "ben", "🎉");
    h.repo.inject_delete("general", &message.id);
    h.client.sync_messages("general").unwrap();
    let stored = &h.client.cached_messages("general")[0];
    assert!(stored.deleted, "{stored:?}");
    assert_eq!(
        conversation(&h.client, "general").unread_count,
        0,
        "changes are not unread"
    );
    assert!(updated(&h.drain(), &message.id).unwrap().deleted);
}

#[test]
fn deleting_an_unsent_message_removes_it_from_the_outbox() {
    let h = harness();
    h.client.list_conversations().unwrap();
    h.repo.set_offline(true);
    let queued = h.client.send_message("general", "never mind").unwrap();
    assert_eq!(h.client.delete_message(&queued.id).unwrap(), None);
    assert!(h.drain().contains(&ChatEvent::MessageRemoved {
        conversation_id: "general".into(),
        id: queued.id.clone(),
    }));
    assert!(h.client.outbox().is_empty());
    assert!(h.client.cached_messages("general").is_empty());
    h.repo.set_offline(false);
    h.client.flush_outbox();
    std::thread::sleep(Duration::from_millis(50));
    assert!(h.repo.messages("general").is_empty(), "never sent");
}

#[test]
fn drafts_are_kept_until_sent() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Arc::new(InMemoryRepo::with_rooms(&[("general", "General")]));
    {
        let h = harness_with(repo.clone(), ChatStore::in_directory(dir.path()));
        h.client.list_conversations().unwrap();
        h.client.set_draft("general", "Remember to bring").unwrap();
        assert_eq!(
            h.client.draft("general").as_deref(),
            Some("Remember to bring")
        );
    }
    let h = harness_with(repo, ChatStore::in_directory(dir.path()));
    assert_eq!(
        h.client.draft("general").as_deref(),
        Some("Remember to bring"),
        "kept across a relaunch"
    );
    h.client
        .send_message("general", "Remember to bring snacks")
        .unwrap();
    assert_eq!(h.client.draft("general"), None, "sending clears it");
    h.client.set_draft("general", "x").unwrap();
    h.client.set_draft("general", "   ").unwrap();
    assert_eq!(h.client.draft("general"), None);
}

fn file(dir: &std::path::Path, name: &str, bytes: &[u8], mime: &str) -> OutgoingFile {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    OutgoingFile {
        path,
        name: name.into(),
        mime: mime.into(),
    }
}

#[test]
fn files_are_uploaded_before_the_message_and_fetched_by_others() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness();
    h.client.list_conversations().unwrap();
    let photo = file(
        dir.path(),
        "IMG_0001.jpg",
        &[0xff, 0xd8, 1, 2, 3],
        "image/jpeg",
    );
    let doc = file(dir.path(), "Plan.pdf", b"%PDF-1.7 plan", "application/pdf");
    let sent = h
        .client
        .send_with_files("general", " look ", None, &[photo.clone(), doc])
        .unwrap();
    assert_eq!(sent.status, MessageStatus::Sent);
    assert_eq!(sent.body, "look");
    assert_eq!(
        sent.attachments
            .iter()
            .map(|a| (a.name.as_str(), a.mime.as_str(), a.size, a.is_image()))
            .collect::<Vec<_>>(),
        [
            ("IMG_0001.jpg", "image/jpeg", 5, true),
            ("Plan.pdf", "application/pdf", 13, false)
        ]
    );
    // Both files went up before the message.
    let calls: Vec<String> = h
        .repo
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("upload") || c.starts_with("post"))
        .collect();
    assert_eq!(
        calls,
        [
            "upload image/jpeg".to_string(),
            "upload application/pdf".into(),
            format!("post general {}", sent.id)
        ]
    );
    // The sender keeps its own copy, even once the original is gone.
    std::fs::remove_file(&photo.path).unwrap();
    let mine = h.client.attachment_file(&sent.attachments[0].id).unwrap();
    assert_eq!(std::fs::read(mine).unwrap(), [0xff, 0xd8, 1, 2, 3]);

    // Someone else sees the message and fetches the file once.
    let other_dir = tempfile::tempdir().unwrap();
    let other = harness_with(h.repo.clone(), ChatStore::in_directory(other_dir.path()));
    other.client.list_conversations().unwrap();
    let theirs = other.client.sync_messages("general").unwrap();
    let pdf = theirs[0].attachments[1].clone();
    assert_eq!(other.client.attachment_file(&pdf.id), None);
    let path = other.client.fetch_attachment(&pdf.id).unwrap();
    assert!(path.starts_with(other_dir.path().join("attachments")));
    assert_eq!(std::fs::read(&path).unwrap(), b"%PDF-1.7 plan");
    let downloads = || {
        h.repo
            .calls()
            .iter()
            .filter(|c| c.starts_with("download"))
            .count()
    };
    assert_eq!(downloads(), 1);
    assert_eq!(other.client.fetch_attachment(&pdf.id).unwrap(), path);
    assert_eq!(downloads(), 1, "fetched once, then read locally");
    assert!(matches!(
        other.client.fetch_attachment("../forge-chat.db"),
        Err(ChatError::InvalidInput { .. })
    ));

    // Search finds a message by its files' names.
    assert_eq!(other.client.search("plan", None).unwrap()[0].id, sent.id);
    // Deleting it for everyone drops the attachments from the message.
    let tombstone = h.client.delete_message(&sent.id).unwrap().unwrap();
    assert!(tombstone.attachments.is_empty());
    other.client.shutdown();
}

#[test]
fn files_sent_offline_wait_in_the_outbox_with_their_own_copy() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness();
    h.client.list_conversations().unwrap();
    h.repo.set_offline(true);
    let doc = file(dir.path(), "notes.txt", b"offline notes", "text/plain");
    let queued = h
        .client
        .send_with_files("general", "", None, std::slice::from_ref(&doc))
        .unwrap();
    assert_eq!(queued.status, MessageStatus::Sending);
    assert_eq!(h.client.outbox().len(), 1);
    // The picked file may be a temporary copy the system deletes.
    std::fs::remove_file(&doc.path).unwrap();
    h.repo.set_offline(false);
    h.client.flush_outbox();
    h.wait_for("the queued file is sent", |e| {
        *e == status_event(&queued, MessageStatus::Sent)
    });
    let sent = &h.client.cached_messages("general")[0];
    assert_eq!(sent.attachments[0].name, "notes.txt");
    assert!(h.client.outbox().is_empty());
}

#[test]
fn bad_files_are_refused_before_the_network() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness();
    h.client.list_conversations().unwrap();
    let empty = file(dir.path(), "empty.txt", b"", "text/plain");
    let missing = OutgoingFile {
        path: dir.path().join("missing.jpg"),
        name: String::new(),
        mime: String::new(),
    };
    let one = file(dir.path(), "one.txt", b"1", "text/plain");
    let too_many = vec![one; MAX_ATTACHMENTS_PER_MESSAGE + 1];
    for files in [vec![empty], vec![missing], too_many] {
        assert!(matches!(
            h.client.send_with_files("general", "x", None, &files),
            Err(ChatError::InvalidInput { .. })
        ));
    }
    assert!(h.client.cached_messages("general").is_empty());
    assert!(!h.repo.calls().iter().any(|c| c.starts_with("upload")));
}
