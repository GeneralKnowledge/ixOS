# Native services

Forge splits an app in two. Rust holds the logic: state, persistence, networking and domain rules. The native layer holds what makes the device a device. On iOS that layer is SwiftUI plus a set of small Swift adapters. This page covers those adapters, and the Rust interfaces they implement.

| Service | Rust (`crate::native`) | iOS (in `ForgeFFI`) |
|---------|------------------------|---------------------|
| Keychain | `SecureStore`; `NativeServices::read_secret_string` / `write_secret_string` / `delete_secret` | `KeychainSecureStore` |
| App directories | `AppDirectories` (documents, application support, caches, temporary) | `ForgeFiles.appDirectories()` |
| File import | (reads the copied file by path) | `ForgeFiles.importCopy(of:)`, `.forgeFileImporter(...)` |
| Local notifications | `LocalNotifier`; `NativeServices::schedule_notification` / `cancel_notification` | `UserNotificationsNotifier` |
| Permissions | `PermissionProvider`, `NativePermission`, `PermissionStatus` | `ForgePermissions`, `ForgePermissionAdapter` |
| Sharing | `ShareItem` (text, URL, file) | `ForgeShareLink(item:)` |
| Deep links | `DeepLink`, `parse_deep_link` | `ForgeDeepLinks`, `.forgeOpenURLs()`, `.onForgeDeepLink { }` |

Every app from `forge init` has these services installed. Device capabilities (connectivity, device status, motion, location, Bluetooth, audio) are [optional services](#optional-services) an app selects. Android implementations are not written yet. On Android, `NativeServices::installed()` returns `NotInstalled` (see [Android](#android)).

## How it fits together

```
rust/src/lib.rs                      forge_services::native_services!();
  └─ crate::native                   records, errors, the three host traits,
                                     NativeServices (the Rust-side API)
Generated/ForgeFFI/forge_ffi.swift   protocols SecureStore, LocalNotifier,
                                     PermissionProvider (from UniFFI)
Generated/ForgeFFI/ForgeServices*.swift
                                     Forge's Swift implementations, copied from
                                     crates/forge-services/swift/ by forge build
Sources/AppHost/AppServices.swift    AppServices.install(), .receivesDeepLinks()
Sources/AppHostXtoolApp/AppMain.swift
                                     calls both at launch
```

- **One namespace.** The macro emits the UniFFI items into your crate, so they join `forge_ffi`. They land in the same Swift module (`ForgeFFI`) and Kotlin package as your own API. There is no second native library, header or module.
- **Swift implements Rust's traits.** `SecureStore`, `LocalNotifier` and `PermissionProvider` are UniFFI foreign traits (`#[uniffi::export(with_foreign)]`). The Swift adapters compile inside `ForgeFFI`, next to the generated protocols, and `ForgeServices.install()` hands them to Rust once at launch.
- **Checked in Rust.** Validation that should behave the same on every platform runs in Rust before anything reaches the platform: notification triggers, secret keys and URL parsing.
- **Version check.** `forge_services::VERSION` and the adapters' `forgeServicesSwiftVersion` must match. `ForgeServices.install()` refuses and logs if they differ, and `scripts/check-template-sync.sh` checks it in CI.

## Using the services from Rust

Take the services when your app object is created, and keep them. That lets tests pass in fakes.

```rust
use crate::native::{LocalNotification, NativePermission, NativeServices, NotificationTrigger};

#[derive(uniffi::Object)]
pub struct AppCore {
    services: Option<NativeServices>, // None on hosts that have not installed them
    // ...
}

#[uniffi::export]
impl AppCore {
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, AppError> {
        let services = NativeServices::installed().ok();
        // ...
    }

    /// Saves the API token in the Keychain.
    pub fn sign_in(&self, token: String) -> Result<(), AppError> {
        let services = self.services.as_ref().ok_or(AppError::Unsupported)?;
        services.write_secret_string("api-token", &token)?;
        Ok(())
    }

    /// Reminds the user in an hour; tapping opens the book.
    pub async fn remind(&self, book_id: u64) -> Result<(), AppError> {
        let services = self.services.clone().ok_or(AppError::Unsupported)?;
        if !services.request_permission(NativePermission::Notifications).await.allows_use() {
            return Err(AppError::NotificationsOff);
        }
        let mut reminder = LocalNotification::now(format!("book-{book_id}"), "Keep reading", "");
        reminder.trigger = NotificationTrigger::After { seconds: 3600, repeats: false };
        reminder.deep_link = Some(format!("myapp://book/{book_id}"));
        services.schedule_notification(reminder).await?;
        Ok(())
    }
}
```

Map `native::NativeServiceError` into your own `AppError`, the way the starter maps `StorageError`. The error cases are `NotInstalled`, `InvalidInput`, `PermissionDenied`, `Unavailable` and `Platform { code, reason }`.

Things to know:

- **Keychain calls are synchronous.** Call them inside `forge_runtime::background` or from a non-UI path, as you would file I/O. Items use `AfterFirstUnlockThisDeviceOnly`: background work can read them after the first unlock, and they never sync to another device.
- **Notifications and permissions are `async`.** The Swift side awaits `UNUserNotificationCenter`. Scheduling fails with `PermissionDenied` if the user has said no, or has not been asked yet, so request the permission first, from something the user did. A notification with the `id` of a pending one replaces it.
- **Store paths relative to `AppDirectories`.** iOS changes the container path between installs, so a stored absolute path breaks after an update.

### Tests

`NativeServices::for_tests(dir)` returns in-memory services and the fakes behind them: `MemorySecureStore`, `RecordingNotifier` (`.scheduled()`, `.deny()`) and `FixedPermissions` (`.set(..)`). They run with `cargo test` on Linux and need no device:

```rust
let dir = tempfile::tempdir().unwrap();
let (services, fakes) = native::NativeServices::for_tests(dir.path());
let app = AppCore::with_services(dir.path(), services); // your own constructor
forge_runtime::block_on(app.remind(7)).unwrap();
assert_eq!(fakes.notifier.scheduled()[0].deep_link.as_deref(), Some("myapp://book/7"));
```

`crates/forge-services/tests/native_services.rs` shows every fake in use.

## Using the services from SwiftUI

The starter's entry point already calls `AppServices.install()` in `App.init()` and attaches `.receivesDeepLinks()` to the root view. In views:

```swift
// React to myapp://book/7, https://example.com/book/7, or a notification tap.
.onForgeDeepLink { link in
    if link.route.first == "book", let id = link.route.dropFirst().first.flatMap(UInt64.init) {
        model.showBook(id)
    }
}

// Share text, a link, or a file Rust wrote. The sheet includes "Save to Files".
ForgeShareLink(item: .file(path: exportPath, title: "Library export"))

// Import: you get a copy the app owns, ready to pass to Rust by path.
.forgeFileImporter(isPresented: $importing, allowedContentTypes: [.epub]) { result in
    if case .success(let url) = result { Task { await model.importBook(path: url.path) } }
}

// Permission state for the UI, and the way back from "denied".
let status = await ForgePermissions.shared.status(permission: .notifications)
ForgePermissions.openSettings()
```

Links that arrive before any view has registered `onForgeDeepLink` are held, up to 16, and delivered to the first handler. This covers a cold launch from a link.

## Swift helpers

Some platform code has no Rust side at all. These files are copied into every app's `Generated/ForgeFFI`, whether or not it uses `native_services!`, from `crates/forge-services/swift/helpers/`:

| Helper | What it does |
|--------|--------------|
| `ForgeAudioSession` | `activate(.playback)` (spoken audio, plays with the ring switch on silent) or `activate(.record)`, and `deactivate()` so other apps' audio resumes. Needs no permission, so an app that only plays sound (Pocket Library's read-aloud) uses it without selecting `audio`. AVFoundation, iOS only; does nothing elsewhere |
| `ForgeWeb` | Web pages inside the app, on WebKit and SafariServices; see [Web pages](#web-pages) below. iOS only |
| `ForgeImages` | `thumbnail(of: url, maxPixels:, cached:)` and `thumbnail(of: data, maxPixels:)` decode an image straight to the size shown, the right way up, off the main actor; `writeDownscaledJPEG(from:maxPixels:to:)` turns a picked photo into a smaller JPEG without its location or other metadata; `pixels(_ points:, scale:)` converts a view size. Uses ImageIO only |

### Web pages

The jobs apps keep repeating, each in a line or two. All of them are iOS only (`#if os(iOS)`).

| Job | Use |
|-----|-----|
| A help page, privacy policy, terms, or an article's original page | `.forgeWebSheet($page)` with `page = .link(url, title: "Privacy")`. The page's own site opens in the sheet, every other link in Safari |
| HTML the app already has (an article body, a feed's rich text) | `page = .html(html, title:)`: no scripts, nothing from the network, links open in Safari |
| A help page bundled with the app | `page = .bundled(Bundle.main.url(forResource: "help", withExtension: "html")!)`: may read files in its own folder only |
| A link the app doesn't control | `.forgeSafari($url)`, Safari inside the app (`SFSafariViewController`): the person's own Safari, which the app can't see into |
| A web page as part of the app's own flow | `ForgeWebScreen(page)` pushed in a `NavigationStack`, or `ForgeWebView(source, policy:, state:)` with a `ForgeWebState` for a custom screen |

`ForgeWebScreen` adds a progress bar, Back, Open in Safari and an error view with Try Again around the page (identifiers `web.page`, `web.back`, `web.openInSafari`, `web.retry`, and `web.done` on the sheet). `ForgeWebState` exposes the title, address, progress, back and forward, and a `ForgeWebError` (`loadFailed`, `downloadRefused`, `contentCrashed`).

`ForgeWebPolicy` keeps each power a separate opt-in (the spec is in [native-capability-roadmap.md](native-capability-roadmap.md#web-view)):

- `allowedHosts`: where top-level navigation may go; a host includes its subdomains. Other web links open in Safari, and only when the person tapped them.
- `externalSchemes`: non-web schemes (`mailto`, `tel`) handed to the system; any other scheme is refused.
- `javaScript` (on by default), `persistentWebsiteData` (off: nothing kept between launches), `remoteContent` (on; off blocks every http and https load with a content rule list).
- Never configurable here: downloads (refused, with `downloadRefused`), a JavaScript message bridge (there is none), a second window (`window.open` loads in place or not at all). A crashed web process reloads the page once, then reports `contentCrashed`.

## Optional services

Device capabilities that link privacy-sensitive or heavy frameworks are opt-in. An app names the ones it uses in the macro call, and gets only those:

```rust
forge_services::native_services!(connectivity, device_status, motion, location, bluetooth, audio);
```

| Service | Rust (`crate::native::<name>`) | iOS adapter | Permission and `forge.toml` |
|---------|------------------------------|-------------|-----------------------------|
| `background` | `BackgroundService`: `register_refresh`, `register_processing`, `register_handler`, `schedule`, `cancel`; see [Background tasks](#background-tasks-background) | `ForgeBackgroundScheduler` (`BGTaskScheduler`) | None; `forge.toml` `[ios] background_tasks` |
| `connectivity` | `ConnectivityService`: `current()`, `watch(..)`; `ConnectivityStatus` (online, interfaces, metered, Low Data Mode, offline reason), `allows(TransferPolicy::Unmetered)` | `ForgeConnectivityMonitor` (`NWPathMonitor`) | None |
| `device_status` | `DeviceStatusService`: `info()` (model, iOS version, cores, RAM, screen), `status()` (battery, Low Power Mode, thermal state, storage, memory left), `watch(..)`; `DeviceStatus::defer_heavy_work(memory)` | `ForgeDeviceStatus` (`UIDevice`, `ProcessInfo`, file system, `os_proc_available_memory`) | None |
| `motion` | `MotionService`: `availability()`, `watch(sensor, interval_ms, ..)` for the accelerometer, gyroscope, magnetometer, fused device motion (attitude, gravity, user acceleration, heading) and the barometer | `ForgeMotionSensors` (Core Motion) | The barometer needs `NativePermission::Motion` and `[ios.permissions] motion`; the other sensors need nothing |
| `location` | `LocationService`: `watch(LocationOptions, ..)` for fixes and compass headings, `last_known()`, `is_precise()`; `LocationFix::distance_to` | `ForgeLocationSource` (`CLLocationManager`, when in use) | `NativePermission::Location` and `[ios.permissions] location_when_in_use` |
| `bluetooth` | `BluetoothService`: `state()`, `watch(..)` (on, off, unauthorised, unsupported) | `ForgeBluetoothMonitor` (`CBCentralManager`) | `NativePermission::Bluetooth` and `[ios.permissions] bluetooth` |
| `audio` | `AudioService`: `duration(path)` | `ForgeAudioFiles`; for the UI `ForgeAudioRecorder`, `ForgeAudioPlayer`, `ForgeAudio.waveform` (and the `ForgeAudioSession` helper every app has) (AVFoundation) | `NativePermission::Microphone` and `[ios.permissions] microphone` |
| `transfers` | `TransferService`: `start(TransferRequest)`, `cancel`, `retry`, `remove`, `list`, `get`, `deliver_to(key, FileStore)`, `watch(..)`. Needs forge-services' `transfers` feature | `ForgeTransfers` (background `URLSession`), plus an app delegate hook | None; see [below](#background-transfers-transfers) |
| `http` | `HttpService::prefer_installed(fallback, configure)`: a `forge_net::HttpClient` that sends through the host once installed. Needs forge-services' `http` feature | `ForgeHTTPTransport` (`URLSession`) | None; see [below](#http-through-urlsession-http) |

**How selection works.** Each name adds a submodule to `crate::native` with its records, one host trait, a sink object and an `install_native_<name>` export. `generate-bindings.sh` copies the matching `crates/forge-services/swift/optional/ForgeServices+<Name>.swift` only when the bindings contain `installNative<Name>`, and writes `ForgeServices+Selected.swift` listing the installers, which `ForgeServices.install()` runs. An app that never names `location` has no Core Location code in its binary, which matters for App Store review. `forge config sync`, `forge config show` and every iOS build warn when the app selects `location`, `motion` or `bluetooth` but `forge.toml` lacks the matching `[ios.permissions]` text, because without it the service can only report `Unsupported` on the phone.

**Live values.** Every optional service delivers through a `forge_services::Feed`. The first `watch` starts the platform updates, each value goes to every Rust listener, and dropping the last `Watch` stops them. Listeners run on the host's thread (the main thread for location, a serial queue for motion), so copy the value into your state and return. Watches that need a permission fail with `PermissionDenied` until it is granted, so ask from a button first:

```rust
use crate::native::{location::*, NativePermission, NativeServices};

let native = NativeServices::installed()?;
if native.request_permission(NativePermission::Location).await.allows_use() {
    let location = LocationService::installed()?;
    let watch = location.watch(
        LocationOptions { accuracy: LocationAccuracy::Best, distance_filter_m: Some(5.0), include_heading: false },
        move |event| if let LocationEvent::Fix { fix } = event { /* store it */ },
    )?;
    // keep `watch`; drop it to stop
}

// Wi-Fi-only model downloads:
let network = connectivity::ConnectivityService::installed()?;
if network.current().allows(connectivity::TransferPolicy::Unmetered) { /* start */ }
```

**Tests.** Each service has `for_tests()`, returning the service and a fake you drive (`FakeConnectivity::set`, `FakeMotion::send`, `FakeLocation::send`, `FakeDevice::set`, `FakeBluetooth::set`). `crates/forge-services/tests/optional_services.rs` and `examples/device-lab/rust/src/tests.rs` use all of them.

### Audio (`audio`)

Recording and playing audio files. Chat's voice messages (M4) are its first user. Declare `microphone` under `[ios.permissions]` in `forge.toml` as well: recording without it is refused.

```rust
forge_services::native_services!(audio);
// Rust: ask for the microphone, and measure a file.
native.request_permission(NativePermission::Microphone).await;
let length = native::audio::AudioService::installed()?.duration(path)?;
```

Recording and playing are driven by the UI, so they are SwiftUI-side classes in the adapter:

```swift
// Record: asks for the microphone the first time; AAC .m4a, mono, about 240 KB a minute.
@State private var recorder = ForgeAudioRecorder(maxDuration: 300)
try await recorder.start()           // throws ForgeAudioError.microphoneDenied
recorder.elapsed; recorder.levels    // live time and level meter (0...1, 20 a second)
let recording = recorder.stop()      // nil if shorter than minDuration; recorder.cancel() discards
recorder.onFinish = { recording in } // the limit was reached or a call interrupted it

// Play: one file at a time across the app; pauses on interruptions and when headphones are unplugged.
try ForgeAudioPlayer.shared.toggle(url)
ForgeAudioPlayer.shared.progress(of: url)

// Draw and label it: computed off the main actor.
let bars = try await ForgeAudio.waveform(of: url, bars: 28)
let seconds = try ForgeAudio.duration(of: url)
```

`ForgeAudioSession` sets the iOS audio session for what is happening: `.record` (play-and-record through the speaker) or `.playback` (spoken audio, which plays with the ring switch on silent), and deactivates it afterwards so other apps' audio resumes. Real-time processing (effects, a render callback in Rust) is not part of this service; see spike S4 in the capability plan.

### Background tasks (`background`)

Work iOS runs while the app is in the background or closed, when it chooses (`BGTaskScheduler`). It works on a free team. Commonplace's hourly feed refresh is its first user.

1. List each task in `forge.toml`: `[ios] background_tasks = ["org.example.app.refresh"]`. Forge writes `BGTaskSchedulerPermittedIdentifiers` and adds the `fetch` and `processing` background modes.
2. Select the service: `forge_services::native_services!(background)`. `ForgeServices.install()` registers every listed task at launch, which iOS requires before launch ends.
3. Register a handler from Rust when the core opens. The two jobs apps repeat are built in, and each asks for its own next window:

```rust
use native::background::BackgroundService;

if let Ok(background) = BackgroundService::installed() {
    let core = Arc::downgrade(&core);
    // Fetch new content about hourly, so it is there when the app opens.
    background.register_refresh("org.example.app.refresh", 3600.0, move |run| {
        let core = core.clone();
        async move {
            let Some(core) = core.upgrade() else { return false };
            let cancellation = Cancellation::new();
            let stop = cancellation.clone();
            run.on_cancel(move || stop.cancel());           // iOS ending the window stops the work
            let work = core.rt.lifetime(Some(&*cancellation)); // not on-screen work
            core.refresh_in(work).await.is_ok()
        }
    })?;
}
```

| Job | Use |
|-----|-----|
| Fetch new content regularly (feeds, mail, weather) | `register_refresh(id, every_seconds, handler)`: a short window (about 30 seconds) |
| Upkeep: compact a database, rebuild an index, a backup, uploads | `register_processing(id, every_seconds, requires_external_power, requires_network, handler)`: minutes, usually overnight on power |
| Finish one job later | `register_handler(id, handler)`, then `schedule(BackgroundTaskRequest { .. })` when there is something to do |
| Stop a recurring task | `cancel(id)` |

A handler is async and returns whether it succeeded; each run gets its own thread. `run.on_cancel`, `run.is_cancelled()` and `run.cancelled()` tell it when iOS ends the window; a cancelled run reports failure. When iOS launches the app in the background for a task, `ForgeLifecycle` opens the core (no view will), and the run waits for its handler; it fails if the window ends first.

iOS decides whether and when a task runs, from how the person uses the app, battery and Low Power Mode. Nothing runs after the person force-quits the app, or while Background App Refresh is off for it (`submit` then fails with `PermissionDenied`). To try a task on a phone without waiting, pause the app in a debugger and run `e -l objc -- (void)[[BGTaskScheduler sharedScheduler] _simulateLaunchForTaskWithIdentifier:@"org.example.app.refresh"]`; that needs Xcode, so on Linux leave the app in the background and check the next day with `forge device logs`.

### Background transfers (`transfers`)

Downloads that keep going while the app is suspended, or after iOS has ended it, through a background `URLSession`. iOS may end the app while a transfer runs and relaunch it later, so the service is built around recovering from that. Rust keeps the transfer ledger and every rule; Swift only runs the session. Background transfers are separate from [`forge-net`](../crates/forge-net): use them for large files the person shouldn't have to wait on screen for (models, attachments, offline content), and `forge-net` for ordinary requests.

Enable the feature and select the service:

```toml
# rust/Cargo.toml
forge-services = { path = "...", features = ["transfers"] }
forge-storage = { path = "..." }
```

```rust
forge_services::native_services!(transfers);
use crate::native::transfers::*;

let transfers = TransferService::installed()?;
// At every launch: where files for the "models" key go. Files that finished
// while the app was closed are delivered now.
transfers.deliver_to("models", FileStore::new(app_support.join("models")));

let id = transfers.start(
    TransferRequest::new(url, "models", "qwen-0.5b.gguf")
        .expected_bytes(size)
        .sha256(hash)
        .unmetered_only(),
)?; // keep `id` with your own record

let _watch = transfers.watch(|update| { /* TransferUpdate::Changed(record) */ });
```

What the service guarantees:

- **The record is saved before the transfer starts** (`Application Support/ForgeTransfers/transfers.json`), so a relaunched app knows what it asked for.
- **The file leaves the system's temporary folder before the callback returns.** iOS deletes it afterwards, so `ForgeTransfers` moves it to the service's inbox first, then Rust checks the HTTP status, `max_bytes`, `expected_bytes` and `sha256`. A file that fails is deleted, never kept. A passing one goes into the registered `FileStore` under the requested name (`Delivered`), or waits verified in the inbox (`Downloaded`) until `deliver_to` registers its store.
- **iOS is told the events are handled only after Rust has handled them.** Every delegate callback calls into Rust synchronously on one serial queue, so the system's completion handler is called after all of them.
- **Recovery after a relaunch.** The session lists its tasks and Rust reconciles: a task the ledger doesn't know (an older install, a removed record) is cancelled, an active record whose task is gone becomes `Failed(Interrupted)`, and inbox files nobody waits for are deleted. A finished report that arrives after that still counts.
- **Force quit.** Swiping the app away makes iOS cancel its transfers; they become `Failed(Interrupted)`, not `Cancelled` (which only the app's own `cancel` produces).
- **Retrying is the app's rule.** `retry(id)` starts a failed, interrupted or cancelled transfer again under the same id (`attempts` counts them); space attempts with `forge_net::Backoff`. Background sessions already wait out lost connectivity themselves.

The app delegate has to pass iOS's completion handler on, or the app is not woken for finished transfers. In the `App`:

```swift
@UIApplicationDelegateAdaptor(TransfersAppDelegate.self) private var appDelegate

final class TransfersAppDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication,
                     handleEventsForBackgroundURLSession identifier: String,
                     completionHandler: @escaping () -> Void) {
        ForgeTransfers.handleEvents(forBackgroundURLSession: identifier,
                                    completionHandler: completionHandler)
    }
}
```

No background mode or entitlement is needed. Headers are saved in the ledger, so don't put a secret there that must not be on disk. Uploads are not supported yet.

**Tests.** `TransferService::for_tests(dir)` returns the service and a `FakeSession`; `fake.finish(&service, id, status, body)` plays a finished download. `crates/forge-services/tests/transfers.rs` covers saving before start, verification, delivery, orphans, interrupted transfers and force quit. Device Lab's Background Downloads screen is for the checks that need a phone: a relaunch after the system ends the app (its "End the app when I leave" switch calls `exit(0)` in the background, as Apple suggests for testing), the completion callback while suspended, a transfer finished while the app was closed, and force quit.

### HTTP through URLSession (`http`)

Ordinary requests (`send`, `send_cancellable`, `send_file`) through `URLSession` on iOS instead of Rust's own TLS connections, so they follow the system's proxy and VPN settings, use HTTP/2 and HTTP/3, respect Low Data Mode and use the system trust store. Code written against `forge_net::HttpClient` does not change:

```toml
forge-services = { path = "...", features = ["http"] }
```

```rust
forge_services::native_services!(http);

// URLSession once the host has installed its services, ureq before that
// and in Linux tests. Looked up per request, because the core is usually
// created before `ForgeServices.install()` runs.
let http = native::http::HttpService::prefer_installed(
    Arc::new(UreqClient::with_user_agent(USER_AGENT)),
    |client| client.max_body_bytes(8 * 1024 * 1024),
);
```

Cancelling a `CancelHandle` cancels the `URLSessionTask`. The response body is limited (8 MiB unless `configure` changes it): the adapter stops a response that announces or grows past the limit. Errors map to `NetError` the way ureq's do (`Offline`, `Certificate`, `Timeout`, `Cancelled`, `Transport`). Streams (`open_stream`, Server-Sent Events) still go through the fallback; they move in a later step. Commonplace fetches its feeds this way.

**Foreground only.** Apart from `transfers`, nothing here runs in the background. Background location needs the "always" permission and the `location` background mode, which no service asks for yet.

**What iOS does not give apps,** whatever the account: signal strength, carrier name (retired API), the ambient light sensor, MAC addresses. The Wi-Fi network name needs the paid-team Access Wi-Fi Information entitlement; the device's own name needs an entitlement Apple grants on request (apps see "iPhone"). [examples/device-lab](../examples/device-lab) shows all of it in one app.

## App configuration

The services need no `Info.plist` keys of their own. Notifications, Keychain, files and sharing work in a fresh app.

- **Custom URL schemes** (`myapp://...`) are declared in `forge.toml` under `[ios] url_schemes`, which writes `CFBundleURLTypes` ([app-config.md](app-config.md)).
- **Universal links** (`https://example.com/...`) need the `associated_domains` entitlement in `forge.toml` and an `apple-app-site-association` file on your domain. A free Apple ID cannot sign that entitlement.
- **Permissions that need a usage description** (camera, photos, location) go in `forge.toml` `[ios.permissions]` when you add their adapter. Asking for one without its description crashes the app.
- **The microphone** (`microphone` in `[ios.permissions]`) is needed by the optional `audio` service.
- **Networking from Rust and the local network.** `forge-net` (ureq) opens plain sockets, not `URLSession`, so App Transport Security doesn't apply: plain `http://` works without ATS exceptions. iOS's local network privacy permission does apply. The first connection to an address on the LAN (a device, or a development server on your laptop such as `http://192.168.1.20:8765`) shows the system prompt, and until the person allows it the connection fails as if nothing answered. Set `local_network` in `[ios.permissions]` (it writes `NSLocalNetworkUsageDescription`), and if a LAN connection fails, check Settings > Privacy & Security > Local Network before debugging ATS. Commonplace does this for its benchmark's fixture server.

## Adding a capability

Keep each capability small and iOS-first, behind an interface that Android can implement later. Do not build one cross-platform abstraction for everything.

1. **Decide which side drives it.** If Rust needs to call it (store a secret, schedule something), add a foreign trait or a method to the `native_services!` macro in `crates/forge-services/src/lib.rs`. If only the UI uses it (haptics, the share sheet, a picker), write a SwiftUI helper in `crates/forge-services/swift/` and pass data to Rust as records or paths.
2. **Put rules in plain Rust** in `crates/forge-services/src/` (as `link.rs` and `notify.rs` do), with unit tests.
3. **Write the Swift adapter** as `crates/forge-services/swift/ForgeServices+<Name>.swift`. Code that cannot work outside an app bundle should report `Unavailable` or `.unsupported` under `swift test`, as the notifier does.
4. **For a permission,** add a `NativePermission` case and a `ForgePermissionAdapter`, registered in `ForgePermissions.init` or, for an optional service, in its `ForgeServices.install<Name>()`. A capability that links a privacy-sensitive framework (AVFoundation, Photos, Core Location) is an optional service: add an arm to `__native_service!` in `crates/forge-services/src/optional.rs` and its adapter as `swift/optional/ForgeServices+<Name>.swift`, defining `ForgeServices.install<Name>()`. App Store processing flags apps that reference these APIs without a usage description. Check for the usage description before asking: without it iOS ignores the request (or ends the app, for Bluetooth).
5. **Bump `VERSION`** in `crates/forge-services/src/lib.rs`, and `forgeServicesSwiftVersion` with it, whenever a trait, record or enum changes shape.
6. **Test** in `crates/forge-services/tests/`, in the template's `Tests/AppHostTests/NativeServicesTests.swift` (macOS CI), and on a device with the runbook below.

## Android

The interfaces are platform-neutral, and UniFFI generates the Kotlin interfaces already (`SecureStore`, `LocalNotifier`, `PermissionProvider` in `uniffi.forge_ffi`). A later milestone adds Kotlin implementations: Android Keystore with encrypted preferences, `NotificationManagerCompat`, and runtime permissions. It also adds an `installNativeServices(...)` call in `ForgeAndroid.init`. Until then, Rust code should treat `NotInstalled` as "this platform does not have it yet".

## Checking on an iPhone

`swift test` cannot reach the notification centre, and the Keychain it reaches is the Mac's. So the device behaviour is checked by hand, from Linux with xtool, using a fresh `forge init` app. The steps are in [native-services-device-check.md](native-services-device-check.md).
