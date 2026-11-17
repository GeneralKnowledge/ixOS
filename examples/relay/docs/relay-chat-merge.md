# Merging Chat into Relay

Status: **agreed** (Reuben, 2026-11-17: all three
[decisions](#decisions-reuben-2026-11-17) as recommended). Step 1 (one
app) is built: Chat's screens are Relay's Chats, Assistant and Settings
tabs beside Devices, its Rust is `rust/src/chat`, its engine, server and
terminal client are in `messaging/`, and `examples/chat` is gone. Where this differs from
[relay-platform-plan.md](relay-platform-plan.md) and
[relay-m1-m2-brief.md](relay-m1-m2-brief.md), this wins; the
[doc changes](#docs-that-change) list what is rewritten and when.

## The question

Reuben, 2026-11-17: "I am thinking that relay and chat can be merged as a
chat system is planned for relay."

Today the two are separate on purpose. On 2026-11-14 the decision was
"share, don't merge" (Relay would implement chat-core's `ChatRepo` over
iroh and reuse Chat's views). On 2026-11-16 that hardened into "Relay takes
nothing from Chat": Relay's messages (brief PR 4) get their own small
engine, and a shared messaging library would first have to move into the
framework. That rule exists because apps never depend on each other
([areas.md](../../../docs/areas.md)), not because the two products should
stay apart.

## Recommendation: yes, Relay absorbs Chat

One app, named Relay, that sends messages and files to your devices and the
people you trust. Chat stops being a separate app; its engine, screens and
server become Relay's messaging.

Why:

1. **Relay is about to write a second chat engine.** Brief PR 4 needs an
   outbox, sent/delivered/read, history that survives relaunches, attachments
   and reconnects without duplicates. Chat has all of that, built and tested
   over M0 to M5 (outbox, read cursors, SQLite store with search, edits,
   replies, reactions, photos, files, voice, notifications). Writing a
   smaller copy is the reinvention the project keeps refusing.
2. **It follows the standing rules.** Expand existing apps before adding new
   ones; fewer, fuller products. Two half-overlapping messaging apps is the
   opposite.
3. **It removes the dependency problem instead of working around it.** Inside
   one app, using chat-core is the app using its own code. Nothing has to be
   extracted into the framework on the strength of a single product.
4. **Relay's plan already has the slots.** "Communicate" is one of its four
   pillars, M3 is direct messages, M4 temporary rooms (host is the hub, which
   is a `ChatRepo`), M5 communities, and "AI asks as messages" is in M3,
   which is what Chat's on-device assistant already is.

Why not the other way round (Chat absorbs Relay): Relay is the broader
product with the agreed roadmap and transport (iroh), and "the name stays
Relay" is already decided. Chat's identity is a messaging client for one
server; Relay's is a network of your devices, which messaging fits inside.

Why not keep them separate and share chat-core through the framework: that
is the 2026-11-16 path, and it is still the right fallback if the
[spike](#step-3-messages-between-trusted-devices-on-chat-core) shows chat-core
fights peer-to-peer. But it gives the framework a messaging module with one
product using it twice, which is exactly the early extraction the
architecture rules warn about.

## What the merged app is

**Name:** Relay (provisional, like every name now).

**Who it's for:** Reuben first, his phone, laptop, iMac and servers; then the
people he trusts. Not a public messenger: no accounts with us, no push
(APNs stays a non-goal), and it never promises a message arrives while the
other side's app is closed. Messages wait until the other device is seen,
which is already Chat's outbox rule and Relay's "wait until seen".

**Front door:** task-first, as the LocalSend audit (#288) recommends, with
messages as a peer of transfer rather than hidden in a device view.

| Tab | What | From |
|---|---|---|
| **Chats** | Every conversation in one list, sorted by activity: a trusted device, a person, a room, a conversation on a chat server, the assistant. Search across all of them | Chat's list, search and conversation screens |
| **Send** | One flow, what then who: files, photos, a folder, text; to a nearby or trusted device, or by word code. Receive (enter or scan a code) sits beside it | Relay's Send and Receive (#268), reshaped by the audit's P1 |
| **Devices** | Nearby and trusted devices, presence, and each device's own view (send, message, later its capabilities) | Relay's device list, growing into Nearby and Connections in brief PR 5 |

Activity (transfers, requests waiting for approval) stays a section of Send
and of each device rather than a fourth tab, until there is enough of it to
need one. Settings holds pairing policy, servers and the account.

This replaces the plan's Nearby / Connections / Activity as the top level:
Connections becomes Devices, Activity folds in, and Chats is new.

## Chat's server mode

**Keep it, as one kind of connection, and freeze its features.**

- A chat server you run on a computer of yours appears in Settings, Servers.
  Its rooms appear in Chats beside device conversations. The account stays
  in the Keychain under the same key; `chat-server`, `chat-cli` and the
  deploy scripts keep working unchanged.
- It is the only persistent group conversation that works today, and the
  only thing in the app that exercises forge-net's HTTP and Server-Sent
  Events, server accounts and background catch-up. Dropping it would lose
  working features and the flagship's coverage for nothing.
- Frozen means bug fixes only: no new server features. The future of
  persistent groups is the plan's rooms (M4 on a person's device, M5 on
  Nostr). When M5 lands, decide whether the chat server is retired, kept as
  the "simple server" option, or bridged.

## What happens to chat-core

It becomes Relay's messaging engine and moves into Relay's folder.

- **Moves:** `examples/chat-core`, `chat-server` and `chat-cli` leave the
  root workspace for Relay's own workspace (for example
  `examples/relay/messaging/`), which also settles the three chat
  `workspace` crossings in `docs/boundaries.toml` (step 2 of the boundaries
  plan, with Relay as the destination instead of Chat). `examples/chat/rust`
  (account, alerts, assistant, the UniFFI API) folds into Relay's crate as a
  module.
- **`ChatRepo` gets a second implementation,** `PeerRepo`, over
  `relay/device/1`: a conversation per trusted device, messages as CBOR, ids
  chosen by the sender (chat-core already does this), attachments through
  Relay's iroh-blobs store instead of `POST /attachments`. The `HttpRepo`
  stays for chat servers. Later, temporary rooms (M4) are a `ChatRepo` whose
  hub is the host's device, and communities (M5) a Nostr one.
- **Changes chat-core needs for peer-to-peer** (the spike checks these first):
  - *Several sources.* `ChatClient` owns one repo today. The merged app needs
    a server and many devices at once: either one client per source under a
    small router, or one repo that routes by conversation. Prefer the first;
    it keeps chat-core as it is.
  - *The phone's copy is the only copy.* Chat treats the server as the truth:
    a corrupt database is moved aside and the client starts empty, and rooms
    evict sent messages past 500. For device conversations there is nothing
    to refetch, so eviction is off and a corrupt database is kept for
    recovery, not replaced.
  - *Ordering without a server clock.* The server stamps `sent_at`; a peer
    conversation orders by the sender's time and message id, with the
    receiver's arrival time as the tie-breaker.
  - *Read cursors per device* instead of per server account.
- **The Lean model of delivery** in the plan's verification table (each
  message stored once, its final state reported once) now covers one outbox
  instead of two.

## Existing data

Only Reuben has data, and only inside Device Lab (one bundle, so one
sandbox). Chat keeps its files in `Application Support/Chat/`
(`forge-chat.db`, `assistant.db`, attachments) and the account in the
Keychain as `chat.account`; Relay keeps its own directory, `device.key` and
`Documents/Relay`.

**Plan: nothing is converted.** The merged app opens Chat's directory and
Keychain item where they are, so existing chat server conversations, drafts,
the outbox and assistant conversations are there after the update. Relay's
paths don't change either. A test in the merge PR opens a Chat data directory
from before the merge.

Standalone installs (`org.forgemobile.chat`) are not migrated: they live in
another sandbox, and nothing important is in them. The merged app keeps
Relay's bundle id, so no new App ID is spent.

## Device Lab

- One Apps entry, Relay, instead of Chat and Relay. `examples/chat` leaves
  `apps` in `examples/device-lab/forge.toml`; the merged app keeps Chat's
  `forgechat://` scheme (notification deep links) and its permission texts
  (microphone, photos, notifications).
- "Open at Launch" set to Chat opens Relay's Chats tab.
- The flagship role moves with it: Chat's `requirements.toml` and
  `ACCEPTANCE.md` move into Relay. Chat's M6 phone sitting (M2 to M5, never
  run on the phone) is not run separately: its checks are run once on the
  merged app in step 1's phone check, so the phone time isn't spent twice.
- `docs/app-parity.toml` and `scripts/check-app-parity.sh` lose the `chat`
  entries in the same PR.

## Order of steps

Each step is one PR with its tests and a short phone check, after #268 lands.

### Step 0. This decision (docs only)

### Step 1. One app: Chat's screens inside Relay

Mechanical, no behaviour change. Chat's Swift moves into Relay's module as
the Chats tab (server mode as it is today), its Rust into Relay's crate and
workspace, its requirements and acceptance runbook into Relay. Device Lab
lists one app. `examples/chat` is deleted. Phone check: the shortened Chat
M6 sitting (send, attachment, voice, search, a notification) plus one Relay
transfer, in Device Lab.

### Step 2. Trusted devices (brief PR 3, unchanged)

As planned. Its screens go into the Devices tab.

### Step 3. Messages between trusted devices on chat-core (brief PR 4, changed)

Starts with a short spike: a `PeerRepo` test between two Relay cores on
Linux, through chat-core's outbox, covering the four changes above. If
chat-core fights it, fall back to the 2026-11-16 plan (Relay's own small
outbox) and keep only the screens shared; say so before building further.
Then device conversations in Chats, `relay send <device> --text` in the CLI.
Accepted as the brief says: a message sent while the laptop is off arrives
once when it is back, delivered and read on both sides, after relaunches.

### Step 4. The front door (with brief PR 5)

Chats, Send, Devices as above, applying the LocalSend audit's P1 and P2.
Device presence and status from PR 5 land here. This is the same work as
the UI Improvement programme's Relay redesign pilot (its PR 4): one PR, not
two.

### Then the plan as agreed

Reach (PRs 6, 7) carries messages and files everywhere; temporary rooms (M4)
as a `ChatRepo` on the host's device; Nostr communities (M5), where the chat
server's future is decided.

## Risks

- **A large app.** Chat's client (about 15,000 lines of Rust and Swift with
  tests, plus 2,400 for the server and CLI) and Relay's (about 7,000) in one
  app. Keep them as separate Rust modules and Swift
  folders (`Messaging/`, `Transfer/`, `Devices/`); the merge joins products,
  not code.
- **chat-core's server assumptions.** Covered by step 3's spike and its
  fallback.
- **Two message transports** (HTTP for servers, iroh for devices) for a
  while. Acceptable: both are `ChatRepo`s behind one engine, and the server
  is frozen.
- **Merge-time churn while Relay is mid-roadmap.** Step 1 waits for #268 and
  is mechanical; Relay's transfer work doesn't change.

## Docs that change

- [relay-platform-plan.md](relay-platform-plan.md): "Relay does not depend on
  Chat as a product", the "Another chat engine" and "Relay depending on
  another app" rows and the main screens, rewritten to this plan.
- [relay-m1-m2-brief.md](relay-m1-m2-brief.md): PR 4 and the rule "Relay
  takes nothing from Chat", in step 1, because #268 is still changing the
  brief.
- [chat-demo.md](chat-demo.md): marked as history, pointing
  here.
- The roadmap's demo list and `docs/boundaries.toml` (chat crossings) in
  step 1.

## Decisions (Reuben, 2026-11-17)

1. **Chat merges into Relay.** Relay absorbs Chat; the 2026-11-16 plan
   (keep them separate, share chat-core through the framework) is now only
   step 3's fallback.
2. **Chat's server mode stays,** as a connection kind frozen to bug fixes
   until Nostr communities (M5) decide its future.
3. **The front door is Chats, Send, Devices.**
