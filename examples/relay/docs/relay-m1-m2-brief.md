# Relay M1 to M2: implementation brief

Status: written 2026-11-16 from Reuben's product specification. It turns
the first six PRs of the [platform plan](relay-platform-plan.md#how-the-work-lands)
into concrete work on today's code. The plan stays the roadmap; this brief
only says how to get from where the code is to the end of M2. It changes no
crates: everything stays in `examples/relay/rust` as modules.

**Goal:** finish M1 (a production transfer on one stack), then make M2 the
defining milestone: the phone finds and connects to your own computers from
anywhere, with nothing configured by hand.

## Where the code is (2026-11-16)

| Part | Files | State |
|---|---|---|
| Paired devices | `lib.rs`, `link.rs`, `wire.rs`, `record.rs`, `forge-devproto` | TCP + Noise `XXpsk3`, keyed by the QR code's one-time secret; notes, files (32 KB chunks, SHA-256), AI asks (protocol 2); peers and history in one JSON record, pair secrets included (friction A17) |
| Word codes | `code.rs` (receive), `code_send.rs` (send), `code_session.rs` | iroh 1.4 with relays off, iroh-blobs `FsStore`, SPAKE2 plus key confirmation, Bonjour `_forgerelay._udp`; one file per code; a fresh endpoint and key for every session; nothing goes into the history |
| Proven on the iPhone | | 1 GB Linux to iPhone at 9.5 MB/s, iPhone to Linux, both at the first code and verified (2026-11-15) |
| Not measured | | The app's memory on the phone during a transfer: the readout is in both sheets, no reading recorded |

## One transfer path

Relay ends M1 with iroh as its only transport. The migration keeps what
pairing and transfer already do well:

1. **One endpoint per device, with a key that lasts from the start.**
   `RelayCore` owns one iroh endpoint, bound when it starts, whose secret
   key is the device's identity. From PR 1 the key is saved and reused on
   every launch, so it never changes under connections already made
   ([identity and storage](#identity-and-storage)). Word codes stop binding
   their own endpoints and use it, each protocol on its own ALPN.
2. **Pairing is a policy set in Settings** (Reuben, 2026-11-16). The
   basic functions stay and the higher ones are built on them, never in
   their place. The policies are choices, not a ladder of trust:

   | Policy | What it does |
   |---|---|
   | **Codes only** | A word code moves one transfer; nothing is remembered. Always available, whatever the setting |
   | **Ask to pair** (default) | After a code's SPAKE2 confirmation both sides know each other's endpoint id; Relay offers "Trust this device" and saves its id and name |
   | **Pair automatically** | A device you already know (paired before, including by the old QR pairing) is paired again by itself the next time a code or Bonjour connects you; new devices still ask |

   A revoked device is never paired again by itself under any policy,
   even with a valid code or an old pairing; a code can still move one
   transfer to it if the person accepts, and only the person can trust it
   again. The QR code carries the same word code, so scanning is just a
   faster way to type it.
3. **Paired traffic moves onto iroh.** Notes, files and today's AI asks
   (`serve-ai` / `ask`, moved as they are; AI as a capability is M6)
   between trusted devices go over `relay/device/1` (CBOR messages reused from
   `wire.rs`; files by iroh-blobs). The connection is authenticated by the
   endpoint ids, so no pair secret is stored.
4. **Then the TCP path goes.** Once the app and the CLI use only iroh,
   `link.rs`, the Noise intro and the `_forgerelay._tcp` service are
   deleted. `forge-devproto` stays: the device tools still use it.
   Devices paired the old way are paired again on iroh by the pairing
   policy above: automatically, or after asking.

## PRs to the end of M1

Each PR carries its tests and one short phone check (about 10 minutes,
batched, built before the phone is needed). No test-only PRs.

### 1. Production transfer core

- The core's own endpoint and its lasting key (step 1 above), kept in a
  file in Relay's data directory (0600) on every platform for now; codes
  on it.
- Several files and folders as one transfer: an iroh-blobs collection
  (`HashSeq`) with relative paths, unpacked under one folder in the inbox.
- The store survives a relaunch, and a send or receive resumes from what is
  already there (the spike resumed 4 GB this way on Linux).
- Every code transfer lands in the history, both directions, with errors.
- Cancellation on both sides, and errors the screen can show (wrong code,
  code gone, sender busy, disk full, verification failed).
- **Accepted when** a folder goes Linux to Linux both ways with a code,
  and a transfer killed halfway resumes and verifies.

### 2. Native transfer screens and CLI

- One Send and one Receive in the app (files, photos, a folder), replacing
  the two sheets; progress, cancel, the history, received files in the
  Files app.
- `relay send <paths>` and `relay receive <code>` replace `send-code` and
  the spike's commands.
- **Phone check:** a folder each way between the iPhone and the laptop,
  one interrupted and resumed, and the peak memory read from the sheet.
  This closes the M1 gate.

### 3. Trusted devices

- The pairing policies in Settings (step 2), trusted devices in a list,
  forget and revoke, and the default grants (below). Codes keep working for
  anyone under every policy.
- Notes, files and AI asks to trusted devices over `relay/device/1` (step
  3), then the TCP path removed (step 4).
- The device key moves into the Keychain on iPhone, unchanged (fixes
  A17); computers keep the 0600 file.
- **Accepted when** a trusted device survives relaunches on both sides and
  sends without a code; a revoked one is refused and isn't re-paired by a
  code or the automatic policy.

## PRs that start M2

### 4. Device discovery and status

- Trusted devices show as nearby, reachable or offline (presence). On the
  same network Bonjour finds them; away from it they show as offline until
  PRs 5 and 6 give a way to locate them ([below](#four-separate-questions)).
- Status a device reports about itself: name, platform, last seen, what it
  offers (the first, read-only use of service discovery).
- The app's three screens begin: Nearby, Connections, Activity. A device
  opens its own view.

### 5. The connection resolver and private routes

- One module (`resolver.rs`) picks the route for every connection: local,
  then private (Tailscale addresses the devices exchanged while they were
  together), then iroh's direct internet route; the mode (Local, Hybrid,
  Private) limits it.
- Remote discovery, first form: each trusted device remembers the other's
  last known addresses (local and Tailscale) and tries them.
- Each connection reports its route, shown as "Direct, Wi-Fi", "Private
  network", "Direct, internet" or "Via relay".
- Diagnostics: why a device can't be reached, in words.
- **Accepted when** the phone on 5G reaches the laptop at home over
  Tailscale with no address typed. This is the Tailscale test, not yet the
  M2 test: Tailscale is itself configuration.

### 6. Our relay and internet routes

- Remote discovery, full form: each device publishes where it can be
  reached, signed by its key, so a trusted device finds it by key. Use
  iroh's own discovery (publishing a signed record, as iroh's DNS and
  pkarr discovery do) on our server, or a small rendezvous service only if
  that isn't enough.
- `iroh-relay` on Hetzner as the production relay. A test relay on
  Hetzner comes earlier (below), so this PR makes it permanent. It stays
  optional: local and Tailscale routes never need it, and Relay works with
  it down.
- Reconnecting when the network changes.
- **Accepted when** (the M2 test) the phone reliably finds and connects to
  the laptop and the iMac from outside home, without Tailscale, with
  nothing configured by hand.

## Identity and storage

- **Device identity** is the iroh key, lasting from PR 1 (a 0600 file),
  moved unchanged into the Keychain on iPhone in PR 3. A device has one key
  for all protocols.
- **Person identity** (a Nostr key signing each device's key) and its
  recovery are out of M1 and M2. They come when Relay needs people,
  groups and a portable identity.
- **Trust states** in the record: Paired, Trusted, Revoked.
- **Default grants for a trusted device.** Trust establishes identity, not
  permission:

  | Capability | Default for a trusted device |
  |---|---|
  | Receive files and notes from it | On, shown in its permissions, can be turned off |
  | AI requests (today's `ask`) | Off until granted |
  | Terminal, tasks, folders | Never implied; M6 grants only |
  | After revocation | Nothing, whatever was granted before |

  The full grant model comes with M6. The Lean model of trust and
  revocation is written with it, as the plan's verification table says.

### Four separate questions

| Question | Answered by |
|---|---|
| **Identity:** who is the device? | Its iroh key (PR 1) |
| **Discovery:** where might it be? | Bonjour nearby (PR 4); remembered local and Tailscale addresses (PR 5); a signed record published to our server (PR 6) |
| **Connectivity:** can we reach it? | The resolver's routes: local, private, direct internet, our relay (PRs 5 and 6) |
| **Authentication:** is it the device we expect? | The connection's endpoint id must match the trusted key; nothing else counts |

A key says who a device is, never where it is.

## Rules for this work

- Modules in `examples/relay/rust`, no new crates. The connection fabric,
  identity and capability boundaries are built as features need them.
- Relay takes nothing from Chat. Messages (M3) start only after the shared
  part of `chat-core` has moved into the framework, or Relay writes its own
  small outbox.
- Rust owns the state; Swift shows it and does Bonjour, the Keychain and the
  file picker.
- No 1 GB phone tests: a small file and one large-enough file (about 200
  MB) are the phone checks.
- Update the ledger when a capability changes (`docs/ledger/`).

## Decided (Reuben, 2026-11-16)

1. **Pairing is a setting with three policies** (step 2 of [one transfer
   path](#one-transfer-path)): codes only, ask to pair, pair
   automatically. Basic functions are never lost to the higher ones;
   automatic re-pairing is one policy.
2. **Hetzner runs a test relay early:** `iroh-relay` on Hetzner as soon as
   transfers run on the core's own endpoint (PR 1), so the phone can test
   from 5G long before PR 6. It is for testing; PR 6 makes it the
   production relay and adds reconnecting. Setting it up on the server is a
   step for Reuben to approve when it comes.
3. **Device capabilities come before messaging:** PRs 7 to 10 before PR
   11, so the personal network test passes first.

## Clarified after review (Reuben, 2026-11-16)

1. The device key lasts from PR 1; PR 3 only moves where it is stored.
2. Identity, discovery, connectivity and authentication are four separate
   questions ([above](#four-separate-questions)).
3. Trust never silently becomes permission: the default grants are listed
   [above](#identity-and-storage).
4. Person identity and its recovery wait until people and groups need them.
5. AI comes in three stages: today's requests moved to iroh (PR 3), AI as
   a capability (M6), distributed AI (M8).
6. Pairing modes are policies, not levels; a revoked device is never
   re-paired by itself; the Tailscale test (PR 5) is separate from the
   final M2 test (PR 6); the Hetzner relay stays optional.
