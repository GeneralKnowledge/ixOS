# Chat example (Android)

The Compose twin of the SwiftUI [Chat example](../chat/README.md): a conversation list
and a chat screen over the same Rust core, the same FFI surface and the same mock server.
Design: [docs/chat-demo.md](../../docs/chat-demo.md).

```
ChatRoot (Compose) → ChatViewModel → ChatSession → Generated Kotlin (uniffi.forge_ffi)
  → forge-ffi (ForgeChatApp) → chat-core → forge-net (UreqClient)
  → scripts/mock-chat-server (HTTP + Server-Sent Events)
```

Not a production chat client. No auth, push, media, read receipts or encryption, and PRs
adding them will be declined.

## Run it

1. Start the demo server: `make mock-chat` (rooms `general`, `random`, `forge`).
2. Build and install: `cargo run -p forge-cli -- build android --project examples/chat-android`,
   or stage the native library with
   `FORGE_ANDROID_JNI_LIBS=$PWD/examples/chat-android/app/src/main/jniLibs ./scripts/package-android-library.sh examples/chat-android/Generated`
   and open the folder in Android Studio.
3. Pick the server:
   - **Emulator:** the default, `http://10.0.2.2:8787`, is your computer.
   - **Device over USB:** `adb reverse tcp:8787 tcp:8787`, then tap **Server** and enter
     `http://127.0.0.1:8787`.
   - **Device on Wi-Fi:** `make mock-chat ARGS="--bind 0.0.0.0:8787"` and enter your
     computer's LAN address, e.g. `http://192.168.1.10:8787`.

The demo interaction is the same as on iOS; see [the iOS README](../chat/README.md#the-demo-interaction).
Post from curl with `localhost:8787` from your computer, whichever address the app uses.

## Tests

`ChatSessionTest` drives `ChatSession` against the real Rust core through the generated
bindings, and runs the demo interaction (send, kill the server, failed send, restart, retry,
leave the room and watch its Rust thread end) against the real mock server:

```sh
cargo build -p forge-ffi -p mock-chat-server
cd examples/chat-android && ./gradlew :app:testDebugUnitTest
```

Without the mock server binary the server tests are skipped; CI always builds it.

## Where things live

| What | Where |
|------|-------|
| Presentation state; applies Rust events, re-reads the store after `EventsDropped` | `app/src/main/java/org/forgemobile/chat/ChatSession.kt` |
| The Rust instance per server URL; commands in `viewModelScope` | `ChatViewModel.kt` |
| `ChatException` → text for the UI | `ChatErrors.kt` |
| Screens | `ui/ChatScreens.kt` |
| Tests, including the demo against the mock server | `app/src/test/java/org/forgemobile/chat/` |

## Notes

- **http only on Android.** The Rust client checks certificates with
  `rustls-platform-verifier`, which on Android needs a JNI init that this example does not do
  yet. Rather than fail every request with a TLS error that looks like "server down",
  `ForgeChatApp` refuses an `https://` URL on Android at once with
  `ChatException.UnsupportedOnHost`, and the app says so.
- **No `network_security_config`.** Android's cleartext policy is enforced by the Java HTTP
  stacks; the Rust client opens its own sockets, so plain `http://` works with just the
  `INTERNET` permission.
- **Leaving a room never cancels a send.** Commands run in `viewModelScope`, not in the
  screen's composition; leaving the chat screen only unsubscribes from the room.
- **`ChatSession` is plain Kotlin** (`StateFlow`, no Android types), so the JVM tests use it as
  is. Compose reads it with `collectAsState()`.
- **One store per server URL**, under the app's files directory.
- `Generated/` is an identical copy of the Kotlin bindings every Android example uses
  (`scripts/check-bindings.sh` keeps it in sync).
