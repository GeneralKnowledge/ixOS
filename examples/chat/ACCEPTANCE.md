# Chat acceptance report

What has been checked, where, and what is still to do, per milestone. Results are kept apart by kind: **automated** (CI on every PR), **local build** (by hand on a development machine) and **physical device** (an iPhone, run by Reuben). A device step is marked done only when it was actually performed and reported. Simulator screenshots are review aids, never acceptance results.

Status as of 2026-11-07.

## M0: Chat on the current framework

### Automated and local

| # | Check | Kind | Status | Evidence |
|---|-------|------|--------|----------|
| A1 | chat-core: accounts, tokens, `Unauthorized`, pause/resume streams | automated | Done | `cargo test -p chat-core` (62 tests) |
| A2 | chat-server: accounts, the server owns the author, data survives a restart, owner-only data file, corrupt or newer data refused | automated | Done | `cargo test -p chat-server` (unit + end-to-end over sockets) |
| A3 | chat-cli against a real server: signup, whoami, rooms, send, read, follow | local build (Linux) | Done | Run by hand in the cloud dev environment, 2026-11-07 |
| A4 | The app's Rust crate (`ChatCore`): sign-up keeps the token in the Keychain adapter and in no file, relaunch is signed in offline, two accounts with the same name, background/resume catch-up, a server that forgot the account, sign-out | automated | Done | `cargo test --manifest-path examples/chat/rust/Cargo.toml` (7 tests against the real server in process); `scripts/check-app-crates.sh` in CI |
| A5 | SwiftUI host compiles; model tests against the real Rust library and the real server | automated (macOS CI) | Done | `scripts/test-swift-host.sh examples/chat`; macOS CI green on main at be8d70e |
| A6 | Unsigned `xcodebuild` of the iOS app | automated (macOS CI) | Done | macOS workflow, "Build Chat.xcodeproj"; green on main at be8d70e |
| A7 | Frozen Android client against the new server (anonymous mode) | automated (CI, JVM) | Done | Android workflow, Chat JVM unit tests; green on main at be8d70e |

### On an iPhone (Linux, xtool)

Before you start: the laptop and the iPhone on the same Wi-Fi; the iPhone on USB and unlocked. This is also the first time the `forge-services` Keychain adapter runs on a device (the native services check app has not been run yet), so D4, D9 and D12 are its first real test.

| # | Step | Expected | Status |
|---|------|----------|--------|
| D1 | `git pull`, then `make chat-server ARGS="--bind 0.0.0.0:8787"` | "chat server on http://0.0.0.0:8787 (data in …/server.json; 0 accounts…" | Done. "chat server on http://0.0.0.0:8787 (data in /home/red/.local/share/ixos-chat/server.json; 0 accounts…". First attempt failed: an old `mock-chat-server` from 2026-11-05 still held port 8787 ("Address already in use"); stopped it, then it started |
| D2 | `forge run ios --project examples/chat` (debug) | Builds and installs; note the build time | Done. Installed with no xtool prompts; Swift link step 12.9 s (first build also compiled the Rust dependencies, several minutes). 26 `ld64.lld: warning: This version of lld does not support linking for platform iOS` lines, harmless |
| D3 | First launch | Sign-up screen; iOS asks for local network access, then notifications | Done (reported "working as intended") |
| D4 | Enter your name and `http://<laptop IP>:8787`, tap **Create Account** | The Chats list with General, Random, Forge dev | Done |
| D5 | `make chat-cli ARGS='signup "Laptop"'`, then `make chat-cli ARGS="send general Hello phone"` while General is open on the phone | The message appears within a second, grey, labelled "Laptop" | Done |
| D6 | Send from the phone in General; `make chat-cli ARGS="follow general"` on the laptop | The bubble goes Sending → Sent (blue, on the right); the laptop prints it with your name | Done |
| D7 | Go back to the list; `make chat-cli ARGS="send random ping"` | A banner "Random / Laptop / ping"; Random's badge shows 1 | Done (reported "working as intended") |
| D8 | Press Home for 30 s; send two messages from the laptop to General; open the app again | General catches up on both (open it); no duplicate bubbles | Done |
| D9 | Force-quit the app (app switcher), turn on Airplane Mode, open the app | Signed in at once (no sign-up screen); stored messages visible; a "can't reach" note on the list | Done |
| D10 | Airplane Mode off; pull to refresh | Rooms load; sending works | Done |
| D11 | Stop the server (Ctrl-C) with General open, send a message, restart the server, tap the red bubble | "Reconnecting…", the bubble turns red, then Sent after the tap; the earlier messages are still there (the server kept them on disk) | Done. The server ran in the background, so it was stopped with `pkill` and restarted on the same data file instead of Ctrl-C |
| D12 | Delete the app and reinstall it (`forge run ios` again) | iOS normally keeps Keychain items across a reinstall, so expect to be signed in again without sign-up and the chats to reload from the server. If it shows sign-up instead, note it: that tells us how xtool's signing treats the Keychain | Done. Signed in again after the reinstall and the chats came back: the Keychain item survives xtool re-signing |
| D13 | Stop the server, move `~/.local/share/ixos-chat/server.json` away, start it again, pull to refresh in the app | "The server no longer accepts this account" on the list; Settings → Sign Out → the sign-up screen; signing up again works | Done. Data file moved to `server.json.bak` (kept) |
| D14 | `forge run ios --project examples/chat --release` | Builds, installs and runs like D4–D6 | Done. Release build in 18.3 s, installed with no prompts; still signed in, received "Hello release" from the laptop, and the laptop's `follow` printed the phone's reply live |

Record for each run: iPhone model, iOS version, xtool version, commit, and anything that differed.

**Run 1, 2026-11-07 (10:28–10:54 UTC):** all of D1–D14 passed, reported by Reuben. iPhone "Cati" (UDID 00008130-…; model and iOS version not recorded), Linux laptop with xtool 1.24.0-ixos.1 (patched), commit be8d70e on main. The laptop side (server, chat-cli, installs) was run through a Claude Code session on the laptop; every on-phone result is Reuben's report.

## Framework problems found and fixed in M0

| Problem | Layer | Fix | Regression test |
|---------|-------|-----|-----------------|
| An app crate that depends on a crate outside Forge's own (Chat's `chat-core`) was not rebuilt when that crate changed, so a stale library could reach the phone | build scripts | `scripts/forge-crate.sh` adds every `path =` dependency in the app's `Cargo.toml` to the directories it watches | Checked by hand (`SCAN_DIRS` now lists `examples/chat-core`); no automated test yet |
| `ForgeServices.install()` makes Forge the notification delegate; the old Chat notifier replaced it, which would break notification taps and deep links | app | The notifier no longer sets a delegate; Forge's delegate shows the banners | None automated (needs a device: D7) |

## Gaps this milestone exposed (for later milestones)

- **No app-level way to watch a non-Forge path dependency** beyond direct `path =` lines (dependencies of dependencies are not scanned).
- **Notifications still go through Swift**, not `forge-services`' `LocalNotifier`; M5 moves them.
- **A stale server on the port is hard to spot.** chat-server only says "Address already in use"; it could name the process holding the port, or `forge doctor` could check it.
- **The server is plain HTTP**, so the token crosses the LAN in the clear. Fine on a home network; anything else needs a TLS proxy.

## M1: Native messaging experience

Device results are Reuben's reports; nothing here is marked done until he reported it.

### Automated and local

| # | Check | Kind | Status | Evidence |
|---|-------|------|--------|----------|
| A8 | chat-core: unread from server read cursors, an open room marked read as messages arrive, others' reads turn our messages Read, reads on another device clear unread, mute keeps the count but skips the banner, marked unread, new conversations, a failed send in the preview | automated | Done | `cargo test -p chat-core` (68 tests) |
| A9 | chat-server: per-account read cursors (only forward, capped at the newest message), `read` events, previews in `GET /rooms`, `POST /rooms` with title checks, version 1 data loads, cursors survive a restart | automated | Done | `cargo test -p chat-server` (unit + `read_receipts_previews_and_new_rooms_between_two_accounts` end to end) |
| A10 | The app's Rust crate: read receipts, new rooms and mute through the FFI (`chat_delivery`, `create_conversation`, `mark_read`, `set_muted`, `set_marked_unread`) | automated | Done | `cargo test --manifest-path examples/chat/rust/Cargo.toml` (8 tests) |
| A11 | chat-cli: `create`, `rooms` newest first with previews and `*` for unread, `read` marks the room read | local build (Linux) | Done | Run by hand against a local server in the cloud dev environment, 2026-11-07 |
| A12 | SwiftUI: `MessagingTests` (two accounts against the real server: Read receipts, previews, unread, swipes, mute, new conversations) and `TranscriptTests` (grouping, separators, which bubble shows its delivery) | automated (macOS CI) | Done | `scripts/test-swift-host.sh examples/chat`; macOS CI green on PR #46 and on main at 71dfa1e |
| A13 | Light, dark and largest Dynamic Type screenshots | automated (simulator CI) | Not yet done | Deferred: the screenshot job builds unsigned and has no chat server, so Chat needs a signed-in fixture first |

### On an iPhone (Linux, xtool)

Before you start: the M0 setup (server on `0.0.0.0:8787`, the app signed in, `chat-cli` signed up as "Laptop"). Update the server and the app first: `git pull`, restart `make chat-server ARGS="--bind 0.0.0.0:8787"` on the same data file (it upgrades it to version 2), then `forge run ios --project examples/chat`.

| # | Step | Expected | Status |
|---|------|----------|--------|
| E1 | Open the app | Two tabs, Chats and Settings. The M0 chats and messages are still there | Done |
| E2 | Look at the Chats list | Each row has a title, the time of the last message, and a two-line preview ("You: …" or "Laptop: …"); the most recent conversation is at the top | Done (first seen from stored data while the phone could not reach the server, see the note below; the list was right once it connected) |
| E3 | Stay on the list; `make chat-cli ARGS="send random hello list"` | Random moves to the top with a blue dot and "Laptop: hello list"; the Chats tab shows a badge of 1 | Done |
| E4 | Open Random, go back | The dot and badge are gone, and stay gone after a pull to refresh (the server stored the phone's read cursor). `chat-cli rooms` shows the laptop's own unread, so it cannot show the phone's | Done |
| E5 | `make chat-cli ARGS="follow general"` on the laptop, then `make chat-cli ARGS="read general"` after the phone sends in General | The phone's bubble shows "Delivered", then "Read" within a second of the laptop reading | Done |
| E6 | Swipe right on a conversation: Mark as Unread; then swipe right again: Mark as Read | The dot appears and the badge counts it; then both go | Done |
| E7 | Swipe left on Random: Mute; leave the app in the foreground on the list; `make chat-cli ARGS="send random muted?"` | A bell-slash icon on the row; the dot appears but no banner; Random is listed under Settings → Muted Conversations, where a swipe unmutes it | Done |
| E8 | Tap the compose button, enter "Weekend", tap Create | The new chat opens at once; `make chat-cli ARGS="rooms"` lists it, and `make chat-cli ARGS="send <id> hi"` appears in it | Done |
| E9 | In a busy chat, look at the bubbles | Messages from one person within five minutes sit together with one name; a "Today 14:05" separator after an hour's quiet | Done |
| E10 | Tap the text field and type several lines | The composer grows to six lines, stays above the keyboard, and the newest message stays in view; dragging the transcript down dismisses the keyboard | Done |
| E11 | Send a message; then stop the server and send another | A light tap on send; an error buzz and a red bubble when it fails; tap it after restarting the server to retry | Done |
| E12 | Dark mode, then Settings → Accessibility → Larger Text at the largest size | Bubbles, list and composer stay readable; nothing is clipped | Done |
| E13 | `forge run ios --project examples/chat --release` | Builds, installs, and E2–E5 still work | Done. Release build installed with no prompts; Random's dot and badge confirmed on the phone. The General message reached the laptop and was marked read; the phone's "Read" was not separately reported on this build |

Record for each run: iPhone model, iOS version, xtool version, commit, and anything that differed.

**Run 1, 2026-11-07 (12:12–12:36 UTC):** E1–E13 passed, reported by Reuben. iPhone 15 Pro Max "Cati", iOS 26.3.1 (a), Linux laptop with xtool 1.24.0, main at 7b69dbd (M1 plus R1, R2 and R4). The laptop side (server, chat-cli, installs) ran through a Claude Code session on the laptop. Notes:

- The server was restarted on the M0 data file, which upgraded it to version 2 with accounts and messages intact (the version 1 file was kept as `server.json.v1-pre-M1.bak`).
- Right after installing the new build the phone could not reach the server ("Host is down (os error 64)") although the server answered on 192.168.1.169:8787 from the laptop. It worked again after Reuben checked the phone's network settings; the most likely cause is iOS's Local Network permission after the reinstall. If you see this error, check Settings → Privacy & Security → Local Network → Chat first.
- R4 logging check (`idevicesyslog`, needs `libimobiledevice-utils`): inconclusive. Chat writes no log records in normal use (only two rare warning paths), and Info/Debug records are not shown by the USB syslog relay, so no Forge lines appeared. Proving R4 on a device needs a Warn record the app can be made to emit.

## M2 device run

M2 adds message changes (reactions, replies, edits, delete for everyone), drafts, a durable outbox, search, and moves the phone's store to SQLite. Device results are Reuben's reports; nothing here is marked done until he reported it.

### Automated and local

| # | Check | Kind | Status | Evidence |
|---|-------|------|--------|----------|
| A14 | forge-storage `Database`: migrations, WAL, FTS5, damaged files moved aside, newer databases refused | automated | Done | `cargo test -p forge-storage --features sqlite` (6 database tests) |
| A15 | chat-core: SQLite store round trip, search, drafts, outbox (offline sends wait, go out in order, survive a relaunch), refused sends fail, replies, edits, deletes and reactions live and in catch-up without counting unread, deleting an unsent message, import of the M1 JSON store, corrupt store recovery | automated | Done | `cargo test -p chat-core` (77 tests) |
| A16 | chat-server: edit, delete and react routes (author checks, 409 after deletion), `since` returns changes, all kept across a restart; the killer demo now goes through the outbox | automated | Done | `cargo test -p chat-server` (`message_changes_between_two_accounts`, `killer_demo_interaction_over_real_sockets`) |
| A17 | The app's Rust crate: `send_reply`, `set_reaction` (with `mine`), `edit_message`, `delete_message`, drafts across a relaunch, accent-insensitive search | automated | Done | `cargo test --manifest-path examples/chat/rust/Cargo.toml` (`message_changes_drafts_and_search_through_the_ffi`) |
| A18 | chat-cli: `reply`, `edit`, `delete`, `react`, messages picked as `@1` (newest) or by id | automated | Done | `cargo test -p chat-cli` |
| A19 | SwiftUI: `testRepliesReactionsEditsDeletesDraftsAndSearch` (two accounts against the real server) | automated (macOS CI) | Done | Swift host tests job green on PR #51 (commit 2679eb3) |

### On an iPhone (Linux, xtool)

**Skipped at Reuben's request (2026-11-07): "Assume that these are all running perfectly."** None of F1–F14 was run on a phone, so none is marked passed. Run them together with the M3 steps below when Chat is back on the phone.

Before you start: the M1 setup (server on `0.0.0.0:8787`, the app signed in, `chat-cli` signed up as "Laptop"). Update first: check out the branch (or `git pull` once merged), restart `make chat-server ARGS="--bind 0.0.0.0:8787"` on the same data file (it upgrades it to version 3), then `forge run ios --project examples/chat`. In the steps, `@1` is the newest message in the room; `make chat-cli ARGS="read general"` lists them numbered.

| # | Step | Expected | Status |
|---|------|----------|--------|
| F1 | Open the app, then Settings | The M1 chats and messages are all still there (imported from the old store into SQLite once). Settings shows "Storage: SQLite 3.x" | Skipped (not run) |
| F2 | In General, touch and hold one of the laptop's messages, tap 👍 | A "👍 1" chip under the bubble, highlighted as yours; `make chat-cli ARGS="read general"` shows `👍1` on it. Tap the chip: it goes away | Skipped (not run) |
| F3 | `make chat-cli ARGS="react general @1 ❤️"` with the phone's newest message at `@1` | "❤️ 1" appears under that bubble within a second; no unread dot, no banner | Skipped (not run) |
| F4 | Touch and hold a message, Reply, type "answer", send | A bar "Replying to …" above the composer while typing; the sent bubble quotes the original; tapping the quote scrolls to it. `chat-cli read general` shows `(reply to …)` | Skipped (not run) |
| F5 | `make chat-cli ARGS="reply general @1 back at you"` | The laptop's bubble quotes the phone's message | Skipped (not run) |
| F6 | Touch and hold one of your sent messages, Edit, change the text, tap the check mark | The bubble changes and shows "Edited"; `chat-cli follow general` prints `changed: … (edited)` | Skipped (not run) |
| F7 | `make chat-cli ARGS="send general to delete"`, then `make chat-cli ARGS="delete general @1"` | The bubble appears, then turns into "This message was deleted" | Skipped (not run) |
| F8 | Touch and hold one of your own messages, Delete, Delete for Everyone | It becomes "This message was deleted" on the phone and in `chat-cli read general` | Skipped (not run) |
| F9 | Type "half a thought" in Random without sending, go back, force-quit the app, open it again and go to Random | The composer still holds "half a thought" | Skipped (not run) |
| F10 | Stop the server (Ctrl-C). Send two messages in General | Both stay "Sending…" (not red); the banner says messages wait; the Chats list says 2 messages are waiting | Skipped (not run) |
| F11 | Force-quit the app with the server still stopped; start the server; open the app and General | Both messages go out by themselves, in order, without tapping; `chat-cli read general` shows them | Skipped (not run) |
| F12 | Pull down on the Chats list and search for a word from an older message, typed without accents or capitals | Matching messages from every conversation, with the room name and the word in bold; tapping one opens the chat scrolled to it, highlighted | Skipped (not run) |
| F13 | In a chat: More (…) → Search in Conversation, search for a word | Only that chat's messages; tapping one closes the sheet and scrolls to it | Skipped (not run) |
| F14 | `forge run ios --project examples/chat --release` | Builds, installs, and F2, F6 and F12 still work | Skipped (not run) |

Record for each run: iPhone model, iOS version, xtool version, commit, and anything that differed.

## M3: Photos and documents

M3 adds photos and documents: the photo picker and the Files picker, photos shrunk on the phone, files uploaded through the outbox and kept once by SHA-256 on the server and the phone, downloads checked against their id, thumbnails, Quick Look and sharing, and `chat-cli send-file` / `download`. Device results are Reuben's reports; nothing here is marked done until he reported it.

### Automated and local

| # | Check | Kind | Status | Evidence |
|---|-------|------|--------|----------|
| A20 | forge-net: `send_file` streams a file with its length, a missing file is a clear error, `download_to_file` keeps only a complete 2xx body | automated | Done | `cargo test -p forge-net` |
| A21 | chat-server: upload stored once by content (201 then 200), names and types cleaned, posts must name uploaded files, anonymous downloads refused, bad ids 400, files kept across a restart | automated | Done | `cargo test -p chat-server` (`attachments_between_two_accounts`) |
| A22 | chat-core: files uploaded before the message and fetched (and checked) by the other account; files sent offline wait in the outbox with their own copy; empty, missing and oversized files refused before the network | automated | Done | `cargo test -p chat-core` (81 tests) |
| A23 | The app's Rust crate: `send_files`, `attachment_file`, `fetch_attachment` | automated | Done | `cargo test --manifest-path examples/chat/rust/Cargo.toml` (`photos_and_documents_through_the_ffi`) |
| A24 | chat-cli: `send-file` guesses the type, `download` checks the SHA-256 | automated | Done | `cargo test -p chat-cli` |
| A25 | SwiftUI: `testPhotosAndDocumentsBetweenTwoPeople` (a 4000×3000 photo shrunk to 2048×1536 JPEG, a text file, sent and fetched between two accounts, thumbnail and named preview copy) | automated (macOS CI) | Done | Swift host tests job green on PR #56 (commit f668f27; the test passed, not skipped) |

### On an iPhone (Linux, xtool)

Before you start: the M1 setup, on this branch (or after it is merged), the server restarted on the same data file, `forge run ios --project examples/chat`. Chat must be installed on the phone (with a free account, at most 3 sideloaded apps).

| # | Step | Expected | Status |
|---|------|----------|--------|
| G1 | In General, tap +, Photo, pick a photo from the library | No permission prompt. A "IMG_….jpg" chip above the text field; × removes it | Not yet run |
| G2 | Pick a photo again, type a caption, send | The bubble shows the photo as a thumbnail with the caption below; Sending… then Sent. `make chat-cli ARGS="read general"` shows `[file: IMG_….jpg (…)]` under 2 MB or so | Not yet run |
| G3 | `make chat-cli ARGS="download general @1 /tmp"`, open the file on the laptop | The photo, the right way up, at most 2048 pixels wide; `exiftool` shows no GPS position | Not yet run |
| G4 | Tap the photo in the bubble | Quick Look opens it full screen; its share button offers Save Image, AirDrop and other apps | Not yet run |
| G5 | Tap +, Document, pick a PDF from Files, send without text | A row with a document icon, the file's name and size. The Chats list shows "You: <name>" | Not yet run |
| G6 | Tap the PDF row | Quick Look shows the PDF with its pages | Not yet run |
| G7 | `make chat-cli ARGS="send-file general ~/some.pdf from the laptop"` and `make chat-cli ARGS="send-file general ~/photo.jpg"` | Both appear live; the photo's thumbnail loads by itself; tapping the PDF downloads and opens it | Not yet run |
| G8 | Stop the server. Send a photo. Force-quit the app, start the server, open the app and General | The photo waits as "Sending…" and goes out by itself after the restart; the laptop can download it | Not yet run |
| G9 | Pick four photos at once and send | One message with four thumbnails, in the order picked | Not yet run |
| G10 | Touch and hold a bubble with a photo | The menu has Open Photo (and no Copy when there is no caption); Reply quotes it as "Photo" | Not yet run |
| G11 | `forge run ios --project examples/chat --release` | Builds, installs, and G2 and G4 still work | Not yet run |

Record for each run: iPhone model, iOS version, xtool version, commit, and anything that differed.

## M4: Voice messages

M4 adds voice messages: record from the composer, send as an audio attachment through the M3 path, play inline with a waveform. Recording and playback are Forge's optional `audio` native service, selected with `native_services!(audio)`; forge.toml declares `microphone`. Device results are Reuben's reports; nothing here is marked done until he reported it.

### Automated and local

| # | Check | Kind | Status | Evidence |
|---|-------|------|--------|----------|
| A26 | forge-services: `NativePermission::Microphone` and the `audio` service (`AudioService::duration`), interface v3 in Rust and Swift | automated | Done | `cargo test -p forge-services` (`the_microphone_is_its_own_permission`, `audio_measures_files_through_the_host`), `scripts/check-template-sync.sh` |
| A27 | The audio adapter is copied only into apps that select `audio`, and committed copies match | automated | Done | `scripts/check-app-crates.sh` (Chat has `ForgeServices+Audio.swift`; the other apps do not) |
| A28 | chat-cli shows audio attachments as `[voice: …]` | automated | Done | `cargo test -p chat-cli` |
| A29 | SwiftUI and ForgeAudio: duration and waveform of an AAC file, the microphone registered as a permission, the player (one file at a time, pause, seek), a voice message between two accounts | automated (macOS CI) | Done | macOS run 42397230438 on 4725db4 (the code merged in #60): all four passed, the player really played (not skipped); Chat, Device Lab and the starter app built and tested |

### On an iPhone (Linux, xtool)

Before you start: the M1 setup, this branch (or main once merged), `forge run ios --project examples/chat`. Chat must be installed on the phone (a free account allows at most 3 sideloaded apps). Have headphones to hand for H8.

| # | Step | Expected | Status |
|---|------|----------|--------|
| H1 | In General, with the text field empty | The send button is a microphone. Typing text turns it back into the send arrow | Not yet run |
| H2 | Tap the microphone | The system asks for the microphone with Chat's sentence from forge.toml. Allow | Not yet run |
| H3 | Tap the microphone, speak for about 5 seconds, tap the send arrow | While recording: a red dot, the time counting, a level meter that moves with your voice. The bubble shows a play button, a waveform and "0:05". `make chat-cli ARGS="read general"` shows `[voice: Voice message.m4a (…)]` | Not yet run |
| H4 | Tap play on that bubble | You hear the recording through the speaker; the waveform fills and the time counts up; it returns to the start at the end | Not yet run |
| H5 | Turn the ring switch to silent and play it again | It still plays | Not yet run |
| H6 | Record, then tap the trash button | Nothing is sent | Not yet run |
| H7 | Record something on the laptop (any `.m4a`), then `make chat-cli ARGS="send-file general ~/note.m4a"` | It appears as a voice message on the phone and plays | Not yet run |
| H8 | Play a long voice message with headphones in, then unplug them (or turn off Bluetooth headphones) | Playback pauses instead of switching to the speaker | Not yet run |
| H9 | Start playing one voice message, then tap play on another | The first stops; only one plays | Not yet run |
| H10 | Settings → Privacy & Security → Microphone → turn Chat off, then tap the microphone in Chat | An alert explains it and offers Open Settings, which opens Chat's page in Settings | Not yet run |
| H11 | Record, and have someone call the phone while recording | The recording stops and what was recorded is sent | Not yet run |
| H12 | `forge run ios --project examples/chat --release` | Builds, installs, and H3 and H4 still work | Not yet run |

Record for each run: iPhone model, iOS version, xtool version, commit, and anything that differed.

## L1: On-device assistant

L1 adds the Assistant (design: [local-ai.md](../../docs/local-ai.md), "L1 as built"): private conversations with Apple's on-device model through Foundation Models, with the plain assistant or the test characters Leo and Mia, streaming, a Stop that ends the model's work, and history kept on the phone in `assistant.db`. It needs no account and no chat server. Device results are Reuben's reports; nothing here is marked done until he reported it.

### Automated and local

| # | Check | Kind | Status | Evidence |
|---|-------|------|--------|----------|
| A30 | `AssistantCore` with a fake engine: characters and opening lines, streaming and coalescing (200 pieces in fewer than 50 deliveries), the pinned character and reminder, trimming to the context, Stop keeps the text and the engine computes nothing after it, an engine that ignores Stop, one answer at a time, catching up mid-answer, unavailable/no engine/bad input, refusals and failures, history across a relaunch, `Interrupted` after a kill, delete and shutdown stop the answer, the engine off the caller's thread | automated | Done | `cargo test --manifest-path examples/chat/rust/Cargo.toml assistant` (18 tests) |
| A31 | `ChatCore`: the assistant works signed out and keeps its chats through sign-up, sign-out and a relaunch | automated | Done | `the_assistant_works_signed_out_and_survives_sign_out` |
| A32 | SwiftUI host compiles with the Foundation Models adapter; `AssistantModelTests` (scripted Swift engine: streaming signed out, Stop, no engine) | automated (macOS CI) | Done | macOS run 42459470174 on 5cd6fa2: the 3 `AssistantModelTests` and the other 16 Swift tests passed; unsigned iOS device build OK |

### On an iPhone (Linux, xtool)

Before you start: Chat installed from this branch (`forge run ios --project examples/chat --release`; a free account allows 3 sideloaded apps). Settings → Apple Intelligence & Siri: Apple Intelligence on and its model downloaded (it was on Cati for L0). No server, no extra disk: Foundation Models uses the model iOS already has. On the laptop, `forge device logs --grep assistant` (keep the `ChatXtool … [assistant]` lines; iOS's keyboard "input assistant" lines match too) records each answer's timings (never its text).

| # | Step | Expected | Status |
|---|------|----------|--------|
| AS1 | Open Chat. Signed out: tap the sparkles button on the sign-up screen. Signed in: the **Assistant** tab | "No Assistant Chats" and no warning above it. A warning means Foundation Models is unavailable; note its text | Done (2026-11-08, ada3d23, reported via the iPhone session) |
| AS2 | **Start a Chat** → **🙋‍♀️ Mia** | Mia's opening line. The title is Mia; the line under it says "Apple Intelligence · on this iPhone, offline" | Done (2026-11-08, ada3d23) |
| AS3 | Send "I had a long day at work" | A spinner, then the answer appears as it is written; the send arrow is a stop button meanwhile. Under the answer: "First words x s · y s". Note both numbers. The answer is Mia's message only, never "You:"/"User:" lines | **Failed** on ada3d23 (2026-11-08): Foundation Models streamed (first words 1.4 s, 3.9 s total, 13 pieces, `pieces_after_stop=0`) but Mia also wrote invented "You: …/User: …" turns, and the next two messages got "I'm sorry, I can't continue this roleplay." Cause: the history went to the model as one flattened script. Fixed in #104 (transcript entries, answers cut at a new turn, no "role-play" framing), not yet phone-tested: Reuben skipped the recheck (2026-11-08 09:50) |
| AS4 | Keep chatting with Mia for about five messages, then New → Leo for three | Do they stay in character (warm, short, ask about your day)? Note any refusals ("Apple Intelligence declined…") | **Failed** on ada3d23 (same cause as AS3: invented turns, then "can't continue this roleplay"); fix not yet phone-tested |
| AS5 | New → 💬 Assistant, send "Write a long story about a lighthouse keeper", press the stop button after two or three lines | The text stops growing at once, "Stopped" under it, the send arrow returns, and you can send again straight away. `forge device logs` shows `answer Stopped … pieces_after_stop` of 0 or 1 (the one piece Foundation Models had already produced when Stop was pressed; the engine ends at that piece). Ask for a "1,000-word story": short answers finish in about 3 s | Done on ada3d23 (2026-11-08): stopped mid-sentence, "Stopped"; log `first_text_ms=1331 duration_ms=2015 pieces=5 pieces_after_stop=1` |
| AS6 | Force-quit Chat (app switcher) and open it again | The Assistant list shows the three conversations, newest first, with previews; each opens with its whole history | Partly: after AS7's reopen the history was intact (ada3d23); the list after a plain force-quit not explicitly confirmed |
| AS7 | Send another long request and force-quit while it is being written; open Chat again | That answer says "Interrupted when the app closed"; sending works | Done on ada3d23: "Interrupted when the app closed"; log `1 answer(s) were interrupted by the last exit`. The empty bubble read "No text"; it now says "No answer was written" (ba45825) |
| AS8 | Airplane Mode on (Wi-Fi off too); send a message to Mia | It answers as before: nothing leaves the phone | Done on ada3d23 (2026-11-08, "Airplane mode is working") |
| AS9 | Optional: Settings → Apple Intelligence & Siri → off, back to Chat | The Assistant shows "Turn on Apple Intelligence in Settings…", and sending says the same. Turn it back on (the model may take a while to come back) | Not run (2026-11-08) |

Record for each run: iPhone model, iOS version, xtool version, commit, and anything that differed.
