# Chat demo: design

The Chat example is the third demo, after Counter (FFI basics) and Weather (real HTTP, nested types, cancelling one request). It proves what those two do not:

| Goal | Why Counter / Weather don't prove it |
|---|---|
| A long-lived, high-frequency event stream | Counter emits rarely; Weather emits on refresh |
| Multi-entity storage (conversations + messages) with a `since` cursor | Counter stores one number; Weather caches one report |
| Cancellation of a long-lived subscription on leave-room | Weather cancels one fetch |
| Typed errors for network, HTTP, protocol and host support | Weather has one error enum for one API |
| The same domain on both hosts | Weather is iOS-only |

It is deliberately **not** a production chat client. Non-goals: real auth, push notifications (the apps show *local* notifications while they run; see [Notifications](#notifications)), media messages, read receipts, typing indicators, encryption. PRs that add them will be declined; if the mock server needs persistence or auth, that is the signal the demo needs a real backend, which is out of scope.

## Status

| Phase | What | State |
|---|---|---|
| 1 | Rust core, no FFI: [`examples/chat-core`](../examples/chat-core) | **Done** (PR #18). `cargo test -p chat-core` |
| 2 | FFI boundary in `forge-ffi` (`ForgeChatApp`), Swift + Kotlin bindings | **Done.** `cargo test -p forge-ffi chat`; bindings regenerated |
| 3 | `scripts/mock-chat-server` (axum, in-memory) + `make mock-chat` | **Done.** End-to-end test: `cargo test -p mock-chat-server` |
| 4 | SwiftUI host, [`examples/chat`](../examples/chat/README.md) | **Done.** `scripts/test-swift-host.sh examples/chat` (macOS; runs against the mock server) |
| 5 | Compose host, [`examples/chat-android`](../examples/chat-android/README.md) (+ the Android TLS decision) | **Done.** `./gradlew :app:testDebugUnitTest` in `examples/chat-android` (runs against the mock server) |
| 6 | CI matrix, README (screenshots from an iPhone, built on Linux), roadmap | **Done** |

## Layout

```
examples/chat-core        # Rust domain: types, ChatClient, ChatRepo (in-memory + HTTP/SSE), SSE parser, JSON store
crates/forge-ffi/src/chat # ForgeChatApp, the UniFFI surface (like src/weather)
scripts/mock-chat-server  # In-memory axum server: `make mock-chat`
examples/chat             # SwiftUI host
examples/chat-android     # Compose host
```

`chat-core` is a workspace member, so `cargo test --workspace` (CI, `make test`, `forge check`) runs its tests.

## The core, in one paragraph

`ChatClient` is the engine the hosts drive. It owns a `ChatRepo` (the server as the client sees it), a `ChatStore` (one JSON snapshot) and one `EventSink`. Commands are blocking and run on the caller's thread, like `forge_net::HttpClient`; the FFI layer runs each on a worker thread, as Weather does. `subscribe` starts one pump thread per room that connects the room's Server-Sent Events stream, catches up with a `since` fetch, follows the stream, and reconnects with backoff when it drops.

### Types

```rust
pub struct Conversation { id, title, last_message_at: Option<i64>, unread_count: u32 }
pub struct Message { id, conversation_id, author, body, sent_at: i64, status: MessageStatus }
pub enum MessageStatus { Sending, Sent, Failed }

pub enum ChatEvent {
    ConversationUpdated(Conversation),
    MessageReceived(Message),
    Notify(Message),
    MessageStatusChanged { id, status },
    StreamConnected { conversation_id },
    StreamDisconnected { conversation_id, reason, will_retry: bool },
}

pub enum ChatError {
    Network { reason }, Http { status, reason }, Protocol { reason }, Cancelled,
    NotFound { conversation_id }, UnsupportedOnHost { detail },
    InvalidInput { reason }, ShutDown,
}
```

Times are Unix **milliseconds**. Error fields are named `reason`/`detail`, never `message`, which clashes with `Throwable.message` in Kotlin (the Weather lesson).

### Commands

| Command | Behaviour |
|---|---|
| `list_conversations()` | Lists rooms. For each room that is not open and has newer messages than the cursor, fetches them too: they count towards `unread_count` and are announced with `MessageReceived`. |
| `load_messages(id, since)` / `sync_messages(id)` | Fetches (all, or newer than `since`), merges, returns the room's full stored list including local `Sending`/`Failed` messages. `sync_messages` uses the stored cursor. |
| `send_message(id, body)` | Stores and announces the message as `Sending` *before* the network call, then `MessageStatusChanged` to `Sent` or `Failed`. Returns the final message or the error. |
| `retry_message(message_id)` | `Failed` → `Sending` → `Sent`/`Failed`, same id. Retrying a `Sent` message returns it. |
| `subscribe(id)` | Opens the room: starts its pump (or reuses its watch), marks it read and keeps it read. Returns at once; connection state arrives as events. Idempotent. |
| `unsubscribe(id)` | Closes the room. The pump ends unless the room is watched. Idempotent, allowed after shutdown. |
| `watch(id)` / `unwatch(id)` | Keeps the room's pump running in the background without opening it, so its news counts as unread and raises `Notify`. The pump ends when the room is neither open nor watched. Idempotent. |
| `shutdown()` | Ends every subscription; later commands fail with `ShutDown`. |
| `cached_conversations()`, `cached_messages(id)`, `last_synced(id)` | Local reads for a first render before the network. |

### The event contract

These are the guarantees the FFI layer and both hosts build on. Each has a test in `examples/chat-core/src/client/tests.rs`.

1. **One sink, filtered by the host.** All events of a `ChatClient` go through one sink; hosts filter by `conversation_id` (for `MessageStatusChanged`, by message id).
2. **Ordered, and after the store.** Every event is emitted under the client's lock, right after the store change it describes. Re-reading the store after an event never shows older data.
3. **Every new message is announced exactly once.** Stream, catch-up fetch, list prefetch and our own send all merge by message id. The stream echo of our own message turns the local bubble `Sent` instead of adding a second one. A message evicted from the store (see below) is not announced again if it is re-delivered.
4. **Unsubscribe is a hard stop for events.** When `unsubscribe` returns, `StreamDisconnected { will_retry: false }` has been emitted and no later event comes from that subscription: each pump checks it is still the current subscription, under the same lock, before every change.
5. **And the Rust task really ends.** The pump thread exits at its next wake-up (next stream event or heartbeat, or a 10 ms backoff slice) and drops its connection. A blocked read cannot be interrupted (ureq has no cancel), which is why the server sends heartbeats. `ChatClient::live_pumps()` counts running pumps; the tests assert it reaches zero and that the in-memory server sees the stream closed.
6. **Leaving a room does not cancel a send.** The send finishes and its status event is still emitted; the list screen can ignore it, the store has it.
7. **One `StreamDisconnected` per outage.** With `will_retry: true` the pump reconnects with backoff (500 ms doubling to 5 s by default) and emits `StreamConnected` when it succeeds. With `will_retry: false` the subscription is over: unsubscribed, shut down, or the room does not exist.
8. **No gaps after reconnect.** The pump opens the stream first and catches up second, so a message stored in between arrives on one or both (and is merged).

### Why explicit subscribe/unsubscribe

Tying the subscription to the lifetime of an `AsyncSequence` / `Flow` would make cancellation depend on how each binding generator drops a future, and the async bridge milestone already found that UniFFI's Swift bindings do not cancel Rust futures when a `Task` is cancelled. Explicit commands make the contract visible and testable in Rust. The hosts still get an idiomatic stream: the event sink feeds a pull-based subscription like Counter's (`next()` async, bounded queue), which the Swift bridge wraps in an `AsyncSequence` and the Kotlin host in a `Flow`.

### The `since` cursor

`last_synced[room]` is the greatest server `sent_at` such that every message up to it is in the store. The server keeps `sent_at` strictly increasing within a room, so `since` is exclusive and exact. The cursor only moves when no gap can be left behind:

- a fetch moves it only if its `since` was at or before the cursor (or it was a full fetch);
- a stream message moves it (the stream has been open since the last catch-up);
- **our own send does not**: messages from others may sit between the cursor and ours.

### Unread counts

Tracked on the device: messages from other people that arrive while a room is not open. `list_conversations` notices rooms with news (server `last_message_at` > cursor) and fetches only those. `subscribe` resets the count. No server state, no read receipts.

### Notifications

Local only, while the app runs: the Rust client holds the room connections, and there is no push server (push stays a non-goal). A host that wants notifications **watches** every room. Rust then emits `Notify(message)`, right after the message's `MessageReceived`, when all of these hold:

- the message is new to the store and from someone else;
- its room is not open (it arrived on a watched room's stream, or in a refresh's prefetch);
- it was sent after this `ChatClient` started (server `sent_at` against device time), so the history a room catches up on when first watched, and whatever arrived while the app was closed, never notify. It still counts as unread.

The hosts only display it: `UNUserNotificationCenter` on iOS (with banners in the foreground too), a high-importance `NotificationChannel` on Android (`POST_NOTIFICATIONS` is asked for on Android 13+). Both sessions take the notifier as an optional parameter; without one, nothing is watched. Watching every room means one open stream per room. A `Notify` lost to `EventsDropped` is not replayed: the unread count still shows it.

### Store

One JSON file, `forge-chat-store.json`, rewritten atomically (`forge_storage::write_atomic`) after every change:

```json
{
  "schema_version": 1,
  "conversations": [ { "id": "general", "title": "General", "last_message_at": 1730000000000, "unread_count": 0 } ],
  "messages": { "general": [ { "id": "m-…", "conversation_id": "general", "author": "me", "body": "hi", "sent_at": 1730000000000, "status": "sent" } ] },
  "last_synced": { "general": 1730000000000 }
}
```

- `store::migrate(serde_json::Value)` is where versions are upgraded (only v1 exists).
- A store from a **newer** build makes `ChatClient::open` fail and is never overwritten. A **corrupt** store is set aside: the client starts empty (the server is the source of truth) and `store_error()` says why.
- Each room keeps at most 500 messages. Only `Sent` messages are evicted: unsent ones are the user's.
- Messages left `Sending` by a killed app load as `Failed`, so the user can retry them.
- Keep it dumb: the whole file is rewritten (with fsync) per change. That is fine for a demo; a real client would batch writes. If the high-frequency demo shows it, batch in the FFI layer rather than making the store clever.

## Wire protocol (what the mock server must serve)

| Call | Request | Success |
|---|---|---|
| list | `GET /rooms` | `200` `[{"id","title","last_message_at"}]` |
| load | `GET /rooms/{id}/messages[?since=ms]` | `200` `[Message]`, oldest first, `sent_at > since` |
| post | `POST /rooms/{id}/messages` `{"id","author","body"}` | `200`/`201` `Message`; **idempotent by `id`** |
| stream | `GET /rooms/{id}/stream` | `200` `text/event-stream` |

`Message` is `{"id","conversation_id","author","body","sent_at"}`. The stream sends `event: message` (a `Message`) and `event: room` (a room) with JSON `data`, plus `: ping` comment lines every 15 s as heartbeats. A `404` on a room path means the room does not exist. Unknown fields and event names are ignored. `InMemoryRepo` is the executable reference: the mock server must behave like it (strictly increasing `sent_at` per room, idempotent post, broadcast to the room's streams).

`HttpRepo` implements this over `forge_net::HttpClient`. Phase 1 added `HttpClient::open_stream` (a response whose body is read incrementally) to forge-net, with a real implementation in `UreqClient` (timeout covers connect + headers only) and a buffering default for test doubles, so `StaticClient` can serve canned SSE bodies.

## The mock server (Phase 3)

```sh
make mock-chat                                 # http://127.0.0.1:8787, rooms general, random, forge
make mock-chat ARGS="--bind 0.0.0.0:8787"      # reachable from a phone on the same network
make mock-chat ARGS="--heartbeat-ms 1000"      # faster heartbeats (default 15 s)
```

Try it with curl:

```sh
curl localhost:8787/rooms
curl -N localhost:8787/rooms/general/stream          # leave running: events appear here
curl -X POST localhost:8787/rooms/general/messages \
  -H 'Content-Type: application/json' -d '{"author":"curl","body":"hi"}'
curl 'localhost:8787/rooms/general/messages?since=0'
```

`id` is optional on a POST from curl (the server assigns `srv-N`); clients always send their own. It answers `201` for a new message, `200` with the stored message when the id already exists, `400` for an empty author or body, and `404` for an unknown room. A stream subscriber that falls more than 1024 events behind is disconnected, and the client reconnects and catches up. Ctrl-C stops the server immediately, open streams included (no graceful shutdown, because killing the server is part of the demo). State is in memory only, so a restart starts empty.

`scripts/mock-chat-server/tests/end_to_end.rs` runs chat-core's real HTTP + SSE client (`HttpRepo` over `UreqClient`) against the server over sockets: rooms, idempotent post, `since`, `404`s, and the six-step demo below with a real kill and restart on the same port.

## The demo interaction

This goes in the README with a GIF once the hosts exist. `killer_demo_interaction` in the chat-core tests runs the same script against the in-memory repo, and `killer_demo_interaction_over_real_sockets` in the mock server tests runs it over HTTP.

1. Open a chat, type a message, tap send. It goes `Sending` → `Sent`.
2. Navigate back to the list. Nothing is cancelled: a message in flight still completes.
3. Kill the mock server. The open chat shows "reconnecting" (`StreamDisconnected { will_retry: true }`).
4. Send again. `ChatError::Network` comes back and the bubble turns red (`MessageStatusChanged { status: Failed }`).
5. Restart the server, tap the failed message to retry: `Sending` → `Sent`. The stream reconnects on its own and catches up.
6. While in the chat, post a message with curl. It appears within a second via `MessageReceived`:

```sh
curl -X POST localhost:8787/rooms/general/messages \
  -H 'Content-Type: application/json' \
  -d '{"id":"curl-1","author":"curl","body":"hello from curl"}'
```

That sequence shows typed errors, status transitions via events, external event delivery, reconnect with catch-up, and idempotent retry in one flow.

## Changes from the original plan, and why

The plan was written before the Weather example landed. Checked against the repo:

| Plan | Now | Why |
|---|---|---|
| `forge-net` does the HTTP | Added `HttpClient::open_stream` to forge-net | `HttpClient::send` buffers the whole body (with an 8 MiB cap), so it cannot carry an endless SSE stream. |
| `async fn` commands, `chat_events() -> AsyncSequence<ChatEvent>` | Blocking core; async + `AsyncSequence` at the FFI layer | Matches forge-ffi: UniFFI built-in async without tokio, worker thread per operation (Weather), pull-based event subscription (Counter). UniFFI has no `AsyncSequence` type; the Swift bridge builds one from `next()`. |
| One *global* event stream | One stream per `ChatClient` (per app instance) | Same as Counter's `subscribe()`; still one stream for all rooms, filtered by the host. |
| `forge-storage` JSON snapshots | `forge_storage::write_atomic` + chat's own snapshot type | `JsonSnapshotStore` is hard-wired to the Counter's envelope; only its atomic write is reusable. |
| `"version": 1` | `"schema_version": 1` | The key every other Forge store uses. |
| Seconds (`1730000000`) | Milliseconds | Second resolution makes `since` lossy within a busy second. |
| Command list without retry | Added `retry_message` | Step 5 of the demo needs it. Client-chosen message ids make it idempotent. |
| `Cancelled` "if the user navigates away mid-send" | Navigating away never cancels a send; `Cancelled` is what a send reports when the app shuts down while it is in flight | Step 2 of the demo says leaving cancels nothing; both cannot hold. Not cancelling is the better UX (the message is already on the wire). |
| `StreamDisconnected { conversation_id, reason }` | Added `will_retry` | The UI needs to tell "reconnecting" from "gone". |
| `ChatError` as listed | Added `InvalidInput` and `ShutDown` | Same grouping as `WeatherError` / `ForgeAppError` (input, lifecycle). |
| Phase 2: `chat-ffi` crate or a module | A module, `forge-ffi/src/chat` | One `forge_ffi` library exports every example's API and every `Generated/` copy is identical (see [ffi-policy.md](ffi-policy.md)); a second UniFFI cdylib would mean a second binding set and a second native library per app. |
| `examples/chat-core` | As planned | It is a workspace member like the crates under `crates/`. |

## The FFI surface (Phase 2)

`crates/forge-ffi/src/chat` exposes `ForgeChatApp`. The types mirror the core with a `Chat` prefix (`ChatConversation`, `ChatMessage`, `ChatMessageStatus`), because every example shares one generated module. The contract is in [ffi-policy.md](ffi-policy.md#long-lived-event-streams-chat).

| Rust export | Notes |
|---|---|
| `ForgeChatApp::new(storage_directory, ChatAppConfig { base_url, user, request_timeout_ms })` | Validates the URL and timeout. `""` keeps the store in memory. A store from a newer build fails with `Storage`. |
| `events()` → `ChatEventSubscription` (`async next()`, `cancel()`) | One per consumer, queue of 256, then `EventsDropped { count }`: re-read `cached_*`. |
| `async list_conversations()`, `async load_messages(id, since)`, `async sync_messages(id)` | Each runs on a worker thread. |
| `async send_message(id, body)`, `async retry_message(message_id)` | Not cancellable from the host, on purpose. |
| `subscribe(id)`, `unsubscribe(id)`, `watch(id)`, `unwatch(id)` | Synchronous, return at once. |
| `cached_conversations()`, `cached_messages(id)`, `last_synced(id)`, `is_subscribed(id)`, `is_watched(id)` | Local reads. |
| `live_streams()`, `store_problem()` | Diagnostics: room threads still running, last persistence problem. |
| `shutdown()`, `is_shut_down()` | Closes rooms, then event streams (queued events delivered first). |

`ChatEvent` variants carry named fields (`MessageReceived { message }`, `Notify { message }`, `ConversationUpdated { conversation }`) and add `EventsDropped { count }`. `ChatError` adds `Storage { reason }` and `Internal { reason }`, and calls the core's `Protocol` error `BadResponse` (`.Protocol` is a metatype in Swift). Swift and Kotlin compile is checked by CI (the Swift host tests and the Android APK build compile the regenerated bindings); cloud sessions have no Swift or Kotlin compiler.

## Android and TLS (Phase 5)

Two separate problems, and the demo as planned only hits one of them:

1. **Cleartext HTTP: not a problem for Rust.** Android 9+ refuses cleartext by default, but that policy is enforced by the Java HTTP stacks (`HttpURLConnection`, OkHttp); the platform does not apply it to plain sockets. The Rust client opens its own sockets, so `http://10.0.2.2:8787` (the emulator's view of the host) works with only the `INTERNET` permission, and the example has no `network_security_config`.
2. **The TLS verifier: refused, as a typed error (decided in Phase 5).** `UreqClient` uses `rustls-platform-verifier`, which on Android needs a JNI init call (`rustls_platform_verifier::android::init_hosted` with the app `Context`) and its companion Kotlin library. Plain HTTP never touches it, so the demo does not need it, and wiring it would add a JNI entry point and a Gradle dependency that nothing in the demo exercises. So `ForgeChatApp::new` refuses an `https://` base URL on Android builds with `ChatError::UnsupportedOnHost` (`check_host_supports` in `crates/forge-ffi/src/chat/app.rs`), and the app shows "Android supports http:// chat servers only". Never a silent TLS failure. Wiring the verifier is a follow-up that also unblocks Weather on Android.

## Risks

| Risk | Mitigation |
|---|---|
| Android TLS verifier not wired | Decided in Phase 5: `https` on Android fails at once with `UnsupportedOnHost` (see above). |
| Cancellation semantics differ between Swift and Kotlin | Explicit subscribe/unsubscribe, tested in Rust (`live_pumps()` reaches zero, the server sees the stream closed). |
| Event queue overflow under load | `EventsDropped` + re-read from the store; tested with a 512-message burst. |
| A blocked stream read outlives unsubscribe | Heartbeats from the server; no events are emitted after unsubscribe regardless. |
| Store rewrite per message is slow on a phone | Measured in Phase 4/5; batch writes in the FFI layer if needed. |
| Scope creep (media, auth, receipts) | Non-goals above. |
| Mock server grows into a project | Single file, in-memory, no persistence. |
