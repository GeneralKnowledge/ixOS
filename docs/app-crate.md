# Your app's Rust crate

Every app made with `forge init` has its own Rust crate in `rust/`. Your logic, state, persistence and networking live there, and the SwiftUI or Compose host calls it through UniFFI. You never edit Forge's own crates to build an app.

```
MyApp/
  forge.toml           # [rust] path = "rust"
  rust/
    Cargo.toml         # your dependencies; Forge crates pinned to a release (or by path, see project-generation.md)
    Cargo.lock         # copied from the Forge checkout, so you start on tested versions
    src/lib.rs         # your API: #[uniffi::export], records, errors, events
  Generated/           # Swift or Kotlin bindings, rewritten by forge build / bindings generate
  build/               # staged native libraries (iOS); gitignored
  ...host project (Package.swift + Sources/, or Gradle app/)
```

## The loop

```bash
cd MyApp
cargo test --manifest-path rust/Cargo.toml   # Rust tests, no device or SDK needed
forge bindings generate                      # after changing the exported API
forge test                                   # your crate's tests + host tests (JVM, or XCTest on macOS)
forge run ios                                # Linux/xtool: build and install on an iPhone
forge build android --install                # debug APK on a connected Android phone
```

`forge build` and `forge run` rebuild the crate and regenerate the bindings every time, so a changed Rust API reaches the host without a separate step. `forge bindings generate` is for when you want the new Swift or Kotlin API in your editor before building.

## What the starter shows

`rust/src/lib.rs` is a small notes app on the ixOS application runtime, [`forge_app`](../crates/forge-app/src/lib.rs) ([app-model-review.md](app-model-review.md)). Each part is a pattern you will need, so replace them one at a time:

| Pattern | In the starter | Forge piece |
|---------|----------------|-------------|
| An object the host creates once per process, on a data folder only it can hold | `AppCore::new(storage_directory)` | `#[derive(uniffi::Object)]`, [`forge_app::DataDirectory`](../crates/forge-app/src/runtime.rs) |
| State that survives restarts, with a schema version | `notes.json` via `JsonFile` | [`forge_storage::JsonFile`](../crates/forge-storage/src/json_file.rs) |
| Commands: one at a time, in order, each saved before it returns; then the revision rises and the host hears `Changed` | `add_note`, `update_note`, `delete_note` | [`Runtime::command`](../crates/forge-app/src/runtime.rs) |
| Queries that never wait for a command | `notes`, `search_notes` | [`forge_app::Published`](../crates/forge-app/src/published.rs) |
| Work that is not a command, stopped by the background, the host or shutdown | `check_url(url, cancellation)` | `Runtime::foreground` + `Runtime::run`, `Cancellation`, `HttpClient::send_cancellable` |
| Typed errors, each with a framework kind ForgeKit can present | `AppError`, `app_error_kind` | `#[derive(uniffi::Error)]`, `forge_app::ErrorKind`, `From<Stopped>` |
| Lifecycle, events, revision and shutdown | `forge_app::export_runtime!(AppCore, rt)` | `set_phase`, `events`, `revision`, `shutdown` on `AppCore` |
| HTTPS with the OS's certificate checks | `check_url` | [`forge_net::UreqClient`](../crates/forge-net/src/ureq_client.rs) |
| Logging to the system log, recent records kept for diagnostics, panics logged | `logging::init()` in `AppCore::new`, `logging::info!`; Swift writes the same log with `ForgeLog("Notes").warn(...)` because the crate calls `forge_runtime::log_export!()` | [`forge_runtime::logging`](../crates/forge-runtime/src/logging.rs), [`ForgeLog.swift`](../crates/forge-runtime/swift/ForgeLog.swift) |
| Native iOS services: Keychain, app directories, notifications, permissions, sharing, deep links | `forge_services::native_services!()` adds `crate::native`; the iOS host installs it with `AppServices.install()` | [`forge-services`](../crates/forge-services/src/lib.rs), [native-services.md](native-services.md) |

The Swift side is ForgeKit (`forgekit/`, released as the `ForgeKit` package). `Sources/AppHost/AppHost.swift` makes the app's `ForgeHost`, which opens the core once per process off the main thread, reports the application lifecycle (`ForgeLifecycle.attach`), runs the event loop, and moves `host.changes` when Rust commits a change or the app comes back from the background; the screens then re-read. `NotesModel.swift` only presents: it awaits commands and maps `AppError` to text. Its tests (`Tests/AppHostTests`) drive the model and the host against the real Rust library, not mocks, and `forge check swift` runs them on Linux.

Android keeps its own starter crate (`templates/android-rust`), from before the runtime: it uses `forge_runtime::app_shell!` for `AppPhase`, `Cancellation` and `AppEvents`, and its Kotlin host (`NotesViewModel.kt`, `AppLifecycle.kt`) opens, subscribes and reports phases itself. Android is work in progress and gets no new features.

## Rules that keep it working

- **The library name.** `[lib] name = "forge_ffi"` with `uniffi::setup_scaffolding!()` is the default. The host projects and build scripts always see Forge's fixed names: the staged `libforge_ffi.a`, the Swift module `ForgeFFI` and the Kotlin package `uniffi.forge_ffi`. A crate that another app links names its library after the app instead (`weather_core`) and leaves `cdylib` out of `crate-type`. Forge reads the name from `Cargo.toml`, stages the library as `libforge_ffi.a` and still generates its Swift as `ForgeFFI`, so the app's Swift package doesn't change. The app that links it gets its bindings as a module of their own, re-exported by its `ForgeFFI` (`examples/device-lab`, the Apps screen). Rename the package (`app-core`) as you like.
- **Keep `uniffi` pinned** to the version Forge's `uniffi-bindgen` uses (`=0.32.2` today; see [compatibility.md](compatibility.md)). A different version generates bindings that fail their checksum check at startup.
- **Keep `forge-runtime` as a dependency** on Android: it carries the JNI entry point behind `ForgeAndroid.init(context)`, without which HTTPS requests fail with `TlsNotInitialised` ([android-setup.md](android-setup.md#https-certificate-verification)).
- **Save in commands, block elsewhere off the caller's thread.** An exported `async fn` runs on whatever thread polls it. Changes that save go through `rt.command`, which runs them on the command thread; other slow work goes through `rt.run` (or `forge_runtime::background` in a crate without the runtime). Plain (non-async) exports run on the caller's thread, which is the UI thread when the host calls them from a view, so keep them to reads of `Published` state.
- **Errors carry `reason`, not `message`.** A field named `message` clashes with Kotlin's `Throwable.message`.
- The FFI rules for the whole project are in [ffi-policy.md](ffi-policy.md).

## Lifecycle

**Cancelling a request.** Use `background_cancellable` with `HttpClient::send_cancellable` for work that should stop when the user gives up. The request then ends at once with `Cancelled`, even when it is waiting on a slow server, because Forge closes its connection. The two hosts reach it differently:

- **Kotlin:** cancelling the coroutine is enough. UniFFI drops the Rust future, and dropping it cancels the work.
- **Swift:** cancelling a `Task` does not reach Rust (UniFFI 0.32), so the host passes a `Cancellation` and cancels it from `withTaskCancellationHandler`, as `NotesModel.checkURL` does. Start such work from `.task(id:)` and SwiftUI cancels it when the screen closes ([ios-ui.md](ios-ui.md#cancellation)).

DNS lookup and the TCP connect itself are not interrupted; the request timeout bounds them.

**Shutdown.** `shutdown()` refuses new commands, cancels every request in flight (they fail with `ShutDown`), and ends every event stream. Saves already running finish, so a write is never cut short. Each command saves before it returns, so nothing is lost when the process is killed afterwards. The Android host calls `close()` from `ViewModel.onCleared`. On iOS, ForgeKit's host keeps the core for the life of the process and calls `shutdown()` on `Terminated` or `host.close()`. What happens to a command when iOS suspends or ends the process is in [app-model-review.md](app-model-review.md#what-the-runtime-guarantees-when-ios-suspends-or-ends-the-process).

**One instance per storage directory.** Two `AppCore`s on the same folder would each overwrite the other's `notes.json`. The iOS core holds its folder with `DataDirectory`, so a second one fails with `InUse` until the first is released; the hosts create one instance and share it. Instances on different folders are independent; the starter's tests check this.

**The app lifecycle.** The host reports where the app is with one call, `set_phase(phase)`, and Rust decides what that means. The starter's hosts already do this, so a new app gets it for free:

| `AppPhase` | iOS (SwiftUI) | Android (activity) | What the starter's Rust does |
|---|---|---|---|
| `Launching` | before the first report | before the first report | the initial phase |
| `Active` | `scenePhase == .active` | `onResume` | nothing |
| `Inactive` | `scenePhase == .inactive` | `onStart`, `onPause` | nothing |
| `Background` | `scenePhase == .background` | `onStop`, unless the activity is only rotating | stops requests started on screen; they fail with `Interrupted` |
| `Terminated` | `willTerminateNotification` | (the view model's `onCleared` calls `close()`) | `shutdown()` |

On the way back from `Background`, Rust sends `PhaseChanged { resumed: true }` on the event stream, and the starter hosts re-read the notes (on iOS, `ForgeHost` moves `changes`). That is where an app refreshes data that may have changed while it was away and restarts work the background stopped (a live stream, a poll). Repeated reports are ignored, and nothing after `Terminated` counts.

- **iOS:** the `App` calls `ForgeLifecycle.attach(host)` (ForgeKit). It reports the application's active, inactive and background notifications, and `willTerminate`.
- **Android:** `MainActivity` calls `reportLifecycle { model.setPhase(it) }` (in `AppLifecycle.kt`). It ignores the `onStop` of a rotation, so a request survives turning the phone.

In Rust, `forge_runtime::Lifecycle` keeps the phase and the cancellation scopes, and `forge_app::Runtime` exposes them as work scopes:

| Scope | Use it for | Stopped by |
|---|---|---|
| `rt.foreground(…)` | Work that only makes sense on screen: a search, a refresh, `check_url` | The background (`Interrupted`), the host's `Cancellation`, shutdown |
| `rt.finishing(…)` | Work that should finish even if the person leaves the app: writing an export, sending what they just wrote (Checklist's `export_text`) | iOS's background time running out (`Interrupted`), the host's `Cancellation`, shutdown |
| `rt.lifetime(…)` | Work that may simply pause while iOS suspends the app | The host's `Cancellation`, shutdown |

**Background time.** When the app leaves the screen with commands still running or finishing work in flight, ForgeKit asks iOS for time to finish (`beginBackgroundTask`, installed by `ForgeLifecycle.attach`), and gives it back as soon as that work is done; Rust tells it with `BackgroundWork { pending }` events. The time is best effort: iOS decides how long (usually about 30 seconds) and may give none. When it runs out, finishing work is stopped with `Interrupted`. Commands can't be stopped once accepted: one still running when the app is suspended finishes when the app next runs, or never happened if iOS ends the app first. So finishing work must be safe to abandon at any point, and whatever must survive goes through a command.

**The launch report.** `host.launch` (Rust: `rt.launch_report()`) says whether the previous run was ended while it still had work pending in the background (`unfinishedBackgroundWork`). Show the person a short note if that work mattered to them (an export, a send). It needs the runtime to hold the data directory (`Runtime::with_directory`).

Android's crate uses `lifecycle.foreground().child()` directly. `lifecycle.subscribe()` lets code on other threads follow changes.

Do not rely on `Terminated` to save anything. iOS sends `willTerminate` only when it ends an app that is still running, not one it has suspended, and either OS may kill a background process without notice. The starter saves before each command returns, so it loses nothing either way.

On Android, `close()` also frees the Kotlin `AppCore`, and UniFFI's Kotlin throws `IllegalStateException` for any later call on a freed object before Rust sees it. So after a suspending call, read the view model's current `core` again rather than reusing the instance the call started with: the app may have closed in between. The starter's `NotesViewModel` does this (`refreshNotes`). Swift frees the object only when the last reference goes, so the same call there reaches Rust and gets `ShutDown`.

What is verified: the Rust, XCTest and JVM tests drive these phases through the real library. Phase changes on a physical device (pressing Home, the app switcher, an incoming call) have not been recorded yet.

## Adding dependencies

Add crates to `rust/Cargo.toml` as usual. They are compiled for the device targets too (`aarch64-apple-ios`, and the Android ABIs in `forge.toml`), so crates with C code need a cross compiler for those targets: on Linux, xtool's Darwin SDK for iOS and the NDK for Android, which `forge doctor` checks. Pure-Rust crates need nothing extra.

## Where the Forge crates come from

`rust/Cargo.toml` points at the Forge checkout the app was created from: a relative path when the app is inside the checkout, an absolute path when it is not. If you move the checkout, update those three `path =` lines (and, for iOS, `REPO_ROOT` in the Xcode project's "Build Rust FFI" phase). The crates are not on crates.io yet ([roadmap.md](roadmap.md#not-planned-for-now)).

## Older apps

Every app owns its crate, the bundled examples included. Two host apps can share one: `examples/counter-android` sets `[rust] path = "../counter/rust"` and builds the iOS Counter's crate. The old `core_crate` / `ffi_crate` settings named a shared `forge-ffi` crate that no longer exists, and `forge` now rejects them with a pointer here.
