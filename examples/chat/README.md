# Chat example

A deliberately plain SwiftUI chat client: a conversation list and a chat
screen. It exists to stress the FFI contract with what Counter and Weather do
not have: a long-lived, high-frequency event stream that outlives any single
command, explicit room subscriptions, multi-entity storage with a `since`
cursor, and typed errors. Design: [docs/chat-demo.md](../../docs/chat-demo.md).

```
ConversationListView / ChatView → ChatSession → ChatBridge (ChatClient, ChatEvents)
  → Generated Swift (ForgeFFI) → forge-ffi (ForgeChatApp) → chat-core → forge-net (UreqClient)
  → scripts/mock-chat-server (HTTP + Server-Sent Events)
```

Not a production chat client. No auth, push, media, read receipts or
encryption, and PRs adding them will be declined.

## Run it

Like every Forge example, this builds and installs from **Linux**: xtool
supplies the iOS toolchain (see [docs/linux-setup.md](../../docs/linux-setup.md)),
and the app runs on a physical iPhone. A Mac is optional.

1. Start the demo server so the phone can reach it:
   `make mock-chat ARGS="--bind 0.0.0.0:8787"` (rooms `general`, `random`, `forge`).
2. With the iPhone connected over USB, build and install:
   `cargo run -p forge-cli -- build ios --project examples/chat`.
3. In the app, tap **Server** and enter your computer's LAN address, e.g.
   `http://192.168.1.10:8787`. iOS asks for local network access the first
   time.

On a Mac you can also open `Xcode/Chat.xcodeproj`, or run the app on the
desktop with `swift run --package-path examples/chat Chat` (after
`./scripts/test-swift-host.sh examples/chat` has built the host library once;
the default server, `http://127.0.0.1:8787`, then works with plain
`make mock-chat`). The Swift tests also need macOS, so CI runs them on a Mac.

The Android version is [examples/chat-android](../chat-android/README.md).

## The demo interaction

1. Open **General**, type a message, tap **Send**: grey "Sending", then blue.
2. Go back to the list. Nothing is cancelled.
3. Stop the server (Ctrl-C). The chat shows "Reconnecting…".
4. Send again: the bubble turns red, "Not sent. Tap to retry."
5. Restart the server, tap the red bubble: Sending, then Sent. The room
   reconnects and catches up on its own.
6. Post from curl while the chat is open; the message appears within a
   second:

   ```sh
   curl -X POST localhost:8787/rooms/general/messages \
     -H 'Content-Type: application/json' -d '{"author":"curl","body":"hello from curl"}'
   ```

The mock server keeps everything in memory, so a restart empties it; the app
keeps its own copy.

## Where things live

| What | Where |
|------|-------|
| Swift face of the Rust instance; the event stream as an `AsyncSequence` | `Sources/ChatBridge/ChatClient.swift` |
| Rust `ChatError` → `ChatClientError` (`userMessage` for the UI) | `Sources/ChatBridge/ChatClientError.swift` |
| Presentation state; applies Rust events, re-reads the store after `eventsDropped` | `Sources/ChatApp/ChatSession.swift` |
| Screens | `Sources/ChatApp/ConversationListView.swift`, `ChatView.swift`, `ChatRootView.swift` |
| Tests, including the demo against the real mock server | `Tests/ChatAppTests/` (`scripts/test-swift-host.sh examples/chat`) |

## Notes

- **Leaving a room never cancels a send.** `ChatView` sends from a detached
  `Task`, and the bridge does not tie commands to task cancellation.
- **Plain HTTP.** The demo server is `http://`. The Rust client opens its own
  sockets, so App Transport Security (a URLSession policy) does not apply;
  iOS local network privacy does, hence `NSLocalNetworkUsageDescription`.
- **One store per server URL**, under Application Support, so switching
  servers never mixes rooms.
- `Generated/` is an identical copy of the bindings every iOS example uses
  (`scripts/check-bindings.sh` keeps it in sync).
