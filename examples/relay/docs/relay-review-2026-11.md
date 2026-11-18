# Relay: product and architecture review (November 2026)

Status: a review requested by Reuben on 2026-11-18, written the same day
from main at `ac41f1f` plus the two open M2 PRs, #332 (connection
resolver, own servers) and #333 (presence and status). It changes no code
and no decision. Its recommendations are proposals for Reuben; where one
is adopted, the canonical plan ([relay-platform-plan.md](relay-platform-plan.md))
or the current milestone ([relay-reliable-delivery.md](relay-reliable-delivery.md))
records it. It follows the [open-ended design
principles](relay-open-ended-design-principles.md): it reviews what Relay
is today and what it needs next, and leaves what Relay might become open.

## The short version

1. **Most of the "what to do next" list is already written; what it lacks
   is evidence and consolidation.** Transfers between trusted devices
   already run on iroh, the outbox exists, device conversations run on
   chat-core, and the resolver is in #332. What is missing is the phone
   checks (revocation, the offline outbox demo), and removing what the new
   path replaced.
2. **Relay still has two networking stacks.** The old QR pairing, TCP and
   Noise path (`link.rs`, `wire.rs`, `forge-devproto`, `_forgerelay._tcp`)
   still carries QR pairing and AI asks. Finishing brief PR 3b and deleting
   it is the single biggest simplification available.
3. **Relay has two delivery queues and two kinds of text.** Notes go
   through Relay's outbox (`Record::deliveries`); messages go through
   chat-core's outbox. A person sees "notes" in the history and "messages"
   in Chats. One concept (a message, which may carry files) and one
   durable queue underneath would match Reuben's "a single, durable
   delivery system for messages and files".
4. **Everything durable lives in one JSON file** rewritten on every
   change: identity, history, trust, the outbox and 45 days of received
   ids. That is fine at today's volume and the wrong base for exactly-once
   delivery at any real volume. SQLite (already in forge-storage, already
   used by the assistant) gives transactions for free.
5. **The genuinely missing product piece is the network itself.** Today
   Relay is a set of pairs: each device trusts each other device on its
   own, and each revocation has to be done on every device. "Your devices.
   One network." needs a shared, signed list of *your* devices. Borrow
   Syncthing's introducer first, then a signed device list in the style of
   Tailscale's Tailnet Lock or Matrix cross-signing, before rooms or
   capabilities.
6. **The second missing piece is somewhere for things to wait.** An
   iPhone can't receive while Relay is closed, and with a free Apple team
   there is no push. Phone and laptop are rarely open at the same moment,
   so foreground-only delivery will often wait for days. An always-on
   device you own (the Hetzner server, or the laptop) holding items for
   your other devices, encrypted so it can't read them, solves this
   without a third-party server.
7. **Borrow, don't build:** device features from KDE Connect's model,
   discovery and relaying from Syncthing's, the terminal from `iroh-ssh`,
   distributed inference from existing runtimes, folder sync from
   Syncthing itself. Details in [what to borrow](#what-to-borrow).

The recommended order is at the [end](#recommended-order).

## What exists today

Relay is about 7,900 lines of Rust in `rust/src` (plus chat-core, the chat
server and its CLI under `messaging/`), a SwiftUI app hosted in Device
Lab, and the `relay` command for Linux.

| Area | Code | State |
|---|---|---|
| Word codes | `code.rs`, `code_send.rs`, `code_session.rs` | iroh, iroh-blobs, SPAKE2 with key confirmation, Bonjour `_forgerelay._udp`; session state machine modelled in Lean. Passed on the iPhone (1 GB at 9.5 MB/s; folder each way, interrupted and resumed, 74.6 MB peak) |
| Device identity | `node.rs` | One iroh endpoint per device, key in `device.key` (0600) on every platform, the iPhone included. #332 also keeps the UDP port across launches |
| Trust | `trust.rs` | Pure rules: Trusted, Revoked, Forgotten, three pairing policies, "receives from" switch. Unit-tested; revocation not yet checked on a phone |
| Trusted delivery | `device.rs`, `outbox.rs` | `relay/device/1` (CBOR with golden fixtures), one item per connection, files fetched by iroh-blobs, receiver remembers ids 45 days for exactly-once. Outbox in the JSON record with one worker per device and backoff. 200 MB storage check passed; offline-then-arrive check not yet run |
| Device conversations | `device_chat.rs` | chat-core with a `PeerRepo` per device; truth in `devices/<id>/messages.json`; text and replies only |
| Chat servers | `messaging/chat-core`, `chat-server` | The merged Chat app: an HTTP chat server with SSE, its own outbox and store |
| Assistant | `chat/assistant` | On-device model conversations in SQLite |
| AI asks | `link.rs` (protocol 2) | `relay serve-ai` / `relay ask`, still on TCP and Noise |
| Old pairing | `link.rs`, `wire.rs`, `record.rs` peers | QR code, TCP, Noise `XXpsk3`, pair secrets in the JSON record (friction A17) |
| Reach (open PRs) | #332 `resolver.rs`; #333 `presence.rs` | Local, private (Tailscale), internet routes with a Local / Private / Hybrid setting; own relay and pkarr lookup by URL, off until set; presence Nearby / Reachable / Offline and a Status message. The two PRs conflict and need one to rebase on the other |

## Assessment by area

Reuben's table, with a verdict from the code.

| Area | Reuben's view | What the code says |
|---|---|---|
| Transfer | Highest priority; needs production reliability | Already the most mature part: one store, collections, resume, verification, one copy per received item. What remains is the M1 demo on a phone and the 1 GB free-space check deferred from stage 3 |
| Messaging | Good reuse of Chat | Right call, with one cost: notes and messages are two things doing one job ([below](#2-one-delivery-system-not-two)) |
| Networking | Consolidate TCP/Noise and iroh | Transfers are already on iroh. What is left is deleting the old path, which still owns QR pairing and AI asks ([below](#1-one-networking-stack)) |
| Device identity | Needs Keychain and recovery work | Agreed, and more urgent than it looks: the key file is in the app's data container, which iPhone backups include ([below](#4-device-identity-and-the-keychain)) |
| Permissions | Most valuable decision | Agreed. Today it is one switch ("can send to this device"); the grant model is rightly deferred to M6. Keep grants as a local table until delegation is needed |
| Distributed AI | Interesting future | Agreed; borrow the runtimes, keep Relay as transport and permission check ([below](#what-to-borrow)) |
| Extensions | Could make Relay much bigger | Agreed, and the biggest risk to focus. Nothing should start before the network piece exists |

### Three strong decisions worth protecting

Reuben named one networking foundation, trust separate from permissions,
and local-first operation. The code holds to all three, and three more
deserve the same protection:

- **Identity, discovery, connectivity and authentication are separate.**
  The resolver in #332 dials by address but accepts only the trusted
  endpoint id. A key says who, never where.
- **Pure rule modules.** `trust.rs` and `presence.rs` hold the rules with
  no I/O, so the revocation rule is tested exactly. Keep this shape for
  grants.
- **Golden wire fixtures.** Every `relay/device/1` message has pinned
  bytes, so two builds keep understanding each other.

## What is genuinely missing

In order of how much it limits Relay now.

### 1. One networking stack

The platform plan says Relay ends M1 with iroh as its only transport. Today
`lib.rs` still opens a TCP port, advertises `_forgerelay._tcp`, and answers
QR pairings and AI asks through `link.rs`. The SwiftUI app still browses
both Bonjour services. Every new feature has to decide which path it
belongs to, and every bug can be in either.

**Recommendation:** finish brief PR 3b before more M2 work merges on top:
AI asks as a `relay/device/1` message (or their own ALPN on the same
endpoint), QR codes carrying a word code (as the brief already says), then
delete `link.rs`, `wire.rs`, the peers in the record and the TCP service.
QR pairings already made are carried over once by the pair-secret upgrade,
or simply re-trusted with a code: only Reuben's own devices are paired
  the old way.

### 2. One delivery system, not two

There are two queues: Relay's outbox (`Record::deliveries`, notes and
files) and chat-core's outbox (device conversations), joined by the
delivery scheduler waking chat-core. There are also two kinds of text: a
note appears in the transfer history, a message in Chats. The person can
send the laptop text two ways and see it in two places.

**Recommendation:** retire notes as a separate kind. Text to a device is a
message in its conversation; files sent to a device appear in that
conversation as an attachment whose bytes still travel by the outbox and
iroh-blobs. Keep the two state machines the milestone asks for (a message
is sent and delivered; a file is fetched, verified and saved), on one
durable queue. This is the "single, durable delivery system" in Reuben's
list, and it removes a concept rather than adding one. The word-code
transfer stays as it is: it is for devices that aren't trusted.

### 3. Durable state in a database

`record.rs` holds the device's identity, 500 history entries, the trust
list, the outbox and every received id from the last 45 days in one JSON
document, saved whole after each change. Device conversations each have
their own `messages.json`, also saved whole. The exactly-once rule
("save the item, remember its id, then answer") is only as strong as that
save, and the work of each save grows with everything in the file.

**Recommendation:** move trust, deliveries and received ids into one
SQLite database (forge-storage's `sqlite` feature, which Relay already
links for the assistant) before the data grows, with "save the item and
remember its id" in one transaction. History can follow. This is cheap
now, with one user and a migration from one file, and expensive later.

### 4. Device identity and the Keychain

`device.key` is a file in Relay's data directory on the iPhone. iPhone
backups include the data container, so restoring a backup onto a new
phone, or keeping the old one, gives two phones the same identity: both
are "trusted", and revoking one revokes both.

**Recommendation:** move the key into the Keychain with a "this device
only" accessibility (`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`),
so it never leaves the phone through a backup and a restored phone gets a
new identity that has to be trusted. That is the behaviour people expect
from a lost or replaced phone. The Swift side owns the Keychain (rule:
Swift does the Keychain), Rust gets the bytes at start-up. Recovery then
means re-trusting the new phone from any other device, which item 5 makes
one step instead of one per device.

### 5. Your devices as one network

Trust today is pairwise. With a phone, a laptop, an iMac and the Hetzner
server, full trust takes six separate pairings, and revoking a lost phone
means revoking it on three machines, one at a time, each of which must be
reachable. Nothing tells the laptop that the iMac is yours. This is the gap
between "devices that know each other" and "your devices, one network",
and every later direction (computing, social, Workshop) needs it.

The plan defers a person key until people and groups need one. Your own
devices need a lighter version sooner:

- **Step 1, an introducer (Syncthing's design).** Mark one trusted device
  as an introducer: devices it trusts are offered to you to trust here,
  and devices it revokes are revoked here. One device (the always-on
  server, or the phone) becomes the place you manage the network from. It
  needs no new cryptography: the introducer's word is accepted only over
  its own authenticated connection.
- **Step 2, a signed device list.** A small append-only log of "device X
  added" and "device Y revoked", each entry signed by a device already in
  the list, synced between your devices over `relay/device/1`. Tailscale's
  Tailnet Lock and Keybase's sigchains are this shape; Matrix
  cross-signing is a lighter one. This is where the person key from the
  plan comes in, as the root of the list, with its recovery decided then.

Step 1 fits after M2 and before rooms. It is the first thing that makes
Relay a network rather than a set of pairs.

### 6. Somewhere for things to wait

Foreground delivery is the right M1 boundary, and iOS won't change it: the
phone can't receive while Relay is closed, and the free Apple team has no
push. In practice the phone and the laptop are seldom open together, so
a message to the phone can wait until Reuben happens to open Relay while
the laptop is also running it.

**Recommendation:** let a device you own and that stays on (the Hetzner
server running `relay listen`) hold items for your other devices. The
sender encrypts each item to the recipient's key, so the holder stores
bytes it can't read (Syncthing's "untrusted device" encrypted folders work
this way). When the phone opens Relay, it collects from the holder over the
route the resolver finds. This keeps "local-first, not local-only", needs
no third party, and works with the free team. It needs item 5 first, so
the holder knows which devices are yours. A grant for "hold items for my
devices" fits the M6 model when it comes.

### 7. Protocol evolution

`relay/device/1` grows by adding variants to one CBOR enum (#333 adds
`StatusAsk` and `Status`). `Hello` carries a version that stays 1. An older
build that receives a message it doesn't know fails to decode it and drops
the connection, so the newer side sees a connection error, not "that
device doesn't support this". With the phone, the laptop's CLI and the
server on different builds, mixed versions are the normal case.

**Recommendation:** answer an unknown message with
`Refused { reason: "unsupported" }` instead of closing, and let `Hello`
carry the list of features a build speaks, so a sender can tell before it
asks. This is cheap now and keeps every later message (status, grants,
AI asks) safe to add.

### 8. Connections reused

Every item opens a new iroh connection. On the local network this costs
little; through a relay or across the internet each message pays a full
handshake. The stage 3 notes already deferred connection arbitration.
**Recommendation:** keep one connection per trusted device while either
side has work, with one stream per item, once M2 routes are in use.

### 9. Our servers, properly

The Hetzner relay and lookup server run at plain `http://` URLs on an IP
address. Traffic through the relay is still end-to-end encrypted by QUIC,
but:

- **TLS.** `iroh-relay` can get its own certificate (ACME) given a domain
  name; give it one before Relay depends on the server.
- **What the lookup record says.** #332 publishes every address the
  device has, unfiltered, including home-network and Tailscale addresses,
  to a server anyone who knows the endpoint id can query. Endpoint ids
  are not secret (every connection presents one, and Bonjour TXT records
  carry them). Publish only the
  relay URL and public addresses; private routes already travel in Hello
  between trusted devices.

## Where Reuben's next steps stand

| Reuben's step | Where it stands |
|---|---|
| Finish reliable trusted-device delivery | Built (#314, #317, #319, #321). Left: the revocation check and the offline-then-arrive demo on the phone |
| Complete physical iPhone testing: revocation, persistence, large transfers | Large transfers and storage passed (1 GB resume, 200 MB once). Revocation and the outbox demo are open; both fit one 15-minute sitting |
| Consolidate transfers onto iroh | Transfers are done. What remains is the TCP path for QR pairing and AI asks ([1](#1-one-networking-stack)) |
| A single durable delivery system for messages and files | Two queues today ([2](#2-one-delivery-system-not-two)) on a JSON file ([3](#3-durable-state-in-a-database)) |
| Build the connection resolver before internet-wide connectivity | #332 has both, with servers off until set. Merge it with #333 sorted out, then run the Tailscale test before the Hetzner one |

## What to borrow

Projects already assessed (magic-wormhole, croc, LocalSend, libp2p,
Tailscale's wush and derphole, Nostr and Marmot) are in
[relay-transfer-plan.md](relay-transfer-plan.md) and
[relay-rooms-protocols.md](relay-rooms-protocols.md). These are new, or
newly relevant.

| Project | What it does that Relay needs | Borrow | When |
|---|---|---|---|
| **Syncthing** (MPL-2.0) | The closest prior design to Relay's network: device id from a key, signed announcements to a discovery server, relays, introducers, auto-accept, and "untrusted" devices that hold encrypted data | The introducer ([5](#5-your-devices-as-one-network)) and the untrusted holder ([6](#6-somewhere-for-things-to-wait)) as designs. Later, folder sync by running Syncthing itself over a Relay connection rather than building sync | After M2 |
| **KDE Connect** (GPL) | "Connect your devices" for a decade: per-device plugins (battery, clipboard, notifications, run commands, share, find my phone), each device announcing which plugins it offers and accepts, each plugin switched on or off per device | Its capability model for M6: the announce list (`Status.offers` in #333 is the start), per-device switches as the first grant interface, and its list as a check on what people use. Not its protocol (TLS over TCP, JSON) | M6 |
| **Tailscale Tailnet Lock** and **Keybase sigchains** | Signed, append-only lists of which keys belong to a network or person, with revocation that every member can check | The shape of the signed device list | After the introducer |
| **Matrix cross-signing** | A person key that signs device keys; a new device verified once by an existing one | Lighter alternative to the signed list; also the model for the plan's person key | With person identity |
| **iroh-ssh / dumbpipe / sendme** (n0, MIT/Apache) | SSH and plain pipes over iroh connections | Already the plan's terminal; dumbpipe also covers "reach a service on my laptop" without new protocol work | M6 |
| **iroh's own address lookup** | DNS / pkarr publishing (#332 uses it) and an mDNS lookup | Keep Bonjour on the iPhone (multicast needs an entitlement); the `relay` command could use iroh's mDNS lookup instead of its own `mdns-sd` code | Any time |
| **iroh-gossip / iroh-docs** | Broadcast and a synced key-value store between endpoints | Only if the signed device list or presence needs to reach many devices; not needed for one person's handful | Watch |
| **llama.cpp RPC, Ollama, exo** | Running a model on another machine, or across several | Distributed AI stays "Relay carries an OpenAI-style request to a device that has a model" (`relay serve-ai` already does). Splitting one model across machines is exo's or llama.cpp's job | M6, M8 |
| **Biscuit, UCAN** | Capability tokens that can be narrowed and handed on offline | Not yet: a device checking its own local grant table needs no tokens. Tokens matter only when one device delegates a grant to another (M8) | M8 |

Rejected again for completeness: building our own sync engine, our own
overlay VPN (Tailscale, Nebula and headscale exist, and Relay already uses
Tailscale as a route), and our own group encryption (Marmot / MLS stays).

## Product notes

- **The front door.** Relay's tabs (Chats, Send, Devices) hold device
  conversations, chat-server conversations and the assistant. That is
  three kinds of conversation in one list before the network is even
  visible. Worth watching in the M1 demo whether the device conversations
  are easy to find among them. No change proposed now.
- **Name the network.** Once the introducer exists, "My Devices" (the
  list everything else is checked against) is the thing to show first on
  Devices; "Nearby" and other people's devices come under it.
- **The tests are the product.** The messaging and transfer test and the
  personal network test in the platform plan are good acceptance tests.
  Add one between them for the network piece: *trust a new device once,
  from one device, and all your devices accept it; revoke it once, and all
  refuse it.*

## Recommended order

M1 and M2 stay as they are; this only adds what the review found, in the
places it fits.

1. **Finish M1 on the phone:** the revocation check and the
   offline-then-arrive demo, in one sitting with the next combined build.
2. **Sort out #332 and #333** (one rebases on the other) and merge them.
3. **PR 3b, now with deletion:** AI asks on iroh, the TCP path removed,
   the device key in the Keychain with "this device only". Item 7
   (unknown messages refused, features in Hello) rides along: it is a few
   lines.
4. **One delivery system:** notes become messages, files become
   attachments in a device's conversation, and trust, deliveries and
   received ids move into SQLite.
5. **M2's tests:** Tailscale first, then Hetzner with a domain name, TLS
   and a lookup record without private addresses.
6. **My Devices:** the introducer, then the new network test above.
7. **The holder:** the always-on server keeps encrypted items for your
   devices.

Then the choice Reuben already reserved for after M2 (computing, social,
rooms, capabilities) is made on top of a real network. Steps 3 and 4 make
Relay smaller; 6 and 7 are the first features that only a personal network
can offer.
