# Relay audit (November 2026): from identity to persistence

Status: an audit requested by Reuben on 2026-11-19, written the same day
from `claude/project-thread-ebenpi` (#340, stacked on #339). It treats
Relay as a product of its own ("a private network for your devices and the
people you trust"). It traces the code along the path a device takes, from
identity through pairing, trust, connection and delivery to persistence,
and ends with a [prioritised plan](#the-plan). It builds on the
[November review](relay-review-2026-11.md) and its agreed priorities (P0
to P6), and doesn't repeat them. Findings marked **new** are not in that
review.

## The short version

1. **The shape is right, and it now has one transport and one queue.**
   After #339 and #340, a trusted device is an iroh key. Everything goes
   over `relay/device/1`, and messages and files share one outbox and one
   conversation. The pure rule modules (`trust.rs`, `presence.rs`,
   `resolver.rs`), the golden wire fixtures and the exactly-once rule are
   worth protecting.
2. **Evidence is the biggest gap, not code.** None of the following has
   run on the iPhone: revocation, offline delivery, the step 2 routes
   (Tailscale, then Hetzner), the step 3 QR pairing or the step 4 order
   check. The step 3 and step 4 screens have not even been compiled for
   iOS. One sitting after the next combined build covers all of them.
3. **Five small defects are worth fixing before more features**
   ([below](#fix-now-small-and-in-scope)):
   - a message an older build can't read is retried for 30 days;
   - "Private network" lets traffic go through the relay;
   - trust silently allows AI asks;
   - revoking a device doesn't stop a transfer it already started;
   - a second trust offer replaces the first.
4. **Durable state is spread over three formats and two writes that aren't
   atomic together.** These are the JSON record, one JSON log per device
   conversation, and chat-core's own store. A crash between two of those
   writes can show a delivered message as failed. SQLite (P3) is the fix,
   and the audit says what it must hold in one transaction.
5. **A device has two identities.** One is its iroh key; the other is a
   random `device_id` in the record, used as the author of its own
   messages. The Keychain move (P4) should leave one.

## Path 1: device identity

| What | Where | Finding |
|---|---|---|
| The key | `node.rs` `device_key`: 32 random bytes in `device.key` (0600), written to a temporary file, synced, then renamed | Sound on Linux. On the iPhone, the file is in the app's data container, which backups include (P4, known) |
| A damaged key | `Corrupt` error, never a new identity | Right: a silent new identity would look like a new device |
| **A second identity (new)** | `record.device_id`, 16 random bytes made on first open (`lib.rs`), used as `author_id: relay:<device_id>` for this device's own messages. Other devices know it only by its endpoint id | Two identities that can disagree. A restored backup carries both, and a new key with an old `device_id` would mix them up. P4 should make the endpoint id the only device id, and own messages should be authored by it |
| A corrupt record **(new)** | `RelayCore::new` fails on an unreadable `relay.json`, and the app only says "Relay's saved data couldn't be read." | Nothing in the app sets the file aside and starts again (`Document::start_fresh` exists). Starting again would keep the key, so the device keeps its identity and only its trust list is lost. P3 changes the file anyway, so the recovery belongs with it |

## Path 2: pairing (word codes)

| What | Where | Finding |
|---|---|---|
| The code | `code.rs`, `code_send.rs`, `code_session.rs`: SPAKE2 with key confirmation over iroh, the session modelled in Lean, Bonjour `_forgerelay._udp` | The strongest part of Relay. After #339, the QR code is the word code |
| Who is offered trust | The other side's endpoint id comes from the authenticated QUIC connection, and the code proves both sides typed the same words | Right: knowing the code is what earns the offer, and the person still confirms it (Ask to Trust) |
| **One offer at a time (new)** | `trust_offer: Mutex<Option<TrustOffer>>`: a second code transfer before the first offer is answered replaces it | Rare, but the first device then can't be trusted without another transfer. Keep a short list, or say that the earlier offer was dropped |

## Path 3: trust

| What | Where | Finding |
|---|---|---|
| The rules | `trust.rs`: Trusted, Revoked, Forgotten; three policies; revocation never undone by a code | Well tested, pure |
| Checked when | `DeviceHandler::accept` asks `rights()` once, when the connection opens, before reading anything | Right for refusal |
| **Revocation mid-transfer (new)** | A file from a device that is revoked while its transfer runs is still saved and answered Done: rights aren't asked again | Ask again before saving, and refuse if the device is no longer trusted. A few lines |
| **Trust bundles AI asks (new)** | `Ask` is answered for any trusted device, even when "Can Send to This Device" is off; only `relay serve-ai` (or AI Lab A2) turns answering on at all | Reuben's rule is that trust never silently grants unrelated permissions. Before AI asks reach the app, they need their own per-device switch, off by default. This is the first grant, and it fits the M6 model |
| Trust is one-sided | Each side trusts the other separately, and a send to a device that doesn't trust this one ends as Refused | Fine for pairs. My Devices (P5) is the fix for a network |

## Path 4: connection

| What | Where | Finding |
|---|---|---|
| Who is dialled where | `resolver.rs` `plan` puts together Bonjour (nearby), what the device told (Hello), and where it was last reached; filtered by the Connections mode | Good separation: a key says who, never where |
| The route check | `device::send` connects, then closes if the path's kind isn't allowed | The connection is made before it is refused. In Local mode with a relay configured, iroh may still reach the device through the relay and only then close. Building the address without the relay, and skipping key lookup, in Local and Private modes would avoid the attempt |
| **Private allows the relay (new)** | `ConnectionMode::Private` allows every kind but `Internet`, so `Relay` is allowed; the app says Private "never connects over the open internet" | Traffic through Hetzner crosses the internet (end-to-end encrypted, but it does go there). **Decided by Reuben (2026-11-19): Private means no relay.** The relay is used only in Hybrid |
| **Unknown messages (still open from review item 7)** | `DeviceMsg::decode` fails on a variant it doesn't know, and an unexpected item is `connection("expected a note or a file")`. Both are connection errors, which the outbox retries (`Outcome::Retry`) for 30 days | Mixed builds are normal (phone, laptop CLI, server). Answer an item this build can't read with `Refused { "This device's Relay doesn't support that yet" }`, so the sender ends it at once, and let `Hello` list the features a build speaks |
| One connection per item | `device::send` dials for every message or file | Known (review item 8). It matters once the relay route is in use |

## Path 5: delivery

| What | Where | Finding |
|---|---|---|
| One queue | `outbox.rs`: `Record::deliveries`, one worker per device, oldest first, backoff 1 s to 5 min, retried at once when the network changes or the device is seen | Right, after #340 |
| Exactly once | The receiver saves, remembers the id (45 days), then answers; messages are deduplicated by message id in the log | Right, and tested (a lost answer, a relaunch, a retry after revocation) |
| Damaged content | iroh-blobs checks every chunk; a damaged file is refused as "Verification failed" and isn't retried | Tested since #340's review fixes |
| **Two writes, not one (new)** | Queueing writes the conversation log, then the record. Ending writes the record, then the log (`message_ended`). After a relaunch, `settle_messages` marks a queued message with no delivery as Failed | A crash between the two writes of an *ending* leaves a delivered message marked Failed. A retry is harmless, because the receiver deduplicates it, but the state is wrong. SQLite should put the delivery's end and the message's status in one transaction |
| History trimming | The history keeps 500 entries, so a long-pending delivery can outlive its entry | Handled in the #340 migration. In P3, deliveries keep their own name and size, never just a pointer into the history |

## Path 6: persistence

| Store | Format | Finding |
|---|---|---|
| `relay.json` | One versioned JSON document, written atomically (temporary file, sync, rename) and rewritten whole on every change: identity, devices, deliveries, 500 history entries, 45 days of received ids | Sound but O(everything) per change, and received ids grow with traffic (P3, known) |
| `devices/<id>/messages.json` | One JSON log per device conversation, rewritten whole on every message *and every status change*, never trimmed | **New:** grows without bound, and each message costs a full rewrite |
| chat-core's store for each device | chat-core's own files, a cache of the log | **New:** every device message is stored twice. P3 should make one database the truth, which chat-core reads through `PeerRepo` |
| The assistant | SQLite (forge-storage) | Already the target format |
| The blob store | iroh-blobs `FsStore`, held by named tags | Right; let go of after 7 days for receives, at the end of a delivery for sends |

What P3's database must hold, each in **one transaction**:

1. receiving an item: save the message or the file's record, and remember its id;
2. queueing: the conversation message as Queued, and its delivery;
3. ending a delivery: remove it, and set the message's status (and the file's history entry);
4. migration from the JSON files, once, kept idempotent as in #340.

## Evidence: what has and hasn't run on the iPhone

| Check | State |
|---|---|
| Word-code transfer, 1 GB resume, folders, one copy per receive | Passed (2026-11-15 to 2026-11-18) |
| Trusted note and file, both ways | Passed (2026-11-17) |
| P0 revocation | Not run |
| P0 offline delivery (outbox) | Not run |
| Step 2 routes: Tailscale, then Hetzner with Tailscale off | Not run |
| Step 3: QR as word code, re-trust after the TCP pairing is gone | Not run; the Swift is not compiled |
| Step 4: message, photo, message in order, both ways | Not run; the Swift is not compiled |

## The plan

In order. Each step is one PR unless it says otherwise, and each says what
it proves.

### 1. Close the consolidation (now)

- Merge #339, then #340 (retargeted to main), once CI passes.
- One combined Device Lab build, then **one phone sitting of about 20
  minutes** covering all the "Not run" rows above, each recorded
  separately in the ledger, a failure included.

### 2. Fix now: small and in scope

One PR, no new features. Each fix ships with its test.

- **Unknown items refused, not retried:** `Refused` for an item this build
  can't read, and features in `Hello`. Test: an item from a "newer build"
  ends at once with a clear reason.
- **Revocation stops a running transfer:** ask `rights()` again before
  saving. Test: revoke during a fetch, and nothing is saved.
- **AI asks behind their own switch,** per device, off by default. Test:
  a trusted device can't ask until the switch is on.
- **Trust offers don't replace each other.** Test: two code transfers,
  then two offers.
- **Private means no relay** (Reuben, 2026-11-19): only Hybrid uses the
  relay, and Local and Private don't try a relay path at all. Test: in
  Private, a device reachable only through the relay is not reached, and
  the reason says so.

### 3. P3: one database

Built on `claude/relay-sqlite` (stacked on #344): `store.rs`. One part
differs from the plan below. chat-core still keeps its own store of
device conversations, as a cache that it can rebuild through `PeerRepo`,
the same way it caches a server. Relay's database is the truth, and
nothing depends on the cache. Removing the cache would mean changing
chat-core, so that waits until it is needed.

- SQLite (forge-storage) holds devices, deliveries, received ids, the
  history and device conversations (messages and attachment locations),
  with the four transactions above.
- chat-core reads device conversations from it through `PeerRepo`, so
  each message is stored once.
- The JSON files are migrated once (tested from saved files, run twice),
  then kept read-only for one release.
- A record that can't be read is set aside, and Relay starts again with
  the key kept.

### 4. P4: one identity, in the Keychain

- On the iPhone, the key goes into the Keychain with "this device only";
  Linux keeps the 0600 file.
- The endpoint id becomes the only device id, and `device_id` is
  retired. Done on 2026-11-21, with one difference from this line: own
  messages are authored by a local label, `relay:me`, not by the endpoint
  id. The author of our own messages is never sent (the other device
  knows the sender by the connection), and a label keeps history this
  device's own after a backup restore, which is a new key and so a new
  identity. An older build's own messages move to the label once at
  open, in one transaction, each with a new stamp so Chats' cache sees
  the change.
- The migration matrix from the review's refinement 4: fresh install,
  upgrade (same key), backup restore (new identity), reinstall, a lost
  phone, revocation.

### 5. The whole experience, proven ("Relay 1.0 foundations")

Reuben, 2026-11-20: finish the foundations (one identity, one network,
one delivery system) and prove them together before My Devices. Nothing
new starts first. The acceptance run, on the iPhone and Linux, then
across more than two devices:

1. Upgrade: install over existing Relay data. Trusted devices,
   conversations and history are all still there (P3), and the laptop
   still trusts the iPhone without pairing again (P4).
2. Pair a new device with a word code, typed and scanned.
3. Messages and files both ways: a message, a photo, a message, in
   order; a folder.
4. Interrupt a large transfer (stop the receiver, or change network) and
   check it recovers and arrives once.
5. Revoke a device while its transfer runs: the file isn't saved (#344).
6. The Keychain identity survives a restart, and isn't carried to a new
   phone by a backup taken after the update (P4).
7. Private and Hybrid on their own: in Private, a device reachable only
   through the relay isn't reached, and nothing connects (#344); in
   Hybrid it is reached.
8. Both the Relay app on its own and Relay inside Device Lab.

Each check is recorded on its own, a failure included. The run becomes
the regression check before every Relay release.

#### Results, 2026-11-21

Device Lab 1.29921164 on the iPhone 15 Pro Max, `relay` from main
(c03f4d8f) on the laptop, plus two more `relay` devices on the laptop.

| Check | Result |
|---|---|
| 1. Upgrade | Passed on both sides: trust, conversations and history kept; the laptop's JSON moved into `relay.db` once |
| 2. Pair a new device | Passed with a typed code; scanning not run |
| 3. Messages and files both ways | Passed: message, ePub, photo, message, and a folder, in order, once each |
| 4. Interrupted transfer | Passed: 500 MB, Wi-Fi off at 135 MB, resumed from there, verified, once |
| 5. Revoke during a transfer | Passed (not saved), but the download ran to the end first: fixed in #362 |
| 6. Identity survives a restart | Passed; the backup-to-a-new-phone part not run |
| 7. Private and Hybrid | Passed: on 5G without Tailscale, Hybrid went through the relay and Private waited |
| 8. Relay in Device Lab | Passed; the standalone Relay app not run (no free app slot) |
| My Devices, step 1 | Passed: offered once, revoked once (#354) |

Other findings, fixed in #362: on one Wi-Fi the route read "Direct,
internet" over the home's public IPv6; the device-list refusal was titled
"Not Sent"; `relay messages` printed attachments as empty lines.

### 6. P5: My Devices

The introducer, then the signed device list, with the review's authority
model.

Acceptance: pair a new device once, and every device learns of it.
Revoke it once, and every device refuses it.

Step 1, the introducer, is built (2026-11-21, [relay-my-devices.md](relay-my-devices.md)):
tested on Linux with three devices; Swift not yet tried on a phone.

### 7. Away from home

Tailscale, then the Hetzner relay with TLS, under the Connections modes as
decided in step 2.

### 8. P6: the encrypted mailbox

The mailbox comes only once direct delivery and My Devices are
dependable.

### Parked: sites over Relay (Reuben, 2026-11-21)

A website that only your Relay devices can open, never the normal
internet. The site listens only on its host's localhost, and a new
`relay/web/1` channel carries HTTP to it over the existing iroh
connection. It needs a separate `web` right per device, never implied by
trust, and revoking a device cuts it off. Computers view it through
`relay web <device>` (a local address for any browser). The iPhone
views it in Relay's own web view, whose requests Rust fetches over iroh.
The iPhone can also host a folder as a site, but only while Relay is open
(iOS suspends it otherwise, which is the behaviour he wants). It is a new
feature, so it waits for the Relay 1.0 foundations like everything else
here.

## What not to change

- **Word codes** are for devices that aren't trusted, and they stay as
  they are.
- **The pure rule modules and golden fixtures:** new rules (grants, the
  device list) follow the same shape.
- **iroh-blobs for content:** verification, resume and one copy per
  receive come from it, not from Relay code.
- **Framework work:** Relay brings changes to ixOS only when it needs one
  (Reuben, 2026-11-19).
