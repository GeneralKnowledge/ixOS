# Your app's Rust crate

Every app made with `forge init` has its own Rust crate in `rust/`. Your logic, state, persistence and networking live there, and the SwiftUI or Compose host calls it through UniFFI. You never edit Forge's own crates to build an app.

```
MyApp/
  forge.toml           # [rust] path = "rust"
  rust/
    Cargo.toml         # your dependencies; Forge crates by path
    Cargo.lock         # copied from the Forge checkout, so you start on tested versions
    src/lib.rs         # your API: #[uniffi::export], records, errors, events
  Generated/           # Swift or Kotlin bindings, rewritten by forge build / bindings generate
  build/               # staged native libraries (iOS); gitignored
  ...host project (Package.swift + Sources/, or Gradle app/)
```

## The loop

```bash
cd MyApp
cargo test --manifest-path rust/Cargo.toml        # Rust tests, no device or SDK needed
forge bindings generate --project .                # after changing the exported API
forge test --project .                             # Rust tests + host tests (JVM, or XCTest on macOS)
forge run ios --project .                          # Linux/xtool: build, install, launch on an iPhone
forge build android --project . --install          # debug APK on a connected Android phone
```

`forge build` and `forge run` rebuild the crate and regenerate the bindings every time, so a changed Rust API reaches the host without a separate step. `forge bindings generate --project .` is for when you want the new Swift or Kotlin API in your editor before building.

## What the starter shows

`rust/src/lib.rs` is a small notes app. Each part is a pattern you will need, so replace them one at a time:

| Pattern | In the starter | Forge piece |
|---------|----------------|-------------|
| An object the host creates once | `AppCore::new(storage_directory)` | `#[derive(uniffi::Object)]` |
| State that survives restarts, with a schema version | `notes.json` via `JsonFile` | [`forge_storage::JsonFile`](../crates/forge-storage/src/json_file.rs) |
| `async` commands that never block the UI thread | `add_note`, `delete_note`, `check_url` | [`forge_runtime::background`](../crates/forge-runtime/src/background.rs) |
| Typed errors, the same cases on both platforms | `AppError` | `#[derive(uniffi::Error)]` |
| Events the host subscribes to | `events()` → `AppEvents.next()` | [`forge_runtime::EventHub`](../crates/forge-runtime/src/events.rs) |
| HTTPS with the OS's certificate checks | `check_url` | [`forge_net::UreqClient`](../crates/forge-net/src/ureq_client.rs) |
| A request the host can stop part way | `check_url(url, cancellation)`, `Cancellation` | [`forge_runtime::background_cancellable`](../crates/forge-runtime/src/background.rs), `HttpClient::send_cancellable` |
| The app lifecycle: launch, foreground, inactive, background, resume, terminate | `set_phase(phase)`, `AppPhase`, `PhaseChanged` | [`forge_runtime::Lifecycle`](../crates/forge-runtime/src/lifecycle.rs) |
| Shutdown that refuses new work, stops requests and ends streams | `shutdown()` | `CancelHandle`, `EventHub::close` |

The hosts (`Sources/AppHost/NotesModel.swift` and `AppLifecycle.swift`, `app/src/main/java/.../NotesViewModel.kt` and `AppLifecycle.kt`) show the matching Swift and Kotlin side: open once, subscribe before reading, await commands, map `AppError` to text. Their tests run against the real Rust library, not mocks.

## Rules that keep it working

- **Keep the library name.** `[lib] name = "forge_ffi"` and `uniffi::setup_scaffolding!("forge_ffi")` fix the native library name (`libforge_ffi.a` / `.so`), the Swift module (`ForgeFFI`) and the Kotlin package (`uniffi.forge_ffi`) that the host projects and build scripts expect. Rename the package (`app-core`) as you like.
- **Keep `uniffi` pinned** to the version Forge's `uniffi-bindgen` uses (`=0.32.2` today; see [compatibility.md](compatibility.md)). A different version generates bindings that fail their checksum check at startup.
- **Keep `forge-runtime` as a dependency** on Android: it carries the JNI entry point behind `ForgeAndroid.init(context)`, without which HTTPS requests fail with `TlsNotInitialised` ([android-setup.md](android-setup.md#https-certificate-verification)).
- **Do blocking work in `background`.** An exported `async fn` runs on whatever thread polls it; file and network calls belong in `background(...)`. Plain (non-async) exports run on the caller's thread, which is the UI thread when the host calls them from a view.
- **Errors carry `reason`, not `message`.** A field named `message` clashes with Kotlin's `Throwable.message`.
- The FFI rules for the whole project are in [ffi-policy.md](ffi-policy.md).

## Lifecycle

**Cancelling a request.** Use `background_cancellable` with `HttpClient::send_cancellable` for work that should stop when the user gives up. The request then ends at once with `Cancelled`, even when it is waiting on a slow server, because Forge closes its connection. The two hosts reach it differently:

- **Kotlin:** cancelling the coroutine is enough. UniFFI drops the Rust future, and dropping it cancels the work.
- **Swift:** cancelling a `Task` does not reach Rust (UniFFI 0.32), so the host passes a `Cancellation` and cancels it from `withTaskCancellationHandler`, as `NotesModel.checkURL` does. Start such work from `.task(id:)` and SwiftUI cancels it when the screen closes ([ios-ui.md](ios-ui.md#cancellation)).

DNS lookup and the TCP connect itself are not interrupted; the request timeout bounds them.

**Shutdown.** `shutdown()` refuses new commands, cancels every request in flight (they fail with `ShutDown`), and ends every event stream. Saves already running finish, so a write is never cut short. Each command saves before it returns, so nothing is lost when the process is killed afterwards. The Android host calls `close()` from `ViewModel.onCleared`. The iOS host keeps one model for the life of the app.

**One instance per storage directory.** Two `AppCore`s on the same folder would each overwrite the other's `notes.json`. The hosts create one instance and share it. Instances on different folders are independent; the starter's tests check this.

**The app lifecycle.** The host reports where the app is with one call, `set_phase(phase)`, and Rust decides what that means. The starter's hosts already do this, so a new app gets it for free:

| `AppPhase` | iOS (SwiftUI) | Android (activity) | What the starter's Rust does |
|---|---|---|---|
| `Launching` | before the first report | before the first report | the initial phase |
| `Active` | `scenePhase == .active` | `onResume` | nothing |
| `Inactive` | `scenePhase == .inactive` | `onStart`, `onPause` | nothing |
| `Background` | `scenePhase == .background` | `onStop`, unless the activity is only rotating | stops requests started on screen; they fail with `Interrupted` |
| `Terminated` | `willTerminateNotification` | (the view model's `onCleared` calls `close()`) | `shutdown()` |

On the way back from `Background`, Rust sends `PhaseChanged { resumed: true }` on the event stream, and the starter hosts re-read the notes. That is where an app refreshes data that may have changed while it was away and restarts work the background stopped (a live stream, a poll). Repeated reports are ignored, and nothing after `Terminated` counts.

- **iOS:** `ContentView` attaches `.forgeLifecycle { model.setPhase($0) }` (in `AppLifecycle.swift`). It reports the scene phase the view appears in, every change, and `willTerminate`.
- **Android:** `MainActivity` calls `reportLifecycle { model.setPhase(it) }` (in `AppLifecycle.kt`). It ignores the `onStop` of a rotation, so a request survives turning the phone.

In Rust, `forge_runtime::Lifecycle` keeps the phase and two cancellation scopes. `lifetime()` is cancelled on `Terminated`. `foreground()` is cancelled on `Background` and renewed on resume. Run work that only makes sense on screen under `lifecycle.foreground().child()`, as `check_url` does. Run work that should finish anyway under the lifetime, or keep it short: iOS suspends a backgrounded app within seconds. `lifecycle.subscribe()` lets code on other threads follow changes.

Do not rely on `Terminated` to save anything. iOS sends `willTerminate` only when it ends an app that is still running, not one it has suspended, and either OS may kill a background process without notice. The starter saves before each command returns, so it loses nothing either way.

What is verified: the Rust, XCTest and JVM tests drive these phases through the real library. Phase changes on a physical device (pressing Home, the app switcher, an incoming call) have not been recorded yet.

## Adding dependencies

Add crates to `rust/Cargo.toml` as usual. They are compiled for the device targets too (`aarch64-apple-ios`, and the Android ABIs in `forge.toml`), so crates with C code need a cross compiler for those targets: on Linux, xtool's Darwin SDK for iOS and the NDK for Android, which `forge doctor` checks. Pure-Rust crates need nothing extra.

## Where the Forge crates come from

`rust/Cargo.toml` points at the Forge checkout the app was created from: a relative path when the app is inside the checkout, an absolute path when it is not. If you move the checkout, update those three `path =` lines (and, for iOS, `REPO_ROOT` in the Xcode project's "Build Rust FFI" phase). The crates are not on crates.io yet ([roadmap.md](roadmap.md#not-planned-for-now)).

## Older apps

Apps created before app-owned crates have `core_crate = "forge-core"` and `ffi_crate = "forge-ffi"` in `forge.toml` and build the workspace's shared `forge-ffi`. They keep working. The bundled examples (`examples/counter`, `chat`, `weather`) still use that layout because they share one library.
