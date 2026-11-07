# Chat example (Android)

The Compose twin of the SwiftUI [Chat example](../chat/README.md): a conversation list
and a chat screen over the same Rust core, the same FFI surface and the same chat server.

**Frozen (2026-11-07).** The iOS app ([examples/chat](../chat/README.md)) has grown into the
flagship acceptance app with accounts; this client stays at its original features until it is
brought onto the same app crate. It talks to the chat server in anonymous mode.
Design: [docs/chat-demo.md](../../docs/chat-demo.md).

```
ChatRoot (Compose) → ChatViewModel → ChatSession → Generated Kotlin (uniffi.forge_ffi)
  → forge-ffi (ForgeChatApp) → chat-core → forge-net (UreqClient)
  → examples/chat-server (HTTP + Server-Sent Events, --allow-anonymous)
```

Not a production chat client. No auth, push, media, read receipts or encryption, and PRs
adding them will be declined. New messages in rooms you are not in do show as local
notifications while the app runs.

## Run it

1. Start the chat server without accounts: `make chat-server ARGS="--allow-anonymous"` (rooms `general`, `random`, `forge`).
2. Build and install: `cargo run -p forge-cli -- build android --project examples/chat-android`,
   or stage the native library with
   `FORGE_ANDROID_JNI_LIBS=$PWD/examples/chat-android/app/src/main/jniLibs ./scripts/package-android-library.sh examples/chat-android/Generated`
   and open the folder in Android Studio.
3. Pick the server:
   - **Emulator:** the default, `http://10.0.2.2:8787`, is your computer.
   - **Device over USB:** `adb reverse tcp:8787 tcp:8787`, then tap **Server** and enter
     `http://127.0.0.1:8787`.
   - **Device on Wi-Fi:** `make chat-server ARGS="--allow-anonymous --bind 0.0.0.0:8787"` and enter your
     computer's LAN address, e.g. `http://192.168.1.10:8787`.

The demo interaction is the same as on iOS; see [the iOS README](../chat/README.md#the-demo-interaction).
Post from curl with `localhost:8787` from your computer, whichever address the app uses.

## Tests

`ChatSessionTest` drives `ChatSession` against the real Rust core through the generated
bindings, and runs the demo interaction (send, kill the server, failed send, restart, retry,
leave the room and watch its Rust thread end) against the real chat server:

```sh
cargo build -p forge-ffi -p chat-server
cd examples/chat-android && ./gradlew :app:testDebugUnitTest
```

Without the chat server binary the server tests are skipped; CI always builds it.

## Where things live

| What | Where |
|------|-------|
| Presentation state; applies Rust events, re-reads the store after `EventsDropped` | `app/src/main/java/org/forgemobile/chat/ChatSession.kt` |
| Local notifications (channel, permission check) | `AndroidChatNotifier.kt` |
| The Rust instance per server URL; commands in `viewModelScope` | `ChatViewModel.kt` |
| `ChatException` → text for the UI | `ChatErrors.kt` |
| Screens | `ui/ChatScreens.kt` |
| Tests, including the demo against the chat server | `app/src/test/java/org/forgemobile/chat/` |

## Notes

- **Local notifications.** `ChatSession` watches every room when it has a notifier, and
  posts Rust's `Notify` events through `AndroidChatNotifier` on a high-importance channel,
  so they show as banners in the foreground too. On Android 13+ the app asks for
  `POST_NOTIFICATIONS` at launch; saying no only turns them off. Only while the app runs:
  there is no push server. Design: [docs/chat-demo.md](../../docs/chat-demo.md#notifications).
- **https works, verified by Android.** `MainActivity` calls `ForgeAndroid.init` before the
  view model opens the Rust app, so the Rust client checks `https://` certificates against
  Android's trust store (`rustls-platform-verifier`). A rejected certificate shows "security
  certificate isn't trusted" (`ChatException.Certificate`). See
  [docs/android-setup.md](../../docs/android-setup.md#https-certificate-verification).
- **Plain `http://` still works.** Android's cleartext policy is enforced by the Java HTTP
  stacks; the Rust client opens its own sockets, so the chat server needs only the
  `INTERNET` permission.
- **Leaving a room never cancels a send.** Commands run in `viewModelScope`, not in the
  screen's composition; leaving the chat screen only unsubscribes from the room.
- **`ChatSession` is plain Kotlin** (`StateFlow`, no Android types), so the JVM tests use it as
  is. Compose reads it with `collectAsState()`.
- **One store per server URL**, under the app's files directory.
- `Generated/` is an identical copy of the Kotlin bindings every Android example uses
  (`scripts/check-bindings.sh` keeps it in sync).
