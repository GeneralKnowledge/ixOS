# Native capability roadmap

Proposed 2026-11-10. This page is the plan for the capabilities almost every serious iOS app eventually needs: documents, media, background work, identity, offline data, maps and charts. It says what ixOS already has, what is missing, and the order to fill the gaps. What each capability has actually proven stays in the [capability ledger](capability-ledger.md); this page is the plan, the ledger is the evidence.

## The principle: wrap Apple's frameworks, don't replace them

ixOS gives apps native platform capabilities. It does not reinvent the platform.

| Area | Apple framework ixOS wraps |
|---|---|
| Web content | WebKit (`WKWebView`), SafariServices |
| Documents and files | PDFKit, Quick Look, `fileImporter` and `UIDocumentPicker`, `ShareLink` |
| Media | PhotosUI, AVFoundation, AVKit, ImageIO |
| Background work | BackgroundTasks (`BGTaskScheduler`), background `URLSession` |
| Notifications | UserNotifications |
| Identity and security | Security (Keychain), LocalAuthentication, AuthenticationServices |
| Location and maps | Core Location, MapKit |
| Charts | Swift Charts |

Rust owns the portable parts: domain rules, data processing, queues, retry decisions and state. Swift owns the iOS integration and the presentation. ixOS never ships its own authentication cryptography, credential storage, charting engine, map renderer or browser engine.

Four rules keep this small:

1. **No new apps to justify a capability.** A wrapper over an Apple framework is platform-level code, so [architecture.md](architecture.md#framework-principles) lets it move into ixOS on its first use. It is proven in the services check app (`scripts/make-services-check-app.sh`) and adopted by an existing app where one already needs it.
2. **Pick the smallest shape that works.** Each capability is one of:
   - **Service**: Rust needs to call it or hear from it. A `native_services!` trait or optional service in `forge-services`, with a Swift adapter and fakes for tests ([native-services.md](native-services.md#adding-a-capability)).
   - **Helper**: only the UI uses it. A Swift file in `crates/forge-services/swift/helpers/` (like `ForgeImages`), handing data to Rust as records or file paths.
   - **Recipe**: SwiftUI already does it well. A documented pattern in `docs/`, no wrapper code at all.
3. **Privacy-sensitive frameworks are opt-in.** Camera, photos, microphone, location and anything App Store review asks a usage string for is an optional service, so an app that doesn't select it has none of that code.
4. **One PR per capability, with its ledger row.** Rust tests with fakes, the Swift adapter compiled by macOS CI, a section in the services check app, and the ledger record updated in the same PR. The iPhone column changes only when a person runs it on a phone.

## What exists and what is missing

Checked against `main` on 2026-11-10.

| Area | Already in ixOS | Missing |
|---|---|---|
| Web and reader | Pocket Library's EPUB reader (Rust chapters as styled text blocks, SwiftUI layout) | Web view, readable-text extraction from web pages |
| Documents and files | File import (`forgeFileImporter`), share sheet and Save to Files (`ForgeShareLink`), managed file store (`FileStore`), app directories | PDF view, Quick Look preview, clipboard, receiving shared content (share extension) |
| Media | `ForgeImages` thumbnails and downscaled JPEGs, audio record and play (`audio`), `ForgeAudioSession`; Chat uses `PhotosPicker` directly | Photo picker helper, camera capture, video playback, camera and photos permissions |
| Notifications and background | Local notifications with grouping and tap-to-open; `Backoff`; Rust background work while the app runs | Notification actions, background refresh and processing tasks, background transfers, a reusable durable task queue (Chat's outbox is Chat's own) |
| Identity and security | Keychain; permissions for notifications, location, motion, Bluetooth, microphone | Face ID / Touch ID, OAuth sign-in callback, token lifecycle, camera and photos permissions |
| App infrastructure | Custom URL scheme deep links, lifecycle phases, ForgeUI empty/loading/error states, ForgeAppKit destinations and alerts, saved settings (`forge_app::document`) | Navigation restoration, search history |
| Data and offline | SQLite with migrations and full-text search, JSON documents, connectivity, `Backoff`, file-backed downloads | HTTP cache with expiry and revalidation, generic upload and download queue, backup and export archive, conflict handling |
| Location and maps | Location fixes, headings and motion (`location`, `motion`) | Map view, geocoding and place search |
| Charts | Nothing | Swift Charts styled with ForgeUI tokens |
| Quieter foundations | Logging (Rust and Swift), crash reports on Linux (`forge crash`), data migrations | Localization, an analytics interface, feature configuration |

## Order

Ranked by how many apps each enables and how much repeated work it removes. "Likely first user" is an existing app that would adopt the capability; where none needs it yet, the services check app is the only proof until one does.

Tier 1 runs alongside ForgeUI and ForgeAppKit (P1, P2 in the [roadmap](roadmap.md#product-direction-a-native-application-framework)), because it is `forge-services` work and doesn't touch them.

### Tier 1: foundation

| # | Capability | Shape | Apple framework | Likely first user |
|---|---|---|---|---|
| 1 | **Web view**: show a page, keep links inside an allow-list, open the rest in Safari, report title and load state | Helper (`ForgeWebView`) | WebKit, SafariServices | None yet; the services check app proves it |
| 2 | **Documents**: PDF view, Quick Look preview of any file in the file store | Helpers (`ForgePDFView`, `ForgeQuickLook`) | PDFKit, Quick Look | Chat (previewing attachments); Pocket Library if it imports PDFs |
| 3 | **Clipboard** and **video playback** | Recipes | SwiftUI `PasteButton`, `UIPasteboard`, AVKit `VideoPlayer` | Any |
| 4 | **Photos and camera**: picker that returns files in the file store, camera capture, `photos` and `camera` permissions | Helper (picker, from Chat's code) and optional service `camera` | PhotosUI, AVFoundation | Chat (attachments) |
| 5 | **Background tasks**: Rust registers refresh and processing work, iOS runs it, Rust gets a deadline as a `Cancellation` | Optional service `background` | BackgroundTasks | Chat (catching up on messages) |
| 6 | **Background transfers** that survive the app being suspended or closed | Native side of `forge-net` downloads | Background `URLSession` | Local AI model downloads |
| 7 | **Notification actions** (reply, mark done) delivered to Rust | Extend `LocalNotifier` | UserNotifications | Chat, Good Dog reminders |
| 8 | **App lock with Face ID / Touch ID** | Optional service `biometrics` | LocalAuthentication | Pocket Library or Chat (opt-in lock) |
| 9 | **OAuth sign-in**: open the provider, return the callback URL to Rust; tokens kept in the Keychain | Helper plus a Rust token record on `SecureStore` | AuthenticationServices (`ASWebAuthenticationSession`) | First app that signs in to a service |

### Tier 2: application data

| # | Capability | Shape | Notes |
|---|---|---|---|
| 10 | **HTTP cache** with expiry, ETag and Last-Modified revalidation | Rust module in `forge-net` | Weather and Pocket Library covers already fetch the same things repeatedly |
| 11 | **Readable text from a web page** into the reader's text blocks | Rust, alongside the EPUB parser | Extract the reader into a module when the second reader arrives |
| 12 | **Durable work queue** (stored before the network call, retried with `Backoff`, recovered after termination) | Rust, from Chat's outbox | Extract on the second user, per the [extraction audit](framework-extraction-audit.md) |
| 13 | **Backup and export archive** of an app's data | Rust (`forge-storage`) plus `ShareLink` | Already a "No" row in the ledger |
| 14 | **Search history** and **navigation restoration** | ForgeAppKit | Stored through `forge_app::document` |

### Tier 3: optional

| # | Capability | Shape | Notes |
|---|---|---|---|
| 15 | **Maps** with annotations, **geocoding** and place search | Recipe for `Map`; geocoding added to the `location` service | MapKit has no offline tiles |
| 16 | **Charts**: line, bar, area, pie and point marks styled from ForgeUI tokens | Recipe plus a small style helper | Swift Charts; no charting engine |
| 17 | **Localization** | Spike first | String Catalogs (`.xcstrings`) are compiled by Xcode; check what xtool on Linux can do |
| 18 | **Analytics interface** and **feature configuration** | Rust traits with no vendor | Only once an app ships to people other than its developer |

## Not planned

- **Push notifications, Sign in with Apple, universal links.** All need the paid Apple Developer Program, and APNs is a non-goal since 2026-11-05. Revisit when the team is paid ([ios-signing.md](ios-signing.md)).
- **A navigation framework, a charting engine, a map renderer or a browser engine.** SwiftUI, Swift Charts, MapKit and WebKit already exist.
- **Generic sync and conflict resolution.** No app has a server that needs it yet; Chat's "server order wins" stands.
- **Unrestricted background execution.** iOS decides when background tasks run. ixOS integrates with that, and doesn't pretend otherwise.
- **New demo apps to exercise this list.** Capabilities are proven in the services check app and adopted by apps that already exist.
