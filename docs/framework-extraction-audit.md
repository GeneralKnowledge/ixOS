# Framework extraction audit

Audit of what the demos have built that belongs in ixOS, 2026-11-07. It covers `main` at `50dee63` and four open pull requests, read without changing them: #53 (Pocket Library EPUB reader and read-aloud), #55 (local AI plan and L0 check app), #56 (Chat M3 photos and documents) and #57 (Forge Device M1 live connection). #45 (logging) is already merged.

This is an audit, not a refactor. Nothing here asks an open PR to change or wait. Each extraction below starts after the PR it draws on has merged and, where the item says so, after its device run.

The rules this audit applies are in [architecture.md](architecture.md#framework-principles):

1. Move reusable infrastructure into ixOS. Keep an app's identity and its domain logic in the app.
2. Extract when there is a real second use, or when the code is clearly platform-level. Do not extract something only because it might be reusable.

## Summary

- **The framework already does more than the docs say.** `forge-storage` has had a SQLite `Database` (migrations, transactions, FTS5, recovery) since Chat M2, and Chat uses it. The ledger still said "No" for SQLite and full-text search; this PR corrects those rows.
- **Four pieces of real duplication**, each with two or more users today: app-managed file stores (Chat attachments, Pocket Library covers and books, the AI models to come), image thumbnails in Swift (Chat, Pocket Library), retry with backoff (two loops in Chat, needed by AI model downloads), and versioned JSON files written by hand (Counter, Weather twice) when `forge_storage::JsonFile` exists.
- **Framework pieces the apps work around.** Chat posts notifications through `UNUserNotificationCenter` directly because `LocalNotifier` cannot group or route them. Chat and Pocket Library call `.fileImporter` directly instead of `forgeFileImporter`. Swift code logs nothing, so the #57 live log stream has no Swift records.
- **Optional modules:** developer tools are already a module (#57: `forge-devproto`, `forge-devtools`, behind the `devtools` feature). Local AI has a planned extraction step (L5 in #55). Attachments split into generic file handling that belongs in the core, and Chat's own rules that stay in Chat. EPUB reading and audio wait for their second user, and both have one in sight.
- **Crate layout:** most of the proposed crates should start as modules inside the five crates we have, selected by Cargo features, and become crates only when they carry heavy dependencies or native code an app must be able to leave out. One name needs to change: `forge-device` already exists and means the Linux developer tool, not camera and location.

## Prioritised list

Priority means the order to do the work in, once its trigger has happened. Reuben confirmed the core order on 2026-11-07: managed files, shared backoff, Swift logging, image processing, notifications, connectivity, with Swift-side service selection (C7) required before any further native integration. **Trigger** is the event that makes extraction worthwhile under rule 2.

### 1. Core framework

| # | Capability | What moves into ixOS | Taken from | Home | Trigger | Priority |
|---|---|---|---|---|---|---|
| C1 | Files | A managed file store: copy a picked file in, name it (by content hash or by the app's own name), verify a download before keeping it, store relative paths only, exclude from iCloud backup when asked, sweep files nothing refers to, report size used | Chat `AttachmentStore` (#56), Pocket Library `LibraryStore` covers, books and exports (#53), AI model folder (#55 plan) | `forge-storage::files` module; SHA-256 behind a `hash` feature | #56 and #53 merged. Three users | High: first |
| C2 | Networking | A `Backoff` policy (initial, cap, jitter, reset on success, cancellable wait) for every retry loop | chat-core stream reconnect (500 ms to 5 s) and outbox (1 s to 60 s); AI model downloads (#55 L2) | `forge-net` | Now for Chat's two loops; before AI L2 at the latest | High |
| C3 | Logging | A Swift log call that writes into `forge_runtime::logging`, so Swift records reach the system log, `recent()`, and the #57 live stream like Rust ones | New; closes #57's "Swift-side logs not included" limit | `forge-runtime` (Rust entry point) and `forge-services/swift` (a `ForgeLog` wrapper) | #57 merged. Platform-level, so no second use needed | High |
| C4 | Media | Thumbnail and downscale helpers on ImageIO, off the main thread, sized for the display scale | Chat `AttachmentFiles.thumbnail` and `preparePhoto` (#56), Pocket Library `CoverThumbnails` | `forge-services/swift` (a `ForgeImages` file). Rust does no image decoding | #56 merged. Two users with near-identical code | Medium-high. **Done:** `ForgeImages.swift`, copied into every app's bindings (no Rust side, no privacy-sensitive API, so not tied to `native_services!`); Chat's photos and thumbnails and Pocket Library's covers use it |
| C5 | Notifications | Grouping (thread id), subtitle, and a routing payload on `LocalNotifier`, delivered back as a deep link when the user taps it; then Chat's `LocalChatNotifier` moves onto it | Chat `LocalChatNotifier.swift` | `forge-services` (`notify.rs`, `ForgeServices+Notifications.swift`) | Now: the framework version exists and Chat bypasses it | Medium |
| C6 | Networking | Connectivity: online or offline, Wi-Fi or cellular, expensive or constrained, as a value Rust can read and an event when it changes | Needed by #55 decision 7 (Wi-Fi-only model downloads), Chat reconnect after airplane mode, Weather's offline fallback | `forge-services` foreign trait (native `NWPathMonitor`), read through `forge-net`. Built in #59 as the `connectivity` optional service (`ConnectivityService`, `TransferPolicy::Unmetered`); adopt it rather than writing a second | AI L2. Platform-level | Done in #59 |
| C7 | Native services | Per-app selection on the Swift side: `forge.toml` names the services an app uses, `native_services!` declares only those traits, and the build copies only their `ForgeServices*.swift` files | `generate-bindings.sh` copies every adapter into every app today | `forge-services`, `forge-cli`, `scripts/generate-bindings.sh` | Now. Prerequisite for C9 and any further native integration. Done in #59, driven by the macro rather than `forge.toml`: `native_services!(connectivity, ...)` declares the optional services, and `generate-bindings.sh` copies `swift/optional/ForgeServices+<Name>.swift` only when the bindings contain `installNative<Name>` | Done in #59 |
| C8 | Files | Find out why both apps call `.fileImporter` directly, then either fix `forgeFileImporter` or drop it | Chat (#56), Pocket Library | `forge-services/swift` | Now. A framework helper nobody uses is either missing something or not needed | Medium |
| C9 | Device integration | Permission cases beyond notifications: microphone, camera, photos, location | `PermissionProvider` covers notifications only; `forge.toml` already writes the purpose strings | `forge-services`, after C7 | With the first feature that asks: Chat M4 voice (microphone) | Medium, triggered |
| C10 | UI foundations | Host files every `forge init` app copies unchanged (`AppLifecycle.swift`, `ErrorAlert.swift`, `AppServices.swift`) shipped with the Swift adapters, so a fix reaches every app at its next build | Template and Chat (byte-identical today) | Copied into `Generated/ForgeFFI` the way `ForgeServices*.swift` already is | When one of them next needs a fix | Low-medium |
| C11 | Search | The FTS5 query builder (turning what the user typed into a safe prefix query), as a helper next to `Database` | chat-core `fts_query` | `forge-storage` (feature `sqlite`) | AI L1, whose history search is the second FTS user | Low, triggered |
| C12 | Persistence | Move Weather's cache and places files onto `JsonFile`; retire `signal.rs` in Weather in favour of `forge_runtime::background` | Weather (`app.rs`, `places.rs`, `signal.rs`) | Already in the framework | Next time Weather's Rust changes | Low |

Not recommended now:

- **Settings.** Each app keeps its settings in its own store, and Swift-only display preferences (read-aloud speed, text size) in `@AppStorage`. No two apps share a setting or a settings screen. Revisit if an app needs a preference that both Rust and Swift read.
- **Search beyond C11.** Pocket Library filters a few hundred books in memory, which is right for its size. A general search interface would have one user.
- **A shared HTTP client factory.** Three apps build `UreqClient::with_user_agent(...)` in one line each. That is configuration, not duplication.
- **A UI component library.** [ios-ui.md](ios-ui.md) and the starter app already define the native patterns. Views that are app-specific (bubbles, forecast pages, the reader) stay in their apps.
- **Counter's hand-written store.** [architecture.md](architecture.md#persistence) documents it as the worked example of the action-and-persistence contract. Leave it unless that page changes.

### 2. Optional modules

| # | Module | State today | What belongs in the module | What stays in the app | Trigger | Priority |
|---|---|---|---|---|---|---|
| O1 | Developer tools | Already a module in #57: `forge-devproto` (wire format, Noise, CBOR), `forge-devtools` (in the app, `devtools` feature, compiled out of release builds), `forge-device` (the host) | Swift logs (C3), built-in commands (reset storage, send a test notification), probes for framework state (storage size, open streams, lifecycle phase) | Each app's own probes and commands, such as Chat's room list | #57 merged | High, in progress |
| O2 | Local AI | Planned in #55: a `LocalModelEngine` foreign trait implemented in Swift, `TokenSink`, an assistant core in Chat's crate, `assistant.db` | The engine trait, records, Swift adapters (Foundation Models, CoreML-LLM), the model library (catalogue, download, verify, remove), and the memory and thermal guard | Characters, system prompts, the assistant's memory and the Chat screens | #55's own L5 step, after L1 to L4 have run on the phone. Downloads and model files should use C1, C2 and C6 rather than code of their own | Medium, scheduled |
| O3 | Attachments | In chat-core (#56): a content-addressed store, upload before send, verified download on demand | Most of it is core, not a module: the file store (C1), file-backed transfer (already in `forge-net` in #56), and later background transfers and upload recovery. A separate attachments module is worth it only for the upload queue and its recovery | The 25 MiB and 10-per-message limits, the `/attachments` routes, the `Attachment` record on a message | A second app that sends files to a server (Relay, or images to the assistant) | Medium for C1; module later |
| O4 | Documents | In Pocket Library (#53): `epub.rs` and `reader.rs`, about 1,500 lines of pure Rust (zip, quick-xml) that turn an EPUB into metadata, chapters and styled text blocks | EPUB metadata and text extraction, position-to-page mapping, behind an `epub` feature so apps that do not read books skip `zip` and `quick-xml` | Library organisation, reading progress rules, the reader screen | A second reader of documents, for example the assistant answering questions about a book. Meanwhile keep `reader.rs` free of Pocket Library types so the move is mechanical | Low, triggered |
| O5 | Audio | Read-aloud in Pocket Library (#53): `AVSpeechSynthesizer`, the audio session, lock-screen controls and Now Playing, all in Swift | Owner of the audio session (one per process, so two features in one app must share it), Now Playing and remote commands, speech synthesis; later recording | What is read and how the reader highlights it | Chat M4 voice messages (recording and playback). The assistant reading answers aloud would be a third | Medium, triggered |

### 3. Individual applications

These stay in their apps. The framework provides the machinery; the app decides what to do with it.

| App | Stays in the app |
|---|---|
| Chat | The conversation model and screens, rooms, read cursors, reactions, edits and deletes, the outbox's ordering rules, the server protocol (`chat-core` and `chat-server`), attachment limits, and later the assistant's characters and memory |
| Pocket Library | Books, shelves and progress, the Open Library catalogue client, reading position rules ("never auto-finishes", hand-typed page overrides), the reader's look |
| Weather | Open-Meteo, the forecast model, `weather_overview`, saved places and their rules (2 km is the same place), sky backgrounds, freshness wording |
| Counter | Everything; it is the reference for the app-crate pattern |
| Chat Android | Frozen copy of the old Chat API; no extraction until Android work resumes |

## Proposed layout checked against the workspace

The layout in the request was illustrative. This is what exists, and what each proposed crate should be for now.

| Proposed | What exists | Recommendation |
|---|---|---|
| `forge-runtime` | `forge-runtime`: background work, cancellation, `EventHub`, `Lifecycle`, `app_shell!`, logging, Android JNI init | Keep. Add C3 |
| `forge-platform` | No crate. The platform adapters are `forge-services` (foreign traits plus `swift/`), `forge-net`'s certificate verifier, and `forge-runtime`'s JNI init | Do not create. `forge-services` is the platform layer |
| `forge-storage` | `forge-storage`: `write_atomic`, `JsonFile`, `Database` (feature `sqlite`) | Keep. Add the `files` module (C1) and the FTS helper (C11) |
| `forge-network` | `forge-net`: `HttpClient`, `UreqClient`, streams, cancellation; file upload and download in #56 | Keep the name. Add `Backoff` (C2) and read connectivity (C6) |
| `forge-files` | Directories, picker import and sharing in `forge-services`; stores in Chat and Pocket Library | A module in `forge-storage` (C1), not a crate |
| `forge-media` | Nothing shared; thumbnails written twice in Swift | Swift helpers in `forge-services/swift` (C4). A crate only if Rust ever processes images |
| `forge-background` | `forge_runtime::background`; nothing for iOS background tasks | Stays in `forge-runtime`. `BGTaskScheduler` goes to `forge-services` with Chat M5 |
| `forge-notifications` | In `forge-services` | Stays there (C5) |
| `forge-search` | FTS5 through `Database` | A helper in `forge-storage` (C11), not a crate |
| `forge-device` (camera, photos, location, biometrics) | **Name taken.** `crates/forge-device` is the Linux-side developer tool behind `forge device` | Device integrations go into `forge-services` as foreign traits. If they outgrow it, call the crate `forge-sensors` or similar |
| `forge-devtools` | `forge-devtools` and `forge-devproto` in #57, plus `forge-device` on the host | Already the module this proposal describes |
| `forge-ai` | Planned in #55 (L5) | As planned |
| `forge-attachments` | chat-core | Generic parts to `forge-storage` and `forge-net`; a module only on a second user (O3) |
| `forge-documents` | Pocket Library | Later, behind an `epub` feature (O4) |
| `forge-audio` | Pocket Library, Swift only | Later, starting with the audio session (O5) |
| `examples/` | Counter, Weather, Chat (with `chat-core`, `chat-server`, `chat-cli`, `chat-android`), Pocket Library, `counter-android` | Matches |
| `templates/` | `app-rust`, `ios-app`, `android-app` | Matches |

### How an app selects capabilities

On the Rust side this already works: each app's `rust/Cargo.toml` lists the framework crates it uses, and heavy parts sit behind features (`forge-storage/sqlite`, `forge-net/ureq`, the app's own `devtools`). A new capability should be a feature first and a crate only when it has its own native adapters or a large dependency tree. Fewer crates also means fewer version checks between Rust and Swift.

The Swift side does not yet allow selection. `generate-bindings.sh` copies every `ForgeServices*.swift` file into each app that uses `native_services!`, and the macro declares every foreign trait. That is fine for Keychain, notifications and files. It will not be fine for camera, photos, location or microphone: an app should not link APIs it does not use, and App Store review flags binaries that reference privacy-sensitive APIs without the matching purpose strings. That is C7, and it comes before C9 and any further native integration. PR #59 does C7 for new services: optional ones are selected in `native_services!(...)` and only their adapters are copied. The core set (Keychain, notifications, files) is still copied into every app, which is fine because none of it is privacy-sensitive.

## Findings in detail

Each finding gives the evidence, so it can be checked.

**F1. App-managed file stores, three times.** Chat's `AttachmentStore` (`examples/chat-core/src/attachments.rs` in #56) names files by SHA-256, copies a picked file in before sending, and checks a downloaded file's hash before keeping it. Pocket Library's `LibraryStore` (`examples/pocket-library/rust/src/store.rs`) keeps `covers/`, `books/`, `reader/` and `exports/`, names files itself, and deletes orphans (`remove_orphan_covers`). The local AI plan (#55, Persistence) keeps model files under `Application Support/Models/<engine>/<model>/`, excluded from backup, with relative paths in Rust. All three need copy-in, relative paths, verification and cleanup.

**F2. Thumbnails, twice.** `CGImageSourceCreateThumbnailAtIndex` with the same options appears in Chat's `Attachments.swift` (#56) and Pocket Library's `CoverImage.swift`, both decoding off the main thread at the size shown.

**F3. Retry with backoff, twice in Chat, and soon a third time.** chat-core's stream pump doubles from `reconnect_initial` to `reconnect_max`; its outbox doubles from `OUTBOX_INITIAL_DELAY` to `OUTBOX_MAX_DELAY`. The ledger already lists "Retry with exponential backoff and jitter" as a framework capability under W11. AI model downloads need resume and retry.

**F4. Versioned JSON written by hand.** Counter (`store.rs`), Weather's cache (`app.rs`, schema 2) and Weather's places (`places.rs`) each define an envelope with `schema_version` and call `write_atomic`. Pocket Library and the template use `forge_storage::JsonFile`, which does the same with migrations. Counter and Weather predate `JsonFile`.

**F5. Two copies of the async signal primitives.** `examples/counter/rust/src/signal.rs` and `examples/weather/rust/src/signal.rs` both define `Completion` for handing results from a worker thread to an awaiting caller. `forge_runtime::background` covers the same need, and Chat, Pocket Library and the template use it.

**F6. Notifications bypass the framework.** `examples/chat/Sources/AppHost/LocalChatNotifier.swift` builds `UNMutableNotificationContent` itself, with `threadIdentifier` and a `conversationId` payload, because `LocalNotifier.schedule` has neither. It also requests permission itself rather than through `PermissionProvider`.

**F7. File pickers bypass the framework.** `forgeFileImporter` exists in `ForgeServices+Files.swift`, but Pocket Library (`LibraryView.swift`) and Chat (#56, `ChatView.swift`) call `.fileImporter` directly. Only the services check app uses the helper.

**F8. Swift logs go nowhere.** No Swift file in the examples or template uses `Logger` or `os_log`. Rust logs reach the device log and, with #57, the live stream; Swift failures (a picker error, a thumbnail failure, a notification refused) are invisible from Linux.

**F9. Host files copied by `forge init`.** `AppLifecycle.swift`, `ErrorAlert.swift` and `AppServices.swift` are identical in `templates/ios-app` and `examples/chat`. A fix to the template does not reach existing apps.

**F10. Platform code with one user.** Weather's `LocationProvider.swift` (Core Location) and Pocket Library's `ReadAloud.swift` (#53: speech, audio session, Now Playing, remote commands) are clearly platform-level but each has one user. The audio session is process-wide, so once Chat M4 records voice messages, a framework owner avoids two features fighting over it. Location waits for FieldKit.

**F11. The ledger was behind the code.** The SQLite, full-text search and durable outbox rows said "No" or "not yet stored" after Chat M2 (#51) shipped them. Corrected in this PR; their iPhone column stays "No" because the M2 device run was skipped.

## The capability registry

The registry proposed in the request is the [capability ledger](capability-ledger.md), extended rather than duplicated. The ledger already has one row per capability with its proof (framework, Rust API, iOS native, in an app, iPhone, documented, limits). This PR adds a **Modules** section at the top of it: one row per framework module, with what it provides, its crate and feature, the native APIs it needs, its dependencies, its status on iOS, Android and desktop, whether it is stable or experimental, and the demos that exercise it. The capability rows below it stay the evidence for each module's status.

A module is **Stable** when its main capabilities are proven on a physical iPhone and documented; **Experimental** when it is used by an app but not yet proven; **Planned** when it exists only in a plan or an open PR.

Before starting a new demo, read the Modules table for what it needs, and the capability rows for what is still "No".
