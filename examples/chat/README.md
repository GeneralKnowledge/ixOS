# Chat

The flagship ixOS acceptance app: a SwiftUI messaging app on a Rust core,
growing one milestone at a time into a real iPhone chat app. Every feature
works for real and tests a real ixOS capability. Plan and status:
[docs/chat-demo.md](../../docs/chat-demo.md).

```
SwiftUI (Sources/AppHost: ChatModel, screens)        navigation, UI, notifications
  → Generated Swift (ForgeFFI) + forge-services adapters (Keychain, ...)
  → rust/ (ChatCore)                                   account, lifecycle, the UniFFI API
  → examples/chat-core (ChatClient)                    rooms, live streams, store, send/retry
  → forge-net (UreqClient) → examples/chat-server      HTTP + Server-Sent Events, accounts
```

Rust decides everything about the chat: who you are, which messages are
yours, unread counts, reconnects, retries. Swift shows it and owns the
platform (the Keychain adapter, notifications, the scene lifecycle).

## Run it (Linux, physical iPhone)

1. Start the chat server where the phone can reach it:
   `make chat-server ARGS="--bind 0.0.0.0:8787"`.
   Accounts and messages are kept in `~/.local/share/ixos-chat/server.json`.
2. With the iPhone on USB: `forge run ios --project examples/chat`
   (or `cargo run -p forge-cli -- run ios --project examples/chat`).
3. In the app, enter your name and your computer's address on the network,
   e.g. `http://192.168.1.10:8787`, and tap **Create Account**. iOS asks for
   local network access the first time.
4. Be the other person from the terminal:

   ```sh
   make chat-cli ARGS='signup "Reuben (laptop)"'
   make chat-cli ARGS="send general Hello from the laptop"
   make chat-cli ARGS="follow general"
   make chat-cli ARGS="read general"                 # numbered: @1 is the newest
   make chat-cli ARGS="react general @1 👍"
   make chat-cli ARGS="reply general @2 good point"
   make chat-cli ARGS="send-file general ~/photo.jpg a caption"
   make chat-cli ARGS="download general @1 /tmp"     # checked against its SHA-256
   make chat-cli ARGS="send-file general ~/note.m4a"  # shows on the phone as a voice message
   ```

The full device check is [ACCEPTANCE.md](ACCEPTANCE.md).

On a Mac you can also open `Xcode/Chat.xcodeproj`, or run it on the desktop
with `swift run --package-path examples/chat Chat` (after
`./scripts/test-swift-host.sh examples/chat` has built the host library).

## What it does today (M0 to M4)

- **An account per device, in the Keychain.** First launch creates an account
  on the server. Rust stores it, token included, through `forge-services`'
  Keychain adapter; the token never reaches Swift. Relaunching is signed in at
  once, offline too. If the server forgets the account (its data was deleted),
  the app says so and you can sign out from Settings.
- **Rooms and messages**: live updates, sending with Sending / Sent / Failed
  (tap to retry), catch-up after a dropped connection.
- **A native messaging UI (M1).** Chats and Settings tabs; a list sorted by
  activity with previews, times and unread dots; swipe to mark read or unread
  and to mute; bubbles grouped by sender and time with day separators; a
  composer pinned above the keyboard with haptics on send and failure; new
  conversations created on the server from the app.
- **Real read receipts (M1).** The server keeps a read cursor per account and
  room. Opening a room moves yours, unread counts come from it (so reading on
  one device clears another), and your messages say Read once someone else's
  cursor passes them. Mute and "marked unread" stay on the device.
- **Message changes (M2).** Touch and hold a bubble to react (a row of
  quick emojis), reply (the answer quotes the original; tap the quote to jump
  to it), copy, edit your message ("Edited") or delete it for everyone (a
  "This message was deleted" placeholder). Changes by anyone arrive live and
  in catch-up, and never count as unread.
- **Drafts and an outbox (M2).** What you type is kept per conversation,
  across launches. Messages sent while the server cannot be reached wait as
  "Sending…" and go out by themselves when it is back, even after the app was
  closed; only a message the server refuses turns red.
- **Search (M2).** The Chats list searches every conversation on the phone,
  a chat's menu searches that chat; case and accents are ignored. Rust
  answers from a SQLite full-text index.
- **SQLite storage (M2).** The phone's chats are in a SQLite database
  through forge-storage, with migrations and recovery from a damaged file;
  Settings shows the SQLite version.
- **Photos and documents (M3).** The + button next to the text field adds
  photos (the system photo picker, no permission prompt) or documents (the
  Files picker), with an optional caption. Photos are shrunk on the phone to
  a 2048-pixel JPEG without their location. Files wait in the outbox with
  the message and are uploaded first; the server and the phone keep each file
  once, named by its SHA-256. Photos show as thumbnails, other files as rows
  with their size; tap one to open it in Quick Look, which also shares it.
  Files from others are downloaded when shown and checked before they are
  kept. Up to 10 files of 25 MB each per message.
- **Voice messages (M4).** With nothing typed, the send button is a
  microphone: tap it to record (Chat asks for the microphone the first
  time), watch the time and level meter, then send or discard. Voice
  messages play in the bubble, one at a time, with a waveform that fills as
  they play; they keep playing with the ring switch on silent. Recording
  and playback are Forge's optional audio module, not Chat code.
- **Lifecycle.** In the background Rust closes every room connection; back in
  the foreground it reconnects them and catches up, and the list refreshes.
- **A private assistant on the phone (L1, [local-ai.md](../../docs/local-ai.md)).**
  The Assistant tab (and a button on the sign-up screen, so no account is
  needed) chats with Apple's on-device model through Foundation Models: the
  plain assistant, or the test characters Leo and Mia. Answers stream in, Stop
  really stops the model, and the history stays on the phone in
  `assistant.db`, through sign-out and relaunch. Needs iOS 26 with Apple
  Intelligence on; otherwise the screen says why.
- **Local notifications** for new messages in rooms you are not in, while the
  app runs. There is no remote push (APNs), so nothing alerts you while the app
  is suspended or closed.

## Where things live

| What | Where |
|------|-------|
| The Rust API: `ChatCore`, records, events, errors | `rust/src/lib.rs`, `rust/src/types.rs` |
| Where the account is kept (Keychain, or memory in tests) | `rust/src/credentials.rs` |
| Rust tests, against the real chat server in process | `rust/src/tests.rs` (`cargo test --manifest-path examples/chat/rust/Cargo.toml`) |
| Presentation state; applies Rust events | `Sources/AppHost/ChatModel.swift` |
| Screens | `Sources/AppHost/ContentView.swift`, `SignUpView.swift`, `ConversationListView.swift`, `ChatView.swift`, `SettingsView.swift` |
| Picking, shrinking, thumbnails and previews of attachments | `Sources/AppHost/Attachments.swift` |
| The on-device assistant: conversations, characters, prompt, `assistant.db`, the engine interface | `rust/src/assistant/` |
| The assistant's Swift side: Foundation Models adapter, model, screens | `Sources/AppHost/FoundationModelsEngine.swift`, `AssistantModel.swift`, `AssistantViews.swift` |
| Swift tests, against the real chat server | `Tests/AppHostTests/` (`scripts/test-swift-host.sh examples/chat`, macOS) |
| App settings (bundle id, permissions) | `forge.toml` (Forge writes Info.plist, App.entitlements and xtool.yml from it) |

## Notes

- **Leaving a room never cancels a send.** `ChatView` sends from a detached
  `Task`, and Rust finishes the send whatever the screen does.
- **Plain HTTP.** The server is `http://`. The Rust client opens its own
  sockets, so App Transport Security (a URLSession policy) does not apply; iOS
  local network privacy does, hence `NSLocalNetworkUsageDescription`.
- **One store per account**, under Application Support. Signing out deletes it
  (the messages stay on the server).
