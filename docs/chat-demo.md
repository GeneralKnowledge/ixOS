# Chat demo: design

The Chat example is the third demo, after Counter (FFI basics) and Weather (real HTTP, nested types, cancelling one request). It proves what those two do not:

| Goal | Why Counter / Weather don't prove it |
|---|---|
| A long-lived, high-frequency event stream | Counter emits rarely; Weather emits on refresh |
| Multi-entity storage (conversations + messages) with a `since` cursor | Counter stores one number; Weather caches one report |
| Cancellation of a long-lived subscription on leave-room | Weather cancels one fetch |
| Typed errors for network, HTTP, protocol and host support | Weather has one error enum for one API |
| The same domain on both hosts | Weather is iOS-only |

**Since 2026-11-07 Chat is the flagship ixOS acceptance app.** It grows milestone by milestone into a real iPhone messaging app, and every feature it gains has to work for real and exercise a real ixOS capability (accounts in the Keychain, the lifecycle, attachments, voice, notifications). The milestones:

| Milestone | What | State |
|---|---|---|
| M0 | Chat on the current framework: its own Rust crate, `@Observable` SwiftUI, lifecycle, a real server with accounts and storage on disk, the account in the Keychain, `chat-cli` | Done (PR #40); passed on an iPhone (PR #42) |
| M1 | Native messaging UI: Chats and Settings tabs, previews, server read cursors (unread and "Read"), mark read/unread and mute, new conversations, grouped bubbles with time separators, a pinned composer, haptics | Done (PR #46); device run E1–E13 passed on an iPhone 15 Pro Max, see [examples/chat/ACCEPTANCE.md](../examples/chat/ACCEPTANCE.md) |
| M2 | Edits, deletes, replies, reactions; drafts; a durable outbox; search; storage on the reusable forge-storage database (SQLite spike first) | Built (PR #51); iPhone run pending, see [examples/chat/ACCEPTANCE.md](../examples/chat/ACCEPTANCE.md#m2-device-run) |
| M3 | Photos and documents | Planned |
| M4 | Voice messages | Planned |
| M5 | Notifications through forge-services, deep links, background work | Planned |
| M6 | Physical iPhone acceptance and framework gap report | Planned |

Still out of scope: remote push (APNs). Without it, new-message alerts appear only while the app runs, and the app never promises alerts while it is suspended or closed. Also out: end-to-end encryption, and a hosted server (the chat server runs on your own computer).

The Android client ([examples/chat-android](../examples/chat-android/README.md)) is **frozen** at its phase-5 features while the iOS app grows: it keeps building, and its tests run against the new server in anonymous mode.

## Status

| Phase | What | State |
|---|---|---|
| 1 | Rust core, no FFI: [`examples/chat-core`](../examples/chat-core) | **Done** (PR #18). `cargo test -p chat-core` |
| 2 | FFI boundary (`ForgeChatApp`, now in `examples/chat-android/rust`), Swift + Kotlin bindings | **Done.** `cargo test --manifest-path examples/chat-android/rust/Cargo.toml`; bindings regenerated |
| 3 | `scripts/mock-chat-server` (axum, in-memory) + `make mock-chat`; since M0 `examples/chat-server` | **Done.** End-to-end test: `cargo test -p chat-server` |
| 4 | SwiftUI host, [`examples/chat`](../examples/chat/README.md) | **Done.** `scripts/test-swift-host.sh examples/chat` (macOS; runs against the mock server) |
| 5 | Compose host, [`examples/chat-android`](../examples/chat-android/README.md) (+ the Android TLS decision) | **Done.** `./gradlew :app:testDebugUnitTest` in `examples/chat-android` (runs against the mock server) |
| 6 | CI matrix, README (screenshots from an iPhone, built on Linux), roadmap | **Done** |

The phases above built the original FFI stress test. The flagship milestones (M0 onwards) are in the table at the top.

## Layout

```
examples/chat-core        # Rust domain: types, ChatClient, ChatRepo (in-memory + HTTP/SSE), SSE parser, JSON store
examples/chat/rust        # The iOS app's own crate (ChatCore): account + Keychain, lifecycle, the UniFFI API
examples/chat-server      # The chat server: accounts, rooms, messages on disk (`make chat-server`)
examples/chat-cli         # Terminal client on chat-core: be the other person (`make chat-cli`)
examples/chat             # SwiftUI host
examples/chat-android/rust/src/chat # ForgeChatApp, the frozen Android example's UniFFI surface
examples/chat-android     # Compose host (frozen)
```

`chat-core`, `chat-server` and `chat-cli` are workspace members, so `cargo test --workspace` (CI, `make test`, `forge check`) runs their tests. `examples/chat/rust` has its own Cargo workspace, like every app from `forge init`; `scripts/check-app-crates.sh` checks it.

## The core, in one paragraph

`ChatClient` is the engine the hosts drive. It owns a `ChatRepo` (the server as the client sees it), a `ChatStore` (a SQLite database since M2) and one `EventSink`. Commands are blocking and run on the caller's thread, like `forge_net::HttpClient`; the FFI layer runs each on a worker thread, as Weather does. `subscribe` starts one pump thread per room that connects the room's Server-Sent Events stream, catches up with a `since` fetch, follows the stream, and reconnects with backoff when it drops.

### Types

```rust
pub struct Conversation { id, title, last_message_at: Option<i64>, unread_count: u32 }
pub struct Message {
    id, conversation_id, author, author_id, body, sent_at: i64, status: MessageStatus,
    // M2
    updated_at: Option<i64>, reply_to: Option<String>, edited_at: Option<i64>,
    deleted: bool, reactions: BTreeMap<String, Vec<String>>, // emoji -> user ids
}
pub enum MessageStatus { Sending, Sent, Failed }

pub enum ChatEvent {
    ConversationUpdated(Conversation),
    MessageReceived(Message),
    MessageUpdated(Message),  // M2: edited, deleted or reacted to; never unread
    MessageRemoved { conversation_id, id }, // M2: an unsent message deleted here
    Notify(Message),
    MessageStatusChanged { conversation_id, id, status },
    StreamConnected { conversation_id },
    StreamDisconnected { conversation_id, reason, will_retry: bool },
}

pub enum ChatError {
    Network { reason }, Http { status, reason }, Protocol { reason }, Cancelled,
    NotFound { conversation_id }, UnsupportedOnHost { detail }, Certificate { reason },
    InvalidInput { reason }, ShutDown,
}
```

Times are Unix **milliseconds**. Error fields are named `reason`/`detail`, never `message`, which clashes with `Throwable.message` in Kotlin (the Weather lesson).

### Commands

| Command | Behaviour |
|---|---|
| `list_conversations()` | Lists rooms. For each room that is not open and has newer messages than the cursor, fetches them too: they count towards `unread_count` and are announced with `MessageReceived`. |
| `load_messages(id, since)` / `sync_messages(id)` | Fetches (all, or newer than `since`), merges, returns the room's full stored list including local `Sending`/`Failed` messages. `sync_messages` uses the stored cursor. |
| `send_message(id, body)` / `send_reply(id, body, reply_to)` | Stores and announces the message as `Sending` *before* the network call, then `MessageStatusChanged` to `Sent`. Since M2 a network error or a 5xx leaves it `Sending` in the **outbox** and returns it (see below); only a refusal (4xx) makes it `Failed` and returns the error. Clears the room's draft. |
| `retry_message(message_id)` | `Failed` (or queued) → `Sending` → `Sent`/`Failed`, same id. Retrying a `Sent` message returns it. |
| `edit_message(message_id, body)`, `delete_message(message_id)`, `set_reaction(message_id, emoji, on)` | M2. Change a sent message on the server and announce the result as `MessageUpdated`. Only the author edits or deletes; deleting leaves a tombstone (`deleted`, empty body). Deleting a message that never reached the server removes it here (`MessageRemoved`) and it is never sent. |
| `set_draft(id, text)`, `draft(id)` | M2. The composer's unsent text per room, kept on this device across launches. |
| `search(query, room?)` | M2. Stored messages containing every word of `query` as a word prefix (case and accents ignored), newest first, at most 100; deleted messages are not found. Runs on the device's SQLite FTS5 index. |
| `outbox()`, `flush_outbox()` | M2. Messages waiting to be sent, and a nudge to send them now. |
| `subscribe(id)` | Opens the room: starts its pump (or reuses its watch), marks it read and keeps it read. Returns at once; connection state arrives as events. Idempotent. |
| `unsubscribe(id)` | Closes the room. The pump ends unless the room is watched. Idempotent, allowed after shutdown. |
| `watch(id)` / `unwatch(id)` | Keeps the room's pump running in the background without opening it, so its news counts as unread and raises `Notify`. The pump ends when the room is neither open nor watched. Idempotent. |
| `shutdown()` | Ends every subscription; later commands fail with `ShutDown`. |
| `cached_conversations()`, `cached_messages(id)`, `last_synced(id)` | Local reads for a first render before the network. |

### The event contract

These are the guarantees the FFI layer and both hosts build on. Each has a test in `examples/chat-core/src/client/tests.rs`.

1. **One sink, filtered by the host.** All events of a `ChatClient` go through one sink; hosts filter by `conversation_id`, which every event except the FFI's `EventsDropped` carries.
2. **Ordered, and after the store.** Every event is emitted under the client's lock, right after the store change it describes. Re-reading the store after an event never shows older data.
3. **Every new message is announced exactly once.** Stream, catch-up fetch, list prefetch and our own send all merge by message id. The stream echo of our own message turns the local bubble `Sent` instead of adding a second one. A message evicted from the store (see below) is not announced again if it is re-delivered.
4. **Unsubscribe is a hard stop for events.** When `unsubscribe` returns, `StreamDisconnected { will_retry: false }` has been emitted and no later event comes from that subscription: each pump checks it is still the current subscription, under the same lock, before every change.
5. **And the Rust task really ends.** Each subscription owns a `forge_net::CancelHandle`. Unsubscribing cancels it, which shuts down that room's socket (a read blocked on a silent stream fails at once), aborts a connect in progress and wakes a reconnect backoff, so the pump exits promptly without waiting for a heartbeat. Other rooms' connections are untouched. `ChatClient::live_pumps()` counts running pumps; the tests assert it reaches zero and that the in-memory server sees the stream closed.
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

### Unread counts and read receipts

Since M1 the server keeps a **read cursor** per account and room: every message up to that `sent_at` is read. `list_conversations` returns the caller's cursor (`read_up_to`; an account that never marked a room has read everything before it joined) and the furthest any *other* account has read (`others_read_up_to`). Then:

- `unread_count` is the stored messages from others after `read_up_to`, and 0 while the room is open;
- opening a room (`subscribe`) marks it read here and, on a background thread, on the server; while it is open, the pump marks each arrival read in the same step that stores it, then tells the server;
- posting a message moves its author's cursor to it (writing is reading);
- one of our sent messages is **read** once `others_read_up_to` reaches its `sent_at` (`Conversation::is_read_by_others`; the app crate's `chat_delivery` turns that into `Sending`/`Sent`/`Read`/`Failed`). The stream's `read` events carry cursor moves, so "Read" appears live;
- `mark_read`, `set_marked_unread` and `set_muted` are the list's swipes. Marked-unread and mute are this device's own (in the store); a muted room still counts unread but never raises `Notify`. Reading the room anywhere (another device, `chat-cli read`) clears marked-unread.

With a server that keeps no cursors (`read_up_to` is `None`), the count is kept on the device as before M1: messages from others that arrive while the room is not open. `list_conversations` notices rooms with news (server `last_message_at` > cursor) and fetches only those.

### Notifications

Local only, while the app runs: the Rust client holds the room connections, and there is no push server (push stays a non-goal). A host that wants notifications **watches** every room. Rust then emits `Notify(message)`, right after the message's `MessageReceived`, when all of these hold:

- the message is new to the store and from someone else;
- its room is not open (it arrived on a watched room's stream, or in a refresh's prefetch);
- it was sent after this `ChatClient` started (server `sent_at` against device time), so the history a room catches up on when first watched, and whatever arrived while the app was closed, never notify. It still counts as unread.

The hosts only display it: `UNUserNotificationCenter` on iOS (with banners in the foreground too), a high-importance `NotificationChannel` on Android (`POST_NOTIFICATIONS` is asked for on Android 13+). Both sessions take the notifier as an optional parameter; without one, nothing is watched. Watching every room means one open stream per room. A `Notify` lost to `EventsDropped` is not replayed: the unread count still shows it.

### Outbox (M2)

A message that cannot reach the server (a network error or a 5xx) stays `Sending` in the store: that is the outbox. A thread (`forge-chat-outbox`) sends queued messages oldest first, with the same id (posts are idempotent), backing off from 1 s to 60 s. It is woken at once when a room's stream connects, when the app resumes, after a successful refresh and by `flush_outbox`. Messages still `Sending` when the app is killed stay queued and go out on the next launch. A refusal by the server (4xx) makes the message `Failed`; the user retries or deletes it. A message is never posted twice at the same time (`in_flight`).

### Message changes (M2)

Edits, deletions and reactions are server events: the server stamps the message with `updated_at` from the room's clock (strictly increasing, shared with `sent_at`) and broadcasts it as a `message` event. `?since=` returns messages *changed* after the cursor, so catch-up after a reconnect brings changes too, and the cursor follows `changed_at() = max(sent_at, updated_at)`. A changed message that is already stored is announced as `MessageUpdated`, never counted unread and never notified. Rooms that are neither open nor watched see changes on their next catch-up (the list prefetch only fetches rooms with new messages).

### Store

**Since M2** the store is a SQLite database, `forge-chat.db`, opened through `forge_storage::Database` (feature `sqlite`: bundled SQLite, WAL mode, migrations by `user_version`). Tables: `conversations`, `messages` (each row the message as JSON plus indexed columns), `cursors`, `drafts`, and `messages_fts`, an FTS5 index kept in step by triggers. Saves are diffs in one transaction, not a rewrite.

- A database from a **newer** build makes `ChatClient::open` fail and is never touched. One that cannot be opened or fails its integrity check is moved aside to `forge-chat.db.corrupt-<ms>`: the client starts empty (the server is the source of truth) and `store_error()` says why.
- The first launch after M2 imports the M1 JSON store (below) once and renames it to `forge-chat-store.json.imported`. An unreadable JSON store is reported like a corrupt database.
- Each room keeps at most 500 messages; only `Sent` messages are evicted.
- Settings shows the engine and version (`storage_engine()`, e.g. "SQLite 3.50.4"), which is the device check that SQLite runs on the iPhone.

**Before M2** it was one JSON file, `forge-chat-store.json`, rewritten atomically (`forge_storage::write_atomic`) after every change:, `forge-chat-store.json`, rewritten atomically (`forge_storage::write_atomic`) after every change:

```json
{
  "schema_version": 1,
  "conversations": [ { "id": "general", "title": "General", "last_message_at": 1730000000000, "unread_count": 0 } ],
  "messages": { "general": [ { "id": "m-…", "conversation_id": "general", "author": "me", "body": "hi", "sent_at": 1730000000000, "status": "sent" } ] },
  "last_synced": { "general": 1730000000000 }
}
```

In M1, messages left `Sending` by a killed app loaded as `Failed`; since M2 they stay queued in the outbox.

## Wire protocol (what the chat server serves)

| Call | Request | Success |
|---|---|---|
| register | `POST /accounts` `{"display_name"}` | `201` `{"user_id","display_name","token"}` |
| who am I | `GET /me` | `200` `{"user_id","display_name"}` |
| list | `GET /rooms` | `200` `[Room]`, with the caller's read state |
| create | `POST /rooms` `{"title"}` | `201` `Room` (needs an account; titles up to 60 characters) |
| mark read | `POST /rooms/{id}/read` `{"up_to"}` | `200` `{"up_to"}`; the cursor only moves forward, and never past the newest message |
| load | `GET /rooms/{id}/messages[?since=ms]` | `200` `[Message]`, oldest first; with `since`, the messages sent **or changed** after it |
| post | `POST /rooms/{id}/messages` `{"id","author","body","reply_to"?}` | `200`/`201` `Message`; **idempotent by `id`**; `reply_to` must be a message in the room (`400`) |
| edit (M2) | `PATCH /rooms/{id}/messages/{mid}` `{"body"}` | `200` `Message` with `edited_at`; author only (`403`), not after deletion (`409`) |
| delete (M2) | `DELETE /rooms/{id}/messages/{mid}` | `200` the tombstone (`deleted: true`, empty body, no reactions); author only (`403`) |
| react (M2) | `POST /rooms/{id}/messages/{mid}/reactions` `{"emoji","on"}` | `200` `Message`; needs an account; one emoji (≤ 8 characters), ≤ 20 different per message; `409` on a deleted message |
| stream | `GET /rooms/{id}/stream` | `200` `text/event-stream` |

Every call but `register` carries `Authorization: Bearer <token>`. The server takes a message's author from the token (`author` and `author_id`), whatever the body says; an unknown or missing token is `401`, which the client reports as `ChatError::Unauthorized`. Only a server started with `--allow-anonymous` also accepts calls without a token, with the author from the body: that is how the frozen Android example and curl post.

`Message` is `{"id","conversation_id","author","author_id","body","sent_at"}` (`author_id` is empty for anonymous posts, and may be missing from older servers), plus since M2 the optional `updated_at`, `reply_to`, `edited_at`, `deleted` and `reactions` (`{"👍": ["user-id", …]}`), each left out when empty. Edits, deletions and reactions are broadcast as `message` events with the changed message. `Room` is `{"id","title","last_message_at","last_message","read_up_to","others_read_up_to"}`; the two read fields are only in answers to the caller (a room in the stream goes to everyone). The stream sends `event: message` (a `Message`), `event: room` (a room) and `event: read` (`{"conversation_id","user_id","up_to"}`) with JSON `data`, plus `: ping` comment lines every 15 s as heartbeats. A `404` on a room path means the room does not exist. Unknown fields and event names are ignored. `InMemoryRepo` is the executable reference: the chat server must behave like it (strictly increasing `sent_at` per room, idempotent post, broadcast to the room's streams).

`HttpRepo` implements this over `forge_net::HttpClient`. Phase 1 added `HttpClient::open_stream` (a response whose body is read incrementally) to forge-net, with a real implementation in `UreqClient` (timeout covers connect + headers only) and a buffering default for test doubles, so `StaticClient` can serve canned SSE bodies.

## The chat server

Started as an in-memory mock (phase 3); since M0 it is a small real server you run on your own computer:

```sh
make chat-server                                 # http://127.0.0.1:8787, data in ~/.local/share/ixos-chat/server.json
make chat-server ARGS="--bind 0.0.0.0:8787"      # reachable from a phone on the same network
make chat-server ARGS="--data /tmp/chat.json"    # somewhere else
make chat-server ARGS="--in-memory --allow-anonymous"   # the old mock: nothing on disk, no accounts needed
```

- **Accounts.** `POST /accounts` creates one and returns a random 256-bit token. There is no password and no recovery: the token is the account. The app keeps it in the Keychain; `chat-cli` keeps its own in `~/.config/ixos-chat/cli-accounts.json` (owner-only).
- **Storage.** One JSON file, rewritten atomically after every change and saved *before* clients hear about the change, so a message a phone has seen survives a server restart. The file holds the tokens, so it is created owner-readable only. A corrupt file, or one from a newer server, is refused, not overwritten.
- **Limits.** For one person's devices on a home network: plain HTTP (put it behind a TLS proxy for anything else), no rate limiting, at most 200 conversations, and other devices see a new conversation on their next refresh (pull to refresh, or coming back to the app), not live.
- A stream subscriber that falls more than 1024 events behind is disconnected, and the client reconnects and catches up. Ctrl-C stops the server immediately (every change is already on disk).

`chat-cli` is the other person in a conversation, from a Linux terminal:

```sh
make chat-cli ARGS='signup "Reuben (laptop)"'
make chat-cli ARGS="rooms"
make chat-cli ARGS="send general Hello from the terminal"
make chat-cli ARGS="follow general"             # live, with chat-core's own reconnect logic
```

`--server URL` (or `CHAT_SERVER`) points it at another server. Curl still works against a server started with `--allow-anonymous`:

```sh
curl -X POST localhost:8787/rooms/general/messages \
  -H 'Content-Type: application/json' -d '{"author":"curl","body":"hi"}'
```

`examples/chat-server/tests/end_to_end.rs` runs chat-core's real HTTP + SSE client (`HttpRepo` over `UreqClient`) against the server over sockets: accounts and tokens, the server owning the author, data surviving a restart, rooms, idempotent post, `since`, `404`s, and the six-step demo below with a real kill and restart on the same port.

## The demo interaction

This goes in the README with a GIF once the hosts exist. `killer_demo_interaction` in the chat-core tests runs the same script against the in-memory repo, and `killer_demo_interaction_over_real_sockets` in the chat server tests runs it over HTTP.

1. Open a chat, type a message, tap send. It goes `Sending` → `Sent`.
2. Navigate back to the list. Nothing is cancelled: a message in flight still completes.
3. Kill the chat server. The open chat shows "reconnecting" (`StreamDisconnected { will_retry: true }`).
4. Send again. Since M2 the message waits in the outbox: it stays `Sending` and the send returns it without an error. (Before M2 it failed with `ChatError::Network` and turned red.)
5. Restart the server. The stream reconnects on its own, catches up, and wakes the outbox, which sends the queued message: `Sending` → `Sent`, with no tap.
6. While in the chat, post a message with curl. It appears within a second via `MessageReceived`:

```sh
curl -X POST localhost:8787/rooms/general/messages \
  -H 'Content-Type: application/json' \
  -d '{"id":"curl-1","author":"curl","body":"hello from curl"}'
```

That sequence shows status transitions via events, external event delivery, reconnect with catch-up, and the outbox's idempotent resend in one flow.

## Changes from the original plan, and why

The plan was written before the Weather example landed. Checked against the repo:

| Plan | Now | Why |
|---|---|---|
| `forge-net` does the HTTP | Added `HttpClient::open_stream` to forge-net | `HttpClient::send` buffers the whole body (with an 8 MiB cap), so it cannot carry an endless SSE stream. |
| `async fn` commands, `chat_events() -> AsyncSequence<ChatEvent>` | Blocking core; async + `AsyncSequence` at the FFI layer | Matches the other examples: UniFFI built-in async without tokio, worker thread per operation (Weather), pull-based event subscription (Counter). UniFFI has no `AsyncSequence` type; the Swift bridge builds one from `next()`. |
| One *global* event stream | One stream per `ChatClient` (per app instance) | Same as Counter's `subscribe()`; still one stream for all rooms, filtered by the host. |
| `forge-storage` JSON snapshots | `forge_storage::write_atomic` + chat's own snapshot type | `JsonSnapshotStore` is hard-wired to the Counter's envelope; only its atomic write is reusable. |
| `"version": 1` | `"schema_version": 1` | The key every other Forge store uses. |
| Seconds (`1730000000`) | Milliseconds | Second resolution makes `since` lossy within a busy second. |
| Command list without retry | Added `retry_message` | Step 5 of the demo needs it. Client-chosen message ids make it idempotent. |
| `Cancelled` "if the user navigates away mid-send" | Navigating away never cancels a send; `Cancelled` is what a send reports when the app shuts down while it is in flight | Step 2 of the demo says leaving cancels nothing; both cannot hold. Not cancelling is the better UX (the message is already on the wire). |
| `StreamDisconnected { conversation_id, reason }` | Added `will_retry` | The UI needs to tell "reconnecting" from "gone". |
| `ChatError` as listed | Added `InvalidInput` and `ShutDown` | Same grouping as `WeatherError` / `ForgeAppError` (input, lifecycle). |
| Phase 2: `chat-ffi` crate or a module | A module, `forge-ffi/src/chat` (since moved to `examples/chat-android/rust/src/chat`) | At the time one `forge_ffi` library exported every example's API. Each app now owns its crate (see [app-crate.md](app-crate.md)). |
| `examples/chat-core` | As planned | It is a workspace member like the crates under `crates/`. |

## The FFI surface (Phase 2)

`examples/chat-android/rust/src/chat` exposes `ForgeChatApp`. The types mirror the core with a `Chat` prefix (`ChatConversation`, `ChatMessage`, `ChatMessageStatus`), a leftover from when every example shared one generated module. The contract is in [ffi-policy.md](ffi-policy.md#long-lived-event-streams-chat).

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
2. **The TLS verifier: wired (2026-11).** `UreqClient` uses `rustls-platform-verifier`, which on Android needs a JNI init with the app `Context` and its companion Kotlin library. Phase 5 refused `https://` on Android with `UnsupportedOnHost` until that existed. Now `MainActivity` calls `ForgeAndroid.init(applicationContext)` (shipped with the generated bindings), the app depends on `org.rustls:rustls-platform-verifier`, and `https://` servers work with Android's trust store. A rejected certificate is `ChatError::Certificate`; an `https` request before the init is `UnsupportedOnHost` with a message that says so. See [android-setup.md](android-setup.md#https-certificate-verification).

## Risks

| Risk | Mitigation |
|---|---|
| Android TLS verifier not wired | Wired: `ForgeAndroid.init` + the verifier's Kotlin library. Until init, `https` fails at once with a typed error, never unverified (see above). |
| Cancellation semantics differ between Swift and Kotlin | Explicit subscribe/unsubscribe, tested in Rust (`live_pumps()` reaches zero, the server sees the stream closed). |
| Event queue overflow under load | `EventsDropped` + re-read from the store; tested with a 512-message burst. |
| A blocked stream read outlives unsubscribe | Fixed in M13: each room streams on its own connection, which unsubscribe and shutdown close (`HttpClient::open_stream_cancellable`). Tested against the chat server with heartbeats 60 s apart. |
| Store rewrite per message is slow on a phone | Measured in Phase 4/5; batch writes in the FFI layer if needed. |
| Scope creep (media, auth, receipts) | Non-goals above. |
| The server grows into a project | It stays a single-binary server for your own devices: one JSON file, no hosting, no push. |
| The account token leaks | The app keeps it in the Keychain (this device only, after first unlock) and never passes it to Swift; the server and chat-cli files are owner-only. Plain HTTP on the LAN still exposes it to the network: use a TLS proxy beyond a home network. |
