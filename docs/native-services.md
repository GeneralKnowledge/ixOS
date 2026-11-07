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

Every app from `forge init` has these services installed. Android implementations are not written yet. On Android, `NativeServices::installed()` returns `NotInstalled` (see [Android](#android)).

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

## App configuration

The services need no `Info.plist` keys of their own. Notifications, Keychain, files and sharing work in a fresh app.

- **Custom URL schemes** (`myapp://...`) are declared in `forge.toml` under `[ios] url_schemes`, which writes `CFBundleURLTypes` ([app-config.md](app-config.md)).
- **Universal links** (`https://example.com/...`) need the `associated_domains` entitlement in `forge.toml` and an `apple-app-site-association` file on your domain. A free Apple ID cannot sign that entitlement.
- **Permissions that need a usage description** (camera, photos, location) go in `forge.toml` `[ios.permissions]` when you add their adapter. Asking for one without its description crashes the app.

## Adding a capability

Keep each capability small and iOS-first, behind an interface that Android can implement later. Do not build one cross-platform abstraction for everything.

1. **Decide which side drives it.** If Rust needs to call it (store a secret, schedule something), add a foreign trait or a method to the `native_services!` macro in `crates/forge-services/src/lib.rs`. If only the UI uses it (haptics, the share sheet, a picker), write a SwiftUI helper in `crates/forge-services/swift/` and pass data to Rust as records or paths.
2. **Put rules in plain Rust** in `crates/forge-services/src/` (as `link.rs` and `notify.rs` do), with unit tests.
3. **Write the Swift adapter** as `crates/forge-services/swift/ForgeServices+<Name>.swift`. Code that cannot work outside an app bundle should report `Unavailable` or `.unsupported` under `swift test`, as the notifier does.
4. **For a permission,** add a `NativePermission` case and a `ForgePermissionAdapter`, registered in `ForgePermissions.init` or by the app (`ForgePermissions.shared.register(_:for:)`). Put adapters that link privacy-sensitive frameworks (AVFoundation, Photos, CoreLocation) in separate files that apps opt into. App Store processing flags apps that reference these APIs without a usage description.
5. **Bump `VERSION`** in `crates/forge-services/src/lib.rs`, and `forgeServicesSwiftVersion` with it, whenever a trait, record or enum changes shape.
6. **Test** in `crates/forge-services/tests/`, in the template's `Tests/AppHostTests/NativeServicesTests.swift` (macOS CI), and on a device with the runbook below.

## Android

The interfaces are platform-neutral, and UniFFI generates the Kotlin interfaces already (`SecureStore`, `LocalNotifier`, `PermissionProvider` in `uniffi.forge_ffi`). A later milestone adds Kotlin implementations: Android Keystore with encrypted preferences, `NotificationManagerCompat`, and runtime permissions. It also adds an `installNativeServices(...)` call in `ForgeAndroid.init`. Until then, Rust code should treat `NotInstalled` as "this platform does not have it yet".

## Checking on an iPhone

`swift test` cannot reach the notification centre, and the Keychain it reaches is the Mac's. So the device behaviour is checked by hand, from Linux with xtool, using a fresh `forge init` app. The steps are in [native-services-device-check.md](native-services-device-check.md).
