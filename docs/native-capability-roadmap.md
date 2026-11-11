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

**A capability doesn't need a Rust interface just because it is in ixOS.** App Intents, widgets, share extensions and many other system features are native Swift. Rust takes part only where there is real domain logic or persistent state to share, and then through the same records and stores the app already uses.

Privacy is decided by what the API reaches, not by which framework it is in:

- **Protected resources and broad access** (camera capture, microphone, location, the whole photo library, contacts or calendar in bulk, anything that needs a usage string and a permission prompt) are opt-in optional services, so an app that doesn't select one has none of that code.
- **User-mediated system pickers** (the photo picker, the document picker, the contact picker, the event editor) are ordinary helpers. The person chooses what to hand over, so they need no permission, and ixOS never asks for one on their behalf.

## How a capability is proven

There are two different things to prove, and they need different evidence:

1. **Does the wrapper integrate correctly with iOS?** A screen in the services check app (`scripts/make-services-check-app.sh`) proves this, with Rust tests and a device run.
2. **Does it make building an app easier?** Only a real app that adopts it proves this. Pocket Library and Chat are the main consumers; Good Dog, Weather and Device Lab adopt where they already need something.

A consumer app may be added when a capability's usability can't be judged in the apps we have. The one planned now is a **feed reader benchmarked against [NetNewsWire](https://github.com/Ranchero-Software/NetNewsWire)**, the established open-source (MIT) reader for Apple platforms. NetNewsWire is the external reference: we compare navigation, article presentation, reader behaviour, refresh, search and interaction against it, which is a more demanding test than a reader whose features we choose ourselves. It is built on ixOS from scratch (no NetNewsWire code is copied in) and ships under its own name and icon, because NetNewsWire's are its authors'. It is the reader's second consumer (see Tier 1), with a different implementation and UX from Pocket Library, and it exercises the web view, readable-text extraction, the HTTP cache and background refresh. Feature parity is not the goal: a gap is recorded as a finding about ixOS, not a reason to add features.

Each capability lands in its own PR, and the ledger keeps **compiled**, **tested** and **device-verified** strictly apart. Before a capability's ledger row says it works on an iPhone, it needs all of:

- Rust tests with fakes, where Rust is involved.
- The Swift side compiled (macOS CI, or xtool with the Darwin SDK).
- An exercise in the services check app.
- At least one run on a physical iPhone, reported by a person. A wrapper that compiles on macOS is not evidence it works on the phone; this matters most for WebKit, the camera and background transfers.
- Adoption by a real app where practical, recorded in the ledger's "In an app" column.

The ledger record is updated in the same PR as the capability ([AGENTS.md rule 7](../AGENTS.md)).

## Where current code departs from native first

The native-first rule is in [architecture.md](architecture.md#framework-principles). Reviewed 2026-11-10: the architecture is close to it, so this is a short list of corrections, not a refactor. In priority order:

1. **Platform limits checked in portable Rust.** `crates/forge-services/src/notify.rs` rejects repeating notifications under `MIN_REPEAT_SECONDS = 60` ("the minimum iOS accepts"), and `src/sensors.rs` rejects motion intervals under 10 ms ("the most the iPhone's motion coprocessor delivers"). Rust keeps the general checks (a repeat interval above zero, a motion interval in a sane range); the iOS adapters check the iOS limits and return `NativeServiceError` with a clear message.
2. **iOS networking in Rust.** `forge-net`'s `UreqClient` (ureq and Rustls) is the HTTP transport on iOS. Its request types, `Backoff`, cancellation and byte limits are portable and stay. **Decided 2026-11-10:** iOS gets a native `URLSession` transport behind the existing `HttpClient` trait, starting with background transfers (Tier 1), which need it in any case. Ordinary requests stay on `UreqClient` until that transport has proven itself on an iPhone; then we judge whether to move all iOS traffic onto it. `UreqClient` remains the transport for Linux tests and Android. **Plan (2026-11-11), one PR each:** (1) background downloads, the `transfers` service (built; phone checks owed); (2) a `URLSession` client behind `HttpClient` for ordinary requests, opt-in, Pocket Library's catalogue search first; (3) streaming (Chat's event stream) on it, then, once (2) has passed on a phone, `URLSession` by default on iOS and no ureq or Rustls in iOS builds.
3. **The reader lives in Pocket Library.** `examples/pocket-library/rust/src/reader.rs` is EPUB-specific and `ReaderView.swift` holds the reading experience, so another app would copy them. Positions are chapter-and-fraction (block indexes inside a chapter), which drift when content changes. Fixed by the ForgeReader extraction with block anchors (Tier 1), when the second consumer arrives.
4. **Chat duplicates file helpers.** `examples/chat/Sources/AppHost/Attachments.swift` does its own security-scoped copy in `prepareDocument`, which `ForgeFiles.importCopy` already does. Its photo and thumbnail code already calls `ForgeImages`. Chat moves the document copy onto `ForgeFiles` and keeps its naming, staging and MIME rules. This is the smallest item.

Swift presentation models such as Pocket Library's `LibraryModel` and Chat's `ChatModel` were checked and are fine: they present the domain, and Rust owns it. Everything else is left alone unless building an app exposes a problem.

## What exists and what is missing

Checked against `main` on 2026-11-10.

| Area | Already in ixOS | Missing |
|---|---|---|
| Web and reading | Pocket Library's EPUB reader: Rust parses chapters into styled text blocks, maps a position (chapter and fraction) to pages and saves it; SwiftUI lays it out with text size and serif settings and reads aloud. All of it is Pocket Library's own and EPUB-specific | Web view, a reader not tied to EPUB, readable-text extraction from web pages |
| Documents and files | File import (`forgeFileImporter`), share sheet and Save to Files (`ForgeShareLink`), managed file store (`FileStore`), app directories | PDF view, Quick Look preview, clipboard, receiving shared content (share extension) |
| Media | `ForgeImages` thumbnails and downscaled JPEGs, audio record and play (`audio`), `ForgeAudioSession`; Chat uses `PhotosPicker` directly | Photo picker helper, camera capture, video playback, camera permission |
| Notifications and background | Local notifications with grouping and tap-to-open; `Backoff`; Rust background work while the app runs; file-backed downloads in `forge-net` (foreground only) | Notification actions, background refresh and processing tasks, background transfers, a reusable durable task queue (Chat's outbox is Chat's own) |
| Identity and security | Keychain; permissions for notifications, location, motion, Bluetooth, microphone | Face ID / Touch ID, OAuth sign-in callback, token lifecycle, camera permission |
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
| 4 | **Photos and camera**: the system photo picker returning the chosen items as files in the file store, and camera capture with its `camera` permission | Helper (picker, from Chat's code, no permission) and optional service `camera` | PhotosUI, AVFoundation | Chat (attachments) |
| 5 | **Background tasks** (contract below): Swift registers them at launch, Rust does the work in the window iOS grants | Optional service `background` | BackgroundTasks | Feed reader (refresh), Chat (catch-up) |
| 6 | **Background transfers** (spec below) | Service, native side of `forge-net` downloads | Background `URLSession` | Local AI model downloads, Chat attachments, Pocket Library |
| 7 | **Notification actions** (reply, mark done) delivered to Rust | Extend `LocalNotifier` | UserNotifications | Chat, Good Dog reminders |
| 8 | **App lock with Face ID / Touch ID** | Optional service `biometrics` | LocalAuthentication | Pocket Library or Chat (opt-in lock) |
| 9 | **OAuth sign-in**: open the provider, return the callback URL to Rust; tokens kept in the Keychain | Helper plus a Rust token record on `SecureStore` | AuthenticationServices (`ASWebAuthenticationSession`) | First app that signs in to a service |
| 10 | **Universal links**: `https://` links that open the app, routed through the existing deep-link path (Swift receives, Rust resolves the destination) | `forge.toml` `associated_domains` plus the existing deep-link service; a recipe for the `apple-app-site-association` file | Associated Domains | Pocket Library (shared book links). **Needs the paid team**: built and unit-tested now, proven on a phone after the switch |
| 11 | **Receiving shared content**: URLs, text, images and PDFs from other apps' share sheets, handed to the app | Share extension template plus a helper; Rust sees only an inbox of files it drains | Share extension, Uniform Type Identifiers, App Groups | Pocket Library (save a link or book), the feed reader (add a feed). Spike first: whether a free team can sign an App Group (S2) |
| 12 | **App Intents and Shortcuts**: a few app actions available to Siri, Shortcuts and Spotlight | Swift (`AppIntent` types in the app), calling the app's Rust core | App Intents | Pocket Library ("log reading"), Good Dog ("start a session"). Spike first: App Shortcuts need build-time metadata Xcode generates; check what xtool on Linux produces |

Items 10 to 12 make an app reachable from outside itself: from the web, from other apps and from the system. They come after the first nine and are not started until those are under way.

Background tasks and background transfers are separate capabilities with separate contracts. A task is a short slot of time iOS grants when it chooses; a transfer is work iOS carries on in another process while the app is suspended or closed.

#### Photos and camera

The system photo picker (`PhotosPicker`) runs outside the app and hands over only the items the person chooses, so it needs **no photo library permission** and ixOS never asks for one to pick images. Full or limited library access (`NSPhotoLibraryUsageDescription`) is only for an app that browses or changes the library itself, which no planned consumer does. The camera is different: capture needs the `camera` permission and its usage string, so it is an optional service.

#### Background tasks

`BGTaskScheduler` requires every task identifier to be registered by the app during launch, before it finishes launching. So ownership is split:

- **Swift** registers the permitted identifiers at launch (listed in `forge.toml`, which writes `BGTaskSchedulerPermittedIdentifiers`), schedules requests, and receives each task from iOS.
- **Rust** provides the work handler for each identifier and its cancellation contract.
- **Swift** forwards the granted window to Rust: it starts the handler and turns the task's expiration into a `Cancellation`.
- **Rust** reports completion (success or failure) back for `setTaskCompleted`, and persists unfinished work so the next window, or the next launch, continues it.

#### Web view

`ForgeWebView` displays original web content. It is a **constrained web view, not a sandbox for arbitrary untrusted websites**: its defaults are restrictive, and anything more powerful is an explicit opt-in, but an allowed page is still a web page running its own code. Its five security boundaries (top-level navigation, subresources, external URL schemes, JavaScript execution and the native message bridge) are separate settings, because one doesn't imply another:

- **Navigation state:** loading progress, title, navigation errors as values the view can show with ForgeUI's error state, back and forward, and redirects followed only while they stay inside the allow-list.
- **Top-level navigation:** hosts on the app's allow-list open in place; other web links open in Safari (or `SFSafariViewController`). This controls where the view goes, not what a page loads.
- **Subresources:** the allow-list does **not** restrict the scripts, images, styles and frames an allowed page loads from other hosts. An app that needs that restricted opts into a content rule list (`WKContentRuleList`) that blocks loads outside named hosts; the default is WebKit's normal loading.
- **External URL schemes:** `mailto:`, `tel:`, app links and other non-web schemes are refused unless the app lists them, and listed ones are handed to the system, never loaded in the view.
- **Downloads:** off by default. When enabled, the file goes into the app's `FileStore`, never straight to a path the page chooses.
- **Website data:** non-persistent by default (no cookies or storage kept between launches); an app opts into a persistent store when it needs sign-in to stick.
- **JavaScript execution:** on, because most pages need it; an app can turn it off for content it only displays.
- **Native message bridge:** **none by default**, so page script can't reach Swift or Rust. A bridge is an opt-in with a named handler, typed messages and the hosts allowed to send them, never arbitrary calls.
- **Lifecycle and memory:** the `WKWebView` is released when the view goes away, a web content process crash reloads or shows an error rather than a blank view, and one screen never holds more than one live web view.

#### Native reader

ForgeReader is the reusable reading experience, extracted from Pocket Library when the feed reader becomes its second consumer. It is **not coupled to EPUB or feeds**: formats are inputs to it. Its responsibilities, which need not each be a separate module:

| Responsibility | What it is | Side |
|---|---|---|
| Reader document | Generic readable content: sections of styled text blocks (headings, paragraphs, quotes, lists, images by file reference), with metadata | Rust |
| Reading position | A stable anchor: document id, section id, block id, and the relative position within that block. Saved and restored, mapped to pages or progress | Rust |
| Reader view | The SwiftUI reading interface: scrolling per section, contents, read-aloud | Swift helper |
| Reader theme | Typography, spacing, text size and appearance, from ForgeUI tokens | Swift helper |

Block ids are what make positions stable, so they never come from a block's place in the document: an index changes when content is inserted earlier, and an old position would then point at a different paragraph. A block's id is, in order of preference:

1. **The source's own id** where it has one (an EPUB element `id`, an HTML `id` or anchor).
2. Otherwise **derived from the block's content**: a hash of its normalised text (and kind), with an occurrence number when the same text appears more than once in the section, so repeated short blocks such as "* * *" still get distinct ids.

With ids like these, a position survives a change of text size or layout, and survives an article being edited as long as its block still exists. An edited block gets a new content id, so a saved anchor can stop matching. Recovery then tries, in order: the same id; a block in the same section whose text closely matches the saved block's opening words (the anchor stores them); the nearest surviving neighbour, using the saved ids of the blocks before and after; the section's start. Pocket Library saves chapter and fraction today; extracting the reader converts those positions to anchors once.

Each format converts into a reader document: Pocket Library's EPUB parser does now, and readable-text extraction (Tier 2) converts web pages for the feed reader. `ForgeWebView` stays separate: it shows the original page, the reader shows the extracted text.

#### Background transfers

A Rust HTTP request and an iOS background `URLSession` transfer are not interchangeable: iOS may suspend or end the app while the transfer goes on, and relaunch it afterwards. The service is defined around recovering from that:

- **Persistent transfer ids**, stored by Rust before the transfer starts, so a relaunched app knows what it asked for.
- **Destination and ownership:** the finished file is moved into the app's `FileStore` under the name Rust chose; nothing is left in system temporary folders.
- **Progress** reported to Rust while the app is running.
- **Completion and failure recovery** after a relaunch: results that arrived while the app was closed are delivered on the next launch, and failures are retried with `Backoff` where the app allows.
- **Cancellation** by id, from Rust.
- **Verification before keeping a file:** size limit and, where known, the SHA-256 (`FileStore`'s `hash` feature); a file that fails is deleted, never kept.

**Acceptance tests.** Background `URLSession` behaviour is where an implementation can pass ordinary testing and still fail on an iPhone, so the capability is not marked working until each of these has passed on a physical phone:

1. **Reattach after the system ends the app:** start a transfer, send the app to the background and let it be terminated the way iOS does it while suspended, then relaunch; the app recreates the session with the same identifier, and the session reports the transfer's state (still running with its progress, or finished). A development build simulates the system's termination by calling `exit(0)` from a devtools command while in the background, as Apple suggests for testing background sessions. **Force-quitting from the app switcher is not this test:** it is a different event (see 6).
2. **Session completion callback:** with the app suspended, let a transfer finish; iOS wakes the app through `application(_:handleEventsForBackgroundURLSession:completionHandler:)`, the app processes the events, and calls the stored completion handler only after `urlSessionDidFinishEvents`.
3. **Completed while terminated:** a transfer that finished while the app was not running (ended by the system as in 1, not force-quit) is reported to Rust as complete on the next launch, with its file in place.
4. **File moved before the handler is released:** the temporary download file is moved into `FileStore` inside `didFinishDownloadingTo`, before that method returns, because iOS deletes it afterwards; verification and Rust's record update happen after the move, and the system completion handler is called only after all of it.
5. **Orphans:** a transfer the session reports but Rust has no record of (deleted, or from an older install) is cancelled and its file removed; a Rust record whose transfer the session no longer has is marked failed and retried or dropped by the app's rule.
6. **Force quit:** swiping the app away in the app switcher cancels its background transfers and stops iOS relaunching it for session events until the person opens it again. The expected behaviour is documented and tested, not avoided: on the next launch the session reports those transfers as cancelled, Rust marks their records interrupted, and the app retries them (or not) by its own rule, never treating a partial file as complete.

Items 1, 2, 3 and 6 need the device; 4 and 5 also have unit tests against a fake session. This is spike S6 in the capability plan.

**Status (2026-11-11):** downloads are built as the optional service `native_services!(transfers)` ([native-services.md](native-services.md#background-transfers-transfers)), with unit tests for 4 and 5 and Device Lab's Background Downloads screen for 1, 2, 3 and 6, which are still to be run on a phone. Uploads (Chat attachments) come next.

### Tier 2: application data and system reach

| # | Capability | Shape | Notes |
|---|---|---|---|
| 13 | **Readable text from a web page** into a reader document | Rust, alongside the reader | Feeds the reader for the feed reader |
| 14 | **HTTP cache** with expiry, ETag and Last-Modified revalidation | Rust module in `forge-net` | Feed reader, Weather, Pocket Library covers |
| 15 | **Durable work queue** (stored before the network call, retried with `Backoff`, recovered after termination) | Rust, from Chat's outbox | Extract on the second user, per the [extraction audit](framework-extraction-audit.md) |
| 16 | **Backup and export archive** of an app's data | Rust (`forge-storage`) plus `ShareLink` | Already a "No" row in the ledger |
| 17 | **Search history** and **navigation restoration** | ForgeAppKit | Stored through `forge_app::document` |
| 18 | **Vision**: text recognition (OCR), document scanning, barcode and QR detection, live scanning | Optional service for still images (Rust gets text and codes); helper for the live scanner | Vision, VisionKit (`DataScannerViewController`, `VNDocumentCameraViewController`). Investigate before writing any image processing of our own. Live scanning needs the `camera` permission |
| 19 | **Contacts and Calendar**: pick a contact, read the chosen details, add an event or reminder | Helpers for the system pickers and editors (no broad permission); optional services only for reading or writing in bulk | Contacts, ContactsUI, EventKit, EventKitUI. The pickers and the event editor need no full-access permission |
| 20 | **Widgets and Live Activities** | Extension template; the widget reads a small shared snapshot the app writes | WidgetKit, ActivityKit, App Intents. Good Dog (today's plan, a session timer). Same App Group spike as item 11; the shared-store contract is in [app-model-review.md](app-model-review.md) |
| 21 | **In-app purchases and subscriptions**: products, purchase, transaction verification, entitlement state Rust can read | Optional service `store` | StoreKit 2. Needs the paid team and App Store Connect; comes with the first commercial app |

### Tier 3: optional

| # | Capability | Shape | Notes |
|---|---|---|---|
| 22 | **Maps** with annotations, **geocoding** and place search | Recipe for `Map`; geocoding added to the `location` service | MapKit has no offline tiles |
| 23 | **Charts**: line, bar, area, pie and point marks styled from ForgeUI tokens | Recipe plus a small style helper | Swift Charts; no charting engine |
| 24 | **Localization** | Spike first | String Catalogs (`.xcstrings`) are compiled by Xcode; check what xtool on Linux can do |
| 25 | **Analytics interface** and **feature configuration** | Rust traits with no vendor | Only once an app ships to people other than its developer |
| 26 | **Speech recognition** | Optional service | Speech; useful but app-dependent (Chat voice messages are the likely first user) |
| 27 | **HealthKit** | Optional service | For a subset of apps; free-team signing unproven (S5) |
| 28 | **Bluetooth accessories and NFC** | Optional services (`bluetooth` grows scanning and connections) | Core Bluetooth works on the free team; Core NFC needs the paid team |

### Recipes and small helpers

Common enough to document, too small for a service. None is scheduled; each is written when an app first needs it.

| Capability | Apple framework | Treatment |
|---|---|---|
| Clipboard and pasteboard | `UIPasteboard`, SwiftUI `PasteButton` | Helper |
| Haptics | SwiftUI `sensoryFeedback` | Recipe; already used in Good Dog |
| In-app review prompt | StoreKit `requestReview` | Helper, with a rule for when to ask |
| Ratings and feedback links | App Store review URL, `mailto:` | Helper |
| Notification settings and Focus | UserNotifications (`openSettings`, time-sensitive and Focus filter behaviour) | Recipe |
| Video playback | AVKit `VideoPlayer` | Recipe |

## Backlog

Recorded so they aren't forgotten, not scheduled ahead of the tiers above.

| Capability | Why | Notes |
|---|---|---|
| **Native text editing** | Notes, messaging, documents and composition: creating content, not only reading it | The most likely to be promoted. `TextEditor` and `AttributedString` first; a `UITextView` helper only if rich text needs it |
| Keyboard and input handling | Search, forms, editors and hardware keyboard shortcuts | Mostly recipes (`@FocusState`, `.keyboardShortcut`, toolbar above the keyboard) |
| Accessibility helpers | Consistent VoiceOver and Dynamic Type behaviour | Partly in ForgeUI and the `forge ui` audits; add helpers where apps repeat the same fixes |

## Not planned

- **Push notifications and Sign in with Apple.** Both need the paid Apple Developer Program, and APNs is a non-goal since 2026-11-05. Revisit when the team is paid ([ios-signing.md](ios-signing.md)). Universal links, StoreKit and NFC also need the paid team; they are planned above and proven on a phone after the switch.
- **A second version of iOS.** No custom navigation, form, animation or authentication framework, no database abstraction over SQLite beyond `forge-storage`, and no charting engine, map renderer or browser engine. SwiftUI, ForgeUI, Keychain and AuthenticationServices, Swift Charts, MapKit and WebKit already do these. ixOS makes native capabilities easier to use.
- **An unrestricted JavaScript bridge.** See the web view spec.
- **A synchronisation engine.** No app has a server that needs it yet; Chat's "server order wins" stands.
- **Unrestricted background execution.** iOS decides when background tasks run. ixOS integrates with that, and doesn't pretend otherwise.
- **Demo apps made only to justify a capability.** A consumer app is added only when usability can't be judged in the apps we have, as with the NetNewsWire-benchmarked feed reader.
