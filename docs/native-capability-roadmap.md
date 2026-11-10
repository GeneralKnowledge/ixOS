# Native capability roadmap

Proposed 2026-11-10, revised the same day after review. This page is the plan for the capabilities almost every serious iOS app eventually needs: web content and reading, documents, media, background work, identity, offline data, maps and charts. It says what ixOS already has, what is missing, and the order to fill the gaps. What each capability has actually proven stays in the [capability ledger](capability-ledger.md); this page is the plan, the ledger is the evidence.

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

### Pick the smallest shape that works

Each capability is one of three shapes. Most Apple APIs should not become Rust abstractions.

- **Service**: Rust needs to call it or hear from it. A `native_services!` trait or optional service in `forge-services`, with a Swift adapter and fakes for tests ([native-services.md](native-services.md#adding-a-capability)). Background transfers are a service, because Rust owns the files they produce.
- **Helper**: SwiftUI needs a native component and Rust doesn't. A Swift file in `crates/forge-services/swift/helpers/` (like `ForgeImages`), handing data to Rust as records or file paths. The web view is a helper.
- **Recipe**: Apple already does it well in SwiftUI. A documented pattern in `docs/`, no wrapper code at all.

Privacy-sensitive frameworks (camera, photos, microphone, location, anything App Store review asks a usage string for) are always opt-in optional services, so an app that doesn't select one has none of that code.

## How a capability is proven

There are two different things to prove, and they need different evidence:

1. **Does the wrapper integrate correctly with iOS?** A screen in the services check app (`scripts/make-services-check-app.sh`) proves this, with Rust tests and a device run.
2. **Does it make building an app easier?** Only a real app that adopts it proves this. Pocket Library and Chat are the main consumers; Good Dog, Weather and Device Lab adopt where they already need something.

A consumer app may be added when a capability's usability can't be judged in the apps we have. The one planned now is an **original feed reader**, written for ixOS, not a recreation of an existing app. It is the reader's second consumer (see Tier 1), and it exercises the web view, readable-text extraction, the HTTP cache and background refresh. It is added for that reason only, not to justify capabilities nobody needs.

Each capability lands in its own PR, and the ledger keeps **compiled**, **tested** and **device-verified** strictly apart. Before a capability's ledger row says it works on an iPhone, it needs all of:

- Rust tests with fakes, where Rust is involved.
- The Swift side compiled (macOS CI, or xtool with the Darwin SDK).
- An exercise in the services check app.
- At least one run on a physical iPhone, reported by a person. A wrapper that compiles on macOS is not evidence it works on the phone; this matters most for WebKit, the camera and background transfers.
- Adoption by a real app where practical, recorded in the ledger's "In an app" column.

The ledger record is updated in the same PR as the capability ([AGENTS.md rule 7](../AGENTS.md)).

## What exists and what is missing

Checked against `main` on 2026-11-10.

| Area | Already in ixOS | Missing |
|---|---|---|
| Web and reading | Pocket Library's EPUB reader: Rust parses chapters into styled text blocks, maps a position (chapter and fraction) to pages and saves it; SwiftUI lays it out with text size and serif settings and reads aloud. All of it is Pocket Library's own and EPUB-specific | Web view, a reader not tied to EPUB, readable-text extraction from web pages |
| Documents and files | File import (`forgeFileImporter`), share sheet and Save to Files (`ForgeShareLink`), managed file store (`FileStore`), app directories | PDF view, Quick Look preview, clipboard, receiving shared content (share extension) |
| Media | `ForgeImages` thumbnails and downscaled JPEGs, audio record and play (`audio`), `ForgeAudioSession`; Chat uses `PhotosPicker` directly | Photo picker helper, camera capture, video playback, camera and photos permissions |
| Notifications and background | Local notifications with grouping and tap-to-open; `Backoff`; Rust background work while the app runs; file-backed downloads in `forge-net` (foreground only) | Notification actions, background refresh and processing tasks, background transfers, a reusable durable task queue (Chat's outbox is Chat's own) |
| Identity and security | Keychain; permissions for notifications, location, motion, Bluetooth, microphone | Face ID / Touch ID, OAuth sign-in callback, token lifecycle, camera and photos permissions |
| App infrastructure | Custom URL scheme deep links, lifecycle phases, ForgeUI empty/loading/error states, ForgeAppKit destinations and alerts, saved settings (`forge_app::document`), haptics (SwiftUI, in Good Dog) | Navigation restoration, search history |
| Data and offline | SQLite with migrations and full-text search, JSON documents, connectivity, `Backoff` | HTTP cache with expiry and revalidation, generic upload and download queue, backup and export archive, conflict handling |
| Location and maps | Location fixes, headings and motion (`location`, `motion`) | Map view, geocoding and place search |
| Charts | Nothing | Swift Charts styled with ForgeUI tokens |
| Quieter foundations | Logging (Rust and Swift), crash reports on Linux (`forge crash`), data migrations | Localization, an analytics interface, feature configuration |

## Order

Ranked by how many apps each enables and how much repeated work it removes. "First consumer" is the real app that adopts it. Tier 1 runs alongside ForgeUI and ForgeAppKit (P1 and P2 in the [roadmap](roadmap.md#product-direction-a-native-application-framework)), because it is mostly `forge-services` work. Maps and charts are not started until Tier 1 is done.

### Tier 1: foundation

| # | Capability | Shape | Apple framework | First consumer |
|---|---|---|---|---|
| 1 | **Web view** (spec below) | Helper (`ForgeWebView`) | WebKit, SafariServices | Feed reader (showing the original article) |
| 2 | **Documents**: PDF view, Quick Look preview of any file in the file store | Helpers (`ForgePDFView`, `ForgeQuickLook`) | PDFKit, Quick Look | Chat (attachments); Pocket Library if it imports PDFs |
| 3 | **Native reader** (spec below) | Rust module plus Swift helper, extracted from Pocket Library | SwiftUI text layout, AVSpeechSynthesizer for read-aloud | Pocket Library, then the feed reader |
| 4 | **Photos and camera**: picker that returns files in the file store, camera capture, `photos` and `camera` permissions | Helper (picker, from Chat's code) and optional service `camera` | PhotosUI, AVFoundation | Chat (attachments) |
| 5 | **Background tasks**: Rust registers refresh and processing work, iOS runs it when it chooses, Rust gets the deadline as a `Cancellation` | Optional service `background` | BackgroundTasks | Feed reader (refresh), Chat (catch-up) |
| 6 | **Background transfers** (spec below) | Service, native side of `forge-net` downloads | Background `URLSession` | Local AI model downloads, Chat attachments, Pocket Library |
| 7 | **Notification actions** (reply, mark done) delivered to Rust | Extend `LocalNotifier` | UserNotifications | Chat, Good Dog reminders |
| 8 | **App lock with Face ID / Touch ID** | Optional service `biometrics` | LocalAuthentication | Pocket Library or Chat (opt-in lock) |
| 9 | **OAuth sign-in**: open the provider, return the callback URL to Rust; tokens kept in the Keychain | Helper plus a Rust token record on `SecureStore` | AuthenticationServices (`ASWebAuthenticationSession`) | First app that signs in to a service |

Background tasks and background transfers are separate capabilities with separate contracts. A task is a short slot of time iOS grants when it chooses; a transfer is work iOS carries on in another process while the app is suspended or closed.

#### Web view

`ForgeWebView` displays original web content. It is safe by default, and anything more powerful is an explicit opt-in.

- **Navigation:** loading progress, title, navigation errors as values the view can show with ForgeUI's error state, back and forward, and redirects followed only while they stay inside the allow-list.
- **Links:** hosts on the app's allow-list open in place; other web links open in Safari (or `SFSafariViewController`); other URL schemes (`mailto:`, `tel:`, app links) are refused unless the app lists them.
- **Downloads:** off by default. When enabled, the file goes into the app's `FileStore`, never straight to a path the page chooses.
- **Website data:** non-persistent by default (no cookies or storage kept between launches); an app opts into a persistent store when it needs sign-in to stick.
- **JavaScript:** on, because most pages need it, but with **no JavaScript-to-Rust or JavaScript-to-Swift bridge by default**. A message bridge is an opt-in with a named handler and typed messages, never arbitrary calls.
- **Lifecycle and memory:** the `WKWebView` is released when the view goes away, a web content process crash reloads or shows an error rather than a blank view, and one screen never holds more than one live web view.

#### Native reader

ForgeReader is the reusable reading experience, extracted from Pocket Library when the feed reader becomes its second consumer. It is **not coupled to EPUB or feeds**: formats are inputs to it. Its responsibilities, which need not each be a separate module:

| Responsibility | What it is | Side |
|---|---|---|
| Reader document | Generic readable content: sections of styled text blocks (headings, paragraphs, quotes, lists, images by file reference), with metadata | Rust |
| Reading position | Section and fraction, saved and restored, mapped to pages or progress | Rust |
| Reader view | The SwiftUI reading interface: scrolling per section, contents, read-aloud | Swift helper |
| Reader theme | Typography, spacing, text size and appearance, from ForgeUI tokens | Swift helper |

Each format converts into a reader document: Pocket Library's EPUB parser does now, and readable-text extraction (Tier 2) converts web pages for the feed reader. `ForgeWebView` stays separate: it shows the original page, the reader shows the extracted text.

#### Background transfers

A Rust HTTP request and an iOS background `URLSession` transfer are not interchangeable: iOS may suspend or end the app while the transfer goes on, and relaunch it afterwards. The service is defined around recovering from that:

- **Persistent transfer ids**, stored by Rust before the transfer starts, so a relaunched app knows what it asked for.
- **Destination and ownership:** the finished file is moved into the app's `FileStore` under the name Rust chose; nothing is left in system temporary folders.
- **Progress** reported to Rust while the app is running.
- **Completion and failure recovery** after a relaunch: results that arrived while the app was closed are delivered on the next launch, and failures are retried with `Backoff` where the app allows.
- **Cancellation** by id, from Rust.
- **Verification before keeping a file:** size limit and, where known, the SHA-256 (`FileStore`'s `hash` feature); a file that fails is deleted, never kept.

This is spike S6 in the capability plan.

### Tier 2: application data

| # | Capability | Shape | Notes |
|---|---|---|---|
| 10 | **Readable text from a web page** into a reader document | Rust, alongside the reader | Feeds the reader for the feed reader app |
| 11 | **HTTP cache** with expiry, ETag and Last-Modified revalidation | Rust module in `forge-net` | Feed reader, Weather, Pocket Library covers |
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

## Backlog

Recorded so they aren't forgotten, not scheduled ahead of the tiers above.

| Capability | Why | Notes |
|---|---|---|
| **Native text editing** | Notes, messaging, documents and composition: creating content, not only reading it | The most likely to be promoted. `TextEditor` and `AttributedString` first; a `UITextView` helper only if rich text needs it |
| Keyboard and input handling | Search, forms, editors and hardware keyboard shortcuts | Mostly recipes (`@FocusState`, `.keyboardShortcut`, toolbar above the keyboard) |
| Haptics | Feedback throughout an app | Already works through SwiftUI's `sensoryFeedback` (Good Dog); needs a recipe, not a wrapper |
| Widgets and Live Activities | Timers, tracking, reminders and status outside the app | Needs an app extension (spike S2); each extension uses a free-team App ID |
| Accessibility helpers | Consistent VoiceOver and Dynamic Type behaviour | Partly in ForgeUI and the `forge ui` audits; add helpers where apps repeat the same fixes |
| App extensions | Share extension (receiving shared content) and other system integrations | xtool supports extensions; unproven in ixOS |
| In-app purchases | Subscriptions and paid apps | StoreKit; needs the paid team and App Store Connect |

## Not planned

- **Push notifications, Sign in with Apple, universal links.** All need the paid Apple Developer Program, and APNs is a non-goal since 2026-11-05. Revisit when the team is paid ([ios-signing.md](ios-signing.md)).
- **A navigation framework, a charting engine, a map renderer or a browser engine.** SwiftUI, Swift Charts, MapKit and WebKit already exist.
- **An unrestricted JavaScript bridge.** See the web view spec.
- **Generic sync and conflict resolution.** No app has a server that needs it yet; Chat's "server order wins" stands.
- **Unrestricted background execution.** iOS decides when background tasks run. ixOS integrates with that, and doesn't pretend otherwise.
- **Demo apps made only to justify a capability.** A consumer app is added only when usability can't be judged in the apps we have, as with the feed reader.
