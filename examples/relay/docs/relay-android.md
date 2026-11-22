# Relay on Android

Started 2026-11-22 (Reuben: "bring the Relay app for Android to parity"). Relay
is its own product track (Chats, Send, Devices), and it is the second app to
get an Android version after Pocket Library. The roadmap's "only Pocket
Library on Android" cap is lifted for Relay; the rest of the Android rules
still hold: Rust is the one shared core, the interface is Jetpack Compose with
Material 3 and no shared UI code, iOS goes first, and results come from Linux
tests and the CI emulator until a physical Android phone is available.

There was no Relay app on Android to bring up to date. The old Compose client
(`examples/relay-android`, the chat demo's phase 5) was removed on 2026-11-15
and never had word codes, trusted devices or iroh. This is a new app in the
same place, built on Relay's current Rust core (`examples/relay/rust`)
unchanged: a thin crate (`examples/relay-android/rust`) links it into
`libforge_ffi.so`, as Pocket Library's Android app does.

## What is shared and what each platform does

Everything Relay decides is in Rust and comes to Android as it is: identity,
trust and its rights (receives from, introduces, sees devices, person,
grants), word codes (SPAKE2), iroh connections and the route rules of each
Connections mode, the outbox and its retries, the SQLite record and its
migrations, device conversations, the mailbox client and room protocol, Sites
over Relay, and presence. The rule that no feature gets its own transport,
identity, permissions or queue holds on Android by construction: the Kotlin
side has no networking of its own beyond finding addresses.

What each platform does itself:

| Concern | iPhone | Android |
|---|---|---|
| Finding code senders and trusted devices on the network | Bonjour, `NWBrowser`/`NetService` (`RelayNetwork.swift`) | `NsdManager`, the same `_forgerelay._udp` service and TXT keys (`v`, `n`, `id`, `d`) |
| Where the device key lives | Keychain (`SecureStore`), never a file | Stage 1: `device.key` in the app's private files, with backups off. Stage 4: Android Keystore |
| Picking what to send | `fileImporter`, `PhotosPicker` | Storage Access Framework and the photo picker, copied into the cache first |
| Opening what arrived | Quick Look, the Files app | An `ACTION_VIEW` intent through a `FileProvider` |
| The app's lifecycle | ForgeKit `ForgeHost` | The Android `ForgeHost` (copied from Pocket Library, see below) |

## The gap list

What the iPhone app does, and when Android gets it. Stages are PRs, biggest
value first; each starts when the one before has merged.

| Experience | iPhone feature | Android stage |
|---|---|---|
| Send | Send files with a word code; the code shown with Copy Code | **1** |
| Send | Send photos | **1** (Android's photo picker) |
| Send | Send a whole folder | **3** (the tree picked with SAF is copied in file by file) |
| Send | Code shown as a QR code | 4 (needs a QR library: a dependency decision) |
| Send | Advertising the code on the network, avoiding numbers other senders use | **1** |
| Receive | Type a code, find the sender, receive with progress, resume | **1** |
| Receive | Scan the sender's QR code | 4 (camera plus a QR library) |
| Receive | Received files in the Files app | **1**: opened from the history; Save to Device (a file where the person picks, a folder into a picked folder) in **3** |
| Devices | Trust offer after a code transfer (Ask to Trust), introduced devices | **1** |
| Devices | Trusted and revoked lists, presence ("Reachable, seen 2 min ago") | **1** |
| Devices | Device page: send files, where it is, platform, offers, Ask Again | **1** |
| Devices | Can Send to This Device, This Is a Person, Introduces Devices, Can See My Devices, Check Its Devices Now | **1** |
| Devices | Revoke, Forget, Trust Again | **1** |
| Devices | Rename this device | **1** |
| Devices | Advertising this device to trusted devices nearby | **1** |
| Settings | Pairing policy, Connections mode (Everywhere, Private Networks, This Network Only), servers (Relay's, your own, none), server health | **1** |
| Settings | Saved-data and away-from-home notices | **1** |
| History | Open, copy a note, cancel a waiting send, remove | **1** |
| Chats | Device conversations (messages and attachments to trusted devices, one outbox) | **2** |
| Chats | The Message button on a device's page | **2** |
| Chats | Replies, search, drafts, unread counts, mute, Try Again and deleting a message that failed to send | **2** |
| Chats | Reactions, edits and unsending a delivered message (Rust has them for chat-server rooms only, not devices yet) | 3, with rooms |
| Chats | Voice messages | 5 (the `audio` service adapter) |
| Chats | A chat server account, rooms, Join a Room by code | 3 |
| Chats | Notifications for new messages, tapping one opens the conversation | 3 (`LocalNotifier` adapter, `POST_NOTIFICATIONS`) |
| Chats | Catching up in the background | 3 (`BackgroundScheduler` on WorkManager) |
| Away from home | Relay's relay and lookup servers, the mailbox for devices that are off | **1** for the servers (Rust). The mailbox needs nothing on Android beyond them |
| Security | Device key in the platform's secure store | 4 (Android Keystore `SecureStore` adapter; the key moves in unchanged, as on iOS) |
| Security | App lock with Face ID | 4 (`BiometricPrompt` adapter) |
| Sites | Can Open My Site (the grant) | **1** |
| Sites | Open a trusted device's site in the app | 5 (`WebView` with `shouldInterceptRequest` calling `fetch_site`) |
| Assistant | Apple's on-device model; asking another device's model | Shown as unavailable in 2; a model waits for the Android AI stage ([android-local-ai.md](../../../docs/research/android-local-ai.md)) |
| Diagnostics | Memory readout on the Send sheet | Not planned (an iPhone test aid) |

Not in any stage yet: a home-screen share target ("Send with Relay" from
other apps), which iOS doesn't have either.

## Chats on Android (stage 2)

Chats opens chat-core (`ChatCore`) beside Relay's core in the same host, in
`<app>/chat`, and hands it Relay's core (`use_relay`), so each trusted device
is a conversation and its messages and attachments go through Relay's one
outbox, exactly as on the iPhone. The two cores open and close together; if
Chats can't open, Devices still work and the Chats tab says why.

`ChatModel.kt` mirrors the iPhone's `ChatModel.swift` (it holds no rules: the
store, outbox, retries, unread counts, drafts and search are Rust's) and
`ChatText.kt` holds the words and the transcript grouping, so JVM tests check
both. The screens are a Chats tab (`chats.*`: the list, most recent first,
with unread dots, a long press to mark read or unread or to mute, pull to
refresh, and search across every conversation) and a conversation
(`chat.*`: grouped transcript, unread divider, New Messages button,
connection banner, replies, edits, reactions, Copy, Try Again, Delete with a
confirmation, photos and files from the photo picker and the Storage Access
Framework, and search in the conversation). A device's page has a Message
button (`trusted.message`).

A device conversation carries messages over Relay's outbox, which can't yet
edit, react to or delete a delivered message: chat-core answers those with an
error (for a delete, the misleading "Sign up first.", since the message is
routed as if to a chat server). Android leaves those actions out of a device
conversation's menu; the iPhone still offers them and shows the error, a fix
for its next Relay PR.

Not yet: a chat server account and rooms (stage 3, which also needs the
account kept in a secure store: Chats opens with `with_memory_credentials`
until then), notifications and background catch-up (stage 3), voice messages
(stage 5), and the Assistant tab (the Android AI stage).

## Framework code on Android

The Kotlin host (`ForgeHost`), the ForgeAppKit counterparts (`kit/`) and the
theme are copied from Pocket Library's Android app. Relay is the second
Android app, so this is the real second use that the [framework
principles](../../../docs/architecture.md#framework-principles) ask for:
moving them into a shared Android ForgeKit is a follow-up PR once stage 1 has
merged, not part of it. `scripts/android-tokens.py` writes the theme tokens
for both apps, so they can't drift.

## Checking it

There is no Android phone yet, so every stage is checked on Linux and the CI
emulator only:

- JVM unit tests drive the model against the host build of the real Rust core
  (two cores in one process send to each other over loopback with a word
  code).
- Compose tests on the CI emulator walk each screen by its test tags, the
  same identifiers as the iPhone app (`relay.send`, `trustOffer.trust`, ...).
- Nothing here is recorded in the capability ledger, which tracks iOS.

The check on a real phone, when one is back: two Android phones, or an
Android phone and the iPhone, on one Wi-Fi: send a photo with a code each
way, trust, then send without a code; then the same away from home.
