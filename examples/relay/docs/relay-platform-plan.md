# Relay platform plan

Status: Reuben's product direction, 2026-11-14, sharpened by his product
specification of 2026-11-16 (product identity, the connections, permissions
and capabilities model, sharper milestones, M8, the PR order and the
personal network test) and his direction the same evening: **messaging and
file transfer first; the interconnecting parts are features of the
network**. **This is the canonical Relay roadmap;** the
documents below are its supporting specifications, and there is no second
roadmap. The next work (PRs 1 to 7) is in [relay-m1-m2-brief.md](relay-m1-m2-brief.md).
It builds on [relay-transfer-plan.md](relay-transfer-plan.md) (transfer,
messages, rooms, security, iPhone limits, decisions) and
[relay-rooms-protocols.md](relay-rooms-protocols.md)
(why rooms speak Nostr), which stay as they are; where this plan renumbers
their stages, the [milestones](#milestones) here win.

> **Relay. Your devices. One network.**
>
> Private connectivity for the things you own and the people you trust.
>
> Relay is a private connectivity platform that lets devices, people and
> services discover one another, establish trusted connections and exchange
> capabilities securely. It starts by moving files between your devices,
> grows into a personal network, and eventually becomes a general way for
> connected things to communicate and work together.

**Messaging and file transfer first** (Reuben, 2026-11-16): expand the
basic idea, without going overboard. Relay is first a way to send messages
and files between your devices and the people you trust, on any network.
Reaching your devices from anywhere is what makes that work; device status,
folders, clipboard, tasks, terminal and AI are features of the same network
that come after, not a second product.

**Personal network next, universal platform eventually.** Today a phone
can't easily reach the computer at home, the laptop has files you need
while away, the home server's services aren't public, the machine that runs
AI models is somewhere else, and each fix brings another account, server or
app. Relay makes it one operation: choose something, connect, do something
with it.

Not another AirDrop clone, messaging app, remote desktop, VPN, cloud
storage service or AI agent framework: it can offer capabilities in all of
these areas without becoming any one of them. The name stays Relay.

> **Agreed, 2026-11-17: Chat merges into Relay.** Relay's messages run on
> chat-core instead of a new engine, Chat's server stays as a frozen
> connection kind, and the front door is Chats, Send, Devices. Where this
> plan still says otherwise, [relay-chat-merge.md](relay-chat-merge.md)
> wins.

## Where each part stands

| State | What |
|---|---|
| **Implemented** (on main) | Relay R1/R2 ([examples/relay](../README.md)): device records and QR pairing, TCP + Noise from `forge-devproto`, notes and files between paired devices on one network, transfer history and received files, Bonjour on iPhone, the `relay` command. AI asks between paired devices (protocol 2, `relay serve-ai` / `relay ask`). Word codes on iroh in the app's own crate: "Receive with a Code" and "Send with a Code" on the iPhone and `relay send-code` on Linux. Relay's Swift compiles with xtool and runs in Device Lab, checked on a physical iPhone on 2026-11-15 |
| **Validated, not yet a production system** (Linux spike and the iPhone gate, [results](relay-stage0-spike.md)) | iroh endpoints with relays off; 1 GB offline transfer with iroh-blobs; mDNS discovery by code number; SPAKE2 word codes with key confirmation over both endpoint ids; one wrong guess ends a code; blobs only for the endpoint that proved the code; size about +7 MB. M0 (#199): three EFF short words, no number collisions, busy and wrong-code handling, disk store and resume, the code session as a state machine checked by Lean, and transfers both ways between two Linux machines over Tailscale. iPhone: 1 GB Linux to iPhone at 9.5 MB/s and iPhone to Linux, each with a word code |
| **Approved, not built** | iroh for every connection; iroh-blobs for files; word codes `7-orbit-velvet` with a QR code; trusted devices replacing QR pairing; device messages on chat-core, Chat merged into Relay ([below](#b-communications-first-with-transfer)); temporary rooms that collapse with the host; Relaxed, Standard and Strict room levels; rooms on Nostr (Marmot, NIP-29, NIP-11); our rendezvous and iroh relay on Hetzner; a browser receiver later; stays in ixOS |
| **Proposed** (2026-11-14) | The four pillars and three screens; the connection resolver and three network modes; person and device identities; capability-based permissions; capability services and their declarations; device capabilities (clipboard, files, terminal, tasks, AI); the extension model; the eventual crate layout |
| **New proposals** (2026-11-16) | Connections, permissions and capabilities as the product model; Revoked as a trust level and "trust is not a permission"; scoped grants with three presets; service discovery separate from device discovery; device presence and connection diagnostics; M8 Distributed services; the [new capabilities](#new-capabilities-worth-considering) list; the [personal network test](#the-personal-network-test) as the measure of the product |
| **Deferred** | Our own federation protocol, our own group encryption, a full sync engine, unrestricted remote access, an extension marketplace, agents over Relay (now M8), any dependency on our servers for local use, a repository restructure or early crate split, a separate repository, a new name, a separate Relay server product, several regional relays before reliability needs them |

### What the code holds today (2026-11-16)

So that plans are never counted as features:

1. **M1 is not finished because the iPhone gate passed.** Production
   transfer still needs its whole lifecycle: disk storage that survives a
   relaunch, folders and several files, trusted devices, history,
   cancellation and reliable resume, on the phone as well as on Linux.
2. **There are two transfer paths.** Paired devices use TCP + Noise
   (`link.rs`, `wire.rs`, `forge-devproto`); word codes use iroh and
   iroh-blobs (`code.rs`, `code_send.rs`). They share no transport, and
   word-code transfers don't appear in the history. Relay keeps one
   networking stack in the end: the migration is in the
   [brief](relay-m1-m2-brief.md#one-transfer-path).
3. **Messaging is Chat's, not yet between devices.** Chat merged into
   Relay on 2026-11-17 ([relay-chat-merge.md](relay-chat-merge.md), step
   1): the Chats tab talks to a chat server; messages between trusted
   devices on chat-core come in step 3.
4. **Pair secrets sit in Relay's JSON file,** not the Keychain (friction
   A17).

## The product model

Three ideas carry the product, and every later feature fits into them:

| Idea | What |
|---|---|
| **Connections** | Devices, people and services that have a relationship with one another, each with an identity |
| **Permissions** | Authority someone explicitly granted to use one capability on one resource |
| **Capabilities** | What a connected device or service can actually do |

A device is not simply connected or disconnected. It is connected, has an
identity, and offers some capabilities under a permission policy:

```text
DEVICE      Ubuntu Laptop
IDENTITY    Trusted
AVAILABLE   Transfer files, browse approved folders, device status,
            ask the local AI model, run approved tasks
PERMISSIONS File transfer: allowed   Folders: approved only
            AI inference: allowed    Terminal: ask every time
            Arbitrary commands: denied
```

This is what lets Relay grow without becoming dangerous.

## The four pillars

One platform, one connection layer, four groups of capabilities (not four
apps):

| Pillar | What |
|---|---|
| **A. Transfer** (first mature capability) | Files of any practical size, collections (several files and folders as one transfer), clipboard text and links, photos and videos, documents with metadata and previews, resume, searchable history; later several recipients, short-lived share codes, selective sync. Sending a folder is not syncing one: transfer first, sync is a much larger consistency problem |
| **B. Communicate** | Conversations with your devices, person to person messages, temporary rooms, persistent group rooms, encrypted groups, federated communities |
| **C. Connect and control** | Device status (online, battery, resources, uptime), approved folders, clipboard, terminal, named commands, long-running tasks, hosted services, device notifications, active sessions. The phone becomes the remote control for your own devices: from outside home, start a build on the laptop, follow it, and download the result, with no SSH or server exposed to the internet |
| **D. Extend** | Services from other developers (a media server's playback controls, document processing, local AI inference, build and test on a development machine, approved directories, home automation). Relay needs a service's identity, protocol, permissions and invocation lifecycle, never its implementation |

### The main screens

Decided 2026-11-17 with the Chat merge
([relay-chat-merge.md](relay-chat-merge.md#what-the-merged-app-is)), replacing
Nearby, Connections and Activity:

- **Chats:** every conversation, with a device, a person, a room, a chat
  server or the assistant.
- **Send:** one flow, what then who; Receive beside it. Transfers and
  requests waiting for approval show here.
- **Devices:** nearby and trusted devices, contacts, servers.

Pick a device, and it opens its own capability view: send a file, start a
conversation, open a terminal, share a folder, ask for a task, start a
room. New features land in that view, never as another top-level screen.
Activity also holds the requests waiting for approval and finished and
failed operations. Today's app still presents Relay as a paired-device
transfer tool; it moves to these screens with M2. No networking
words in the interface; the route shows as "Direct, Wi-Fi", "Private
network" or "Via relay".

## Networking

### The connection resolver

One Rust module picks the route for every connection, in this order:

1. **Direct, local:** Bonjour (Swift on iPhone, mDNS in the CLI) and direct
   iroh connections. No internet needed.
2. **Private networks:** Tailscale addresses, or other routes the person
   configured.
3. **Direct over the internet:** iroh hole punching.
4. **Our relays:** rendezvous and iroh relay on Hetzner, Oracle as a
   second region later.
5. **Public relays:** compatible public iroh relays, optional fallback.

The resolver is a distinct Rust responsibility: no capability service has
its own networking strategy. A terminal service receives an authenticated
session with an identity and its permissions, and never learns whether it
runs over Wi-Fi or an iroh relay. The person can restrict
which routes a connection may use.

| Mode | Behaviour |
|---|---|
| **Local** | Local discovery only; works with no internet |
| **Hybrid** (default) | Local, private and public routes, chosen automatically |
| **Private** | Only explicitly trusted networks and our own infrastructure |

**Rule:** local transfers never need our servers. Two devices on one Wi-Fi
transfer files even if every server is down.

Sketch of the interface (iroh already does most of 1, 3, 4 and 5; the
module adds discovery, private routes and policy):

```rust
pub trait ConnectionProvider {
    async fn discover(&self) -> Result<Vec<Peer>>;
    async fn connect(&self, peer: &PeerId) -> Result<Connection>;
    async fn status(&self) -> ConnectionStatus;
}
```

A connection reports its route, quality, encryption, the peer's identity,
reconnection state, and the protocols and capabilities the peer offers.

## Architecture

| Layer | Contents |
|---|---|
| 6. Applications | iPhone and iPad (SwiftUI), macOS, the `relay` command (Linux, macOS), Android when the core is proven, a browser receiver, other apps |
| 5. Capability services | Transfer, chat, rooms, clipboard, terminal, sync, AI, extensions |
| 4. Session and identity | Device identities, people, pairing, trust, permissions, sessions |
| 3. Protocol adapters | Relay Transfer, Relay Device, Relay Session, Nostr/Marmot, Relay Service |
| 2. Connection fabric | iroh (QUIC, NAT traversal, relays), Bonjour, private routes, the resolver |
| 1. Storage and cryptography | Keychain, SQLite, BLAKE3, SPAKE2, MLS, blob storage |

Cryptography stays in the Rust core. The app keeps using ixOS as it is
(forge-runtime, native services, ForgeUI, `chat-core`), with no separate
app infrastructure.

### Identity: people and devices

- **A device** has its own iroh key (transport identity), kept in the
  Keychain on iPhone (friction A17 has to be fixed first) and a 0600 file
  on computers.
- **A person** has a Nostr key (the identity rooms already need). It signs
  a statement for each of their devices' keys; the statement is revocable,
  so a lost device is cut off without touching the others.
- **Device identity comes first.** M1 and M2 build durable device keys
  and device trust only; the person key, and how it is recovered, wait
  until Relay needs people, groups and a portable identity (M3 to M5).
- **Recovery** uses an existing scheme, not one of ours, decided when the
  person key is built. Candidates: the person key on two or more devices, and NIP-06 (a
  Nostr key from a standard word phrase) for a written backup. MLS already
  treats each device as its own member, so a lost device is removed from
  groups by the normal key rotation.

### Protocols

Each protocol has its own iroh ALPN with a version
(`relay/transfer/1`, `relay/device/1`, …), so they evolve separately and a
peer says which versions it speaks when it connects. Inside the request and
response protocols (Device, Session, Service), messages share one envelope:

```rust
pub struct RelayEnvelope {
    pub protocol: String,
    pub version: u16,
    pub request_id: String,
    pub sender: PeerId,
    pub payload: Vec<u8>,
}
```

| Protocol | For |
|---|---|
| Relay Transfer | Files and folders (iroh-blobs, codes) |
| Relay Device | Messages and commands between devices (messages, clipboard, status) |
| Relay Session | Live sessions: temporary rooms, terminals |
| Nostr / Marmot | Persistent and community rooms (an adapter, not the transport for everything) |
| Relay Service | Capability services and extensions |

### Trust and permissions

| Level | Can |
|---|---|
| **Unknown** | Ask to connect; nothing without consent |
| **Paired** | Completed pairing; only what was granted, one capability at a time |
| **Trusted** | Lasting grants for specific capabilities |
| **Revoked** | No grants and no trusted connections; like an unknown device, only one-off code transfers the person accepts |

**Trust is not a permission.** A trusted device can still be refused the
terminal. A grant names the device or service, the capability, the resource
or target, the allowed operations, how long it lasts, whether each use needs
confirmation, and its resource limits:

```text
GRANT  device: Ubuntu Laptop   capability: terminal
       scope: named tasks only  operations: build, test, status
       duration: 24 hours       confirmation: required   revocable: yes
```

**Default grants for a trusted device** (2026-11-16): trusting a device
establishes its identity and nothing more by itself. Receiving files and
notes from it is on by default, shown in its permissions and can be turned
off. AI inference needs an explicit grant. Terminal and tasks are never
implied by trust. Revoking a device overrides every grant it had, and a
revoked device never becomes trusted again by itself, whatever code or old
pairing it presents; only the person can trust it again. Revocation ends
the trusted relationship, not the basic function: a revoked device can
still join a one-off word-code transfer the person starts or accepts, as
any unknown device can, and that transfer never restores trust
([the rule](relay-m1-m2-brief.md#one-transfer-path)).

Three presets cover most grants: **Always allow** (harmless, repeatable),
**Ask first** (sensitive or disruptive) and **Never allow**. Build the model
first and show it through a simple interface; no detailed permission
editor at the start.

### Capability services

Every feature is a service that declares its protocol id and version,
required permissions, operations, resource limits, whether it works offline
and whether it needs a host that stays up. A device advertises what it
offers:

```text
DEVICE ubuntu-laptop   owner: Reuben   trust: Trusted
  relay.transfer.v1  relay.chat.v1  relay.clipboard.v1
  relay.terminal.v1  relay.filesystem.v1  relay.ai.inference.v1
POLICIES
  files: approved folders only   terminal: approval required
  AI inference: available        clipboard: manual approval
```

These identifiers are Relay conventions, not existing standards. Proposed
set: transfer, chat, clipboard, filesystem, terminal, inference, compute,
sync, stream, automation.

## The product systems

### A. Universal transfer (first)

The croc-style flow from the transfer plan, grown into a full service.
**MVP:** files and several files, folders, codes, QR, progress, cancel,
resume, hash verification, trusted devices, history. **Later:** clipboard,
photo and video, browser receiving, selective folder sync, temporary share
links, several recipients, transfer collections.

### B. Communications (first, with transfer)

From messages between your own devices to direct messages, attachments
(sent through Transfer), delivered and read states, temporary and
persistent rooms, communities on independent servers, and perhaps voice and
video later.

**Chat merges into Relay** (Reuben, 2026-11-17, replacing "Relay does
not depend on Chat" of 2026-11-16). Chat's engine, chat-core, becomes
Relay's: messages between trusted devices are a `ChatRepo` over Relay
Device, chat servers keep theirs, and rooms and communities get their own
later. Being one app, nothing moves into the framework for it. Plan, data
and order: [relay-chat-merge.md](relay-chat-merge.md).

### C. Rooms and communities

Nostr relays for events, NIP-11 for what a server supports, NIP-29 for
hosted groups, Marmot (MLS) for end to end groups, after the spike.

| Room | Hosted by | Encryption and history | For |
|---|---|---|---|
| **Temporary** (default private room) | A person's device | Code to join; memory only; closes when the host ends it (or looks away, by level) | Quick conversations and exchanges |
| **Persistent private** | A computer or server, acting as a relay | MLS (Marmot); members leave and come back | Ongoing groups and projects |
| **Hosted community** | A server | What the server declares (retention, who can read), shown before joining | Larger communities, moderation, public rooms |

Closing a temporary room deletes Relay's own copies; it cannot stop anyone
copying or photographing what they saw. Strict is a protective client
behaviour, not remote deletion, and screen capture detection is not a
security boundary. Relay shows a server's real retention and capabilities,
never implies that every room vanishes.

### D. Device connectivity and automation

After the foundations work. On the iPhone, "My network" lists your devices
with their route (laptop: Direct, Wi-Fi; iMac: Private network; Hetzner:
via Tailscale) and actions: browse files, open a terminal, clipboard, send a
task. For example: start a build on the laptop from the phone, send a file
from the iMac to Hetzner, have a local model process a document and get the
result back.

- **Terminal:** borrow before building. Run OpenSSH through an iroh tunnel
  (as `iroh-ssh` does) rather than writing a terminal protocol, with
  per-device authorisation and visible approval.
- **Tasks:** named, approved commands, not an open shell, with approval for
  sensitive ones.
- **AI,** in three stages of one idea, never three implementations:
  1. **Remote model requests** (implemented: `relay serve-ai` / `relay
     ask`, Forge AI's request shape) move from TCP onto iroh in M1, as
     they are.
  2. **AI as a device capability** (M6): discover the models a device
     offers, see what they can do, and invoke them under a grant.
  3. **Distributed AI** (M8): several machines, resource-aware routing.

  Relay is the transport and the permission check; a model is never
  required.

## Extending Relay

- **Built-in services:** Rust modules compiled into Relay.
- **External services:** separate processes speaking the documented,
  versioned Relay Service protocol over a local socket, with restricted
  permissions. No third-party code is loaded inside the iPhone app.

```bash
relay plugin install my-service
relay service start my-service
relay connect ubuntu-laptop
relay service invoke my-service process-document
```

The extension protocol comes before any marketplace.

### Agents over Relay (M8)

Reuben's idea (2026-11-15): AI agents on different machines talk to each
other over Relay, whatever model, framework or host each one uses. Relay
does not need to understand AI; it gives agents what it gives devices:
encrypted direct connections with no public endpoint, identity,
permissions, discovery and delivery.

- **Reuse the existing agent protocols.** Carry
  [A2A](https://a2a-protocol.org) messages (agent cards for discovery,
  tasks, results, files) over Relay, and MCP for tools, rather than
  inventing a `relay.agent` format. Relay adds the transport, identity and
  revocable permissions those protocols leave to the deployment.
- **Messages, tasks and capabilities stay separate.** A message never
  grants permission to act on its contents. Running a task needs a
  capability the person granted to that agent's key, with rate limits, and
  can be revoked.
- **Each agent has its own key,** like a device, signed by the person who
  runs it.
- **Starting point:** `relay serve-ai` / `relay ask` (protocol 2) already
  send OpenAI-style requests to a model on another machine; they move onto
  iroh with the rest of Relay.
- **When:** M8, after the extension protocol (M7). **First test:** two independent agents exchange signed
  messages, one asks the other for a task, gets the result and an
  attachment back, and a revoked grant is refused.

## Security

| Threat | Protection |
|---|---|
| A stranger finds a device | Nothing without authentication |
| Someone guesses a code | PAKE, one guess, expiry |
| A malicious relay operator | End to end encryption |
| A lost device | Revoke its identity |
| A compromised paired device | Per-capability permissions |
| A malicious extension | Restricted execution, separate process |
| A removed room member | MLS key rotation |
| A malicious file | Received into Relay's inbox, opened only on request |

Three properties are never conflated: **transport encryption** (the
connection), **end to end encryption** (content hidden from intermediaries)
and **storage protection** (data at rest). An iroh connection to a server
protects the traffic; it doesn't stop that server reading what it decrypts.

Also required: signed and revocable device associations, key backup and
recovery, replay protection, protocol version negotiation, rate and resource
limits, logs without content, and a security review before anyone but
Reuben uses it.

## Milestones

Each leaves something that works on its own. M0 and M1 are gates. The
estimates are for planning, not promises. The numbers stay; 2026-11-16
sharpened their contents, added an acceptance test to each and added M8.
They are names, not the order: M3 comes before M2.

| Phase | Milestones |
|---|---|
| I. Messaging and transfer, everywhere (now) | M1 transfer, M3 messages, M2 reach |
| II. People together | M4 temporary rooms; M5 communities after its spike |
| III. Features of the network | M6, one feature at a time |
| IV. Extensible (later, not planned in detail) | M7, M8 |

| Milestone | Contents | Was | Estimate |
|---|---|---|---|
| **M0 Foundation validation** (Linux to Linux) | Done: iroh, SPAKE2 codes of three EFF words, no number collisions, busy and wrong-code handling with new codes, disk store, a 4 GB transfer resumed after the receiver was killed, 24 MB peak memory, two Linux machines over Tailscale, and the Lean model of the code session (`verification/relay/`) | Stage 0 | Done |
| **M1 Relay Transfer** | Starts with the iPhone feasibility check moved from M0. Passed on 2026-11-15: the iOS build, Bonjour to the Rust endpoint, Linux to iPhone (200 MB at 15.9 MB/s, 1 GB at 9.5 MB/s over Wi-Fi) and iPhone to Linux with a word code ([results](relay-stage0-spike.md#iphone-start-of-m1-2026-11-15)). Left: iroh as the production transport (one transfer path), send and receive on iPhone and CLI, several files and folders, resumable disk store, progress, cancellation and errors, trusted-device transfer, history, and a memory reading from the phone. **Accepted when** two real devices send and receive large files both ways, also after an interruption, with verified content | Stage 1 (files) | 1 to 2 weeks |
| **M2 Personal device network** (reach: messages and files from anywhere) | Persistent device identity in secure storage, trusted-device management, discovery and status, the resolver, local, private (Tailscale) and internet routes, our rendezvous and relay, diagnostics and reconnecting. **Accepted when** the phone reliably finds and connects to your own computers from outside the home network with no addresses configured | Stage 2 (servers) | 1 to 2 weeks |
| **M3 Direct communications** | Device-to-device messages, a reliable outbox, delivered and read, attachments, history, AI asks as messages, reconnects without duplicates. **Accepted when** messages survive relaunches, disconnections and retries without disappearing or duplicating | Stage 1 (messages) | 1 to 2 weeks |
| **M4 Temporary rooms** | Host and join with codes, room identity, members, permission levels, memory-only history, lifecycle, Relaxed / Standard / Strict. **Accepted when** two or more people use a private room, and it behaves correctly when the host disconnects | Stage 2 (rooms) | 1 to 2 weeks |
| **M5 Decentralised communities** | After the Nostr/Marmot spike (a real gate): Nostr adapter, NIP-11, Marmot, MLS groups, NIP-29, self-hosted compatible relays, relay choice, moderation. **Accepted when** a Relay user joins a supported group alongside an independent Nostr client, with the server's real capabilities and retention shown | After stage 2 | 2 to 4 weeks |
| **M6 Device capabilities** | One at a time: device status (CPU, memory, disk, uptime, battery), approved folders, clipboard text, then terminal (OpenSSH over a Relay connection) and named tasks, which need much more security work, and AI inference; permissions, service discovery. **Accepted when** the phone inspects a remote computer, browses an approved folder, runs an authorised task and gets its result | New | 2 to 4 weeks |
| **M7 Platform** | The documented capability protocol, external service processes, service discovery and invocation, permission declarations, version negotiation, browser receiver, macOS and Linux polish, Android, selective folder sync, public protocol spec. The extension protocol comes before any marketplace | Stage 3 | Longer term |
| **M8 Distributed services** (new proposal, 2026-11-16) | Several devices hosting different services, availability, resource-aware routing, long-running task monitoring, handoff, remote compute, distributed AI, agents over A2A and MCP ([above](#agents-over-relay-m8)). Example: the phone asks Relay to process a document; Relay finds an authorised computer, sends the file, follows the task and returns the result, without deciding how it is processed | New | Long term |

### How the work lands

The order of the next PRs (2026-11-16). Tests and device evidence go in
each feature PR; there are no test-only PRs.

| # | PR scope | Outcome | Milestone |
|---|---|---|---|
| 1 | Finish the M1 transfer core | Production iroh transfers | M1 |
| 2 | Finish the native transfer UI and CLI | Usable on phone and computer | M1 |
| 3 | Trusted device identity | Persistent device trust | M1, M2 |
| 4 | Messages between trusted devices | Messaging | M3 |
| 5 | Device discovery and status | Know who is reachable | M2 |
| 6 | Connection resolver and private routes | Beyond one Wi-Fi | M2 |
| 7 | Our relay and internet routes | Messages and files anywhere | M2 |
| 8 | Temporary rooms | Private group conversations | M4 |
| 9 | Capability permission model | Safe network features | M6 |
| 10 | Device status and approved folders | First network features | M6 |
| 11 | Clipboard and task execution | Useful device control | M6 |
| 12 | Terminal and AI inference | Remote computing | M6 |
| 13 | Nostr/Marmot integration | Communities | M5 |
| 14 | External service protocol | Extensibility | M7 |
| 15 | Browser receiver and wider clients | Platform | M7 |
| 16 | Distributed services and agents | Long-term ambition | M8 |

Messaging and transfer come first (Reuben, 2026-11-16, replacing the
earlier "capabilities before messaging"): messages (4) follow trusted
devices, then reach (5 to 7) carries both everywhere, and the network
features (9 to 12) come after rooms. A test relay on Hetzner goes up early,
after PR 1, and PR 7 makes it permanent. Pairing is a setting with three policies
(codes only, ask to pair, pair automatically); the basic functions stay under the higher
ones ([brief](relay-m1-m2-brief.md#one-transfer-path)).

PR #197 stays the decision record and the spike. Implementation comes in
focused PRs, each with one acceptance condition:

| Scope | Accepted when |
|---|---|
| Linux to Linux (M0) | Two Linux machines move a file both ways with a code, and a dropped transfer resumes byte for byte |
| iOS feasibility (start of M1) | Send on Ubuntu, enter or scan the code on the iPhone, accept, and the file matches byte for byte |
| Production transfer core | A transfer resumes after an interruption and verifies |
| Native Relay UI | Send and receive work on the phone |
| CLI | Linux and macOS work with the phone |
| Trusted devices | Pairing persists securely |
| Device network | Internet and private routes work |
| Peer messaging | Messages between trusted devices survive relaunches and retries, once each, through chat-core's outbox |
| Temporary rooms | Host and join lifecycle works |
| Nostr/Marmot spike | An encrypted group works with another client |
| Each capability | One service at a time |

## Verification

Following the rule in [foundation-review.md](../../../docs/foundation-review.md#where-lean-helps-and-where-it-does-not)
(Reuben, 2026-11-14: Relay is a place where Lean verification is useful and
needed): Lean for small protocols whose properties must hold for every
interleaving, tests for the rest.

| What | Tool | Property |
|---|---|---|
| **The code session** (first, once M0's code is settled) | Lean | Whatever order connections, timeouts, collisions and new codes arrive in: a code allows at most one guess; "busy" and "gone" never spend a code; at most one receiver is ever allowed per code; an expired or spent code never comes back; a receiver never runs the handshake with two senders for one number |
| **Trust and permissions** (M2, M6) | Lean | No capability is used without a grant; a revoked device never regains one; revocation reaches every capability |
| **Message delivery** (M3) | Lean | With retries, reconnects and relaunches, each message is stored once and its final state reported once (the property Chat's outbox relies on, foundation review N1) |
| **Composition of the cryptography** (code proof bound to both device keys; later MLS rooms over relays) | Tamarin or ProVerif | Secrecy and authentication of the session key against an active network attacker. A symbolic protocol prover fits this better than Lean |
| The cryptography itself (SPAKE2, QUIC, MLS) | Proven libraries | Not ours to prove |
| Concurrency in Rust code | `loom` | Exhaustive interleavings of the real code |

Models live in `verification/relay/`, outside the Cargo workspace, with a
CI job once a proof exists. The current order (2026-11-28) is in
[unsafe-and-verification.md](../../../docs/research/unsafe-and-verification.md#the-plan-cheapest-and-most-useful-first):
property tests of trust and the mailbox rules first, then Lean for revocation
across devices and for delivery. Each model names the Rust function it describes,
and a test checks the code against the model's transitions, so they don't
drift apart.

## Not yet, on purpose

| Temptation | Instead |
|---|---|
| Our own federation protocol | Nostr |
| Our own group encryption | Marmot / MLS, after validation |
| Our own P2P transport | iroh |
| Another chat engine | chat-core, Chat's engine, now Relay's own |
| A full sync engine | One-way transfers first |
| Unrestricted remote access | Capability permissions |
| An extension marketplace | The extension protocol first |
| Our own agent protocol | A2A and MCP carried over Relay |
| Needing our servers | Local first, always |
| An elaborate agent orchestration framework | Capability invocation; A2A and MCP in M8 |
| Several regional relays early | One relay on Hetzner until reliability needs more |
| A separate Relay server product, repository or name | `examples/relay` in ixOS |

**Risks to watch:** the iOS lifecycle (reconnect and resume, never fight
Apple's background rules), the maturity of iroh-blobs and Marmot (pin
versions, test interoperability, keep an alternative), and complexity
(never six half-built systems; each milestone useful alone).

## Decisions locked in (2026-11-16)

| Area | Decision |
|---|---|
| Core, CLI | Rust |
| Mobile interface | Native SwiftUI |
| Transport | iroh |
| File transfer | iroh-blobs |
| Local discovery | Bonjour (iPhone), mDNS (CLI) |
| Private routes | Configured routes, Tailscale first |
| Internet routes | iroh hole punching and relays |
| Device identity | iroh key |
| Person identity | Nostr key, with its recovery decided when people and groups need it (after M2) |
| Group messaging | Marmot / MLS, after the spike |
| Remote terminal | OpenSSH over a Relay connection |
| Service protocol | Versioned capability declarations and invocation |
| External extensions | Separate processes with restricted permissions |
| Verification | Tests, plus Lean for the critical state machines |

## New capabilities worth considering

Proposals from 2026-11-16, not yet scheduled beyond their milestone.
Finding a laptop is half the job: Relay should also know it offers a
terminal, a file service and a model, so **service discovery is separate
from device discovery**.

| Idea | Why | Priority |
|---|---|---|
| Device presence | Know which devices are available and reachable | High (M2) |
| Service discovery | Find what a device can actually do | High (M6) |
| Task handoff | Send work to another machine | High (M6) |
| Connection diagnostics | Explain why a connection fails | High (M2) |
| Several recipients | Send to several trusted devices | Medium |
| Browser receiver | Receive without installing Relay | Medium (M7) |
| Remote media access | Use a service hosted on another device | Medium |
| Shared workspaces | Files and messages in a temporary context | Medium |
| Device-to-device automation | Trigger approved actions remotely | Medium |
| Distributed AI | Models across several machines | Later (M8) |
| Agent communication | Connect independent agents securely | Later (M8) |
| Public service discovery | Find third-party Relay services | Later |

## Repository: a destination, not a move

Today everything is `examples/relay` (app, `rust/` core with the CLI,
`spike/`). Code moves into crates only when it has a second real user, as
ixOS's architecture rule says. The core is still compact: build the
connection fabric, identity and capability boundaries as modules when
features need them, not as six or seven crates up front. A possible end state:

```text
crates/relay-core (identity, connections, sessions, permissions, protocol)
crates/relay-transfer, relay-device, relay-rooms, relay-services, relay-cli
services/rendezvous, services/iroh-relay, services/nostr
sdk/rust, sdk/protocol
docs/relay-*.md
```

## The messaging and transfer test

The measure for phase I (PRs 1 to 7):

1. The phone and the laptop trust each other once.
2. Send a message and a folder each way on home Wi-Fi.
3. Do it again with the phone on 5G, with nothing configured by hand.
4. Send a message while the laptop is off; it arrives when the laptop is
   back, once, and both sides show it delivered and read.
5. Relaunch both apps: the conversation and the transfers are still there.

## The personal network test

The longer-term scenario, once the network features (phase III) exist,
judges whether Relay has become what it is meant to be:

1. Open Relay on the phone: the laptop, the iMac and the remote server are
   in Connections.
2. Pick the laptop: Relay connects over the best route available.
3. Browse a folder: only the folders you approved are visible.
4. Start a named build task on the laptop.
5. Follow its progress, with a way to cancel it.
6. Get the output and the files it made back on the phone.

**Passes when** there is no public SSH port, no network set up by hand, no
separate file-transfer tool and no unrestricted remote access. It exercises
identity, routing, trust, permissions, capability discovery, tasks and
transfer together, which a list of separate features can't.

## Definition of success

1. Transfer a 10 GB file between iPhone and Linux.
2. Transfer files with no internet.
3. Transfer between devices on completely different networks.
4. Resume an interrupted transfer without corruption.
5. Send to an unpaired device with a short code.
6. Connect all of a person's devices through one identity and trust system.
7. Message another device without a central chat server.
8. A temporary room that works on iPhone and desktop.
9. Join a persistent encrypted community through an external Nostr relay.
10. Revoke a compromised device without affecting the others.
11. Offer a remote AI service through a permission-controlled capability.
12. Let other developers write compatible Relay services.

The order: make it useful (transfer, trusted devices), make it connected
(device network, direct messages), make it social (rooms, communities),
make it programmable (services, automation, AI, extensions).
