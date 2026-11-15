# Relay platform plan

Status: Reuben's product direction, 2026-11-14. **This is the canonical
Relay roadmap;** the documents below are its supporting specifications.
It builds on [relay-transfer-plan.md](relay-transfer-plan.md) (transfer,
messages, rooms, security, iPhone limits, decisions) and
[research/relay-rooms-protocols.md](research/relay-rooms-protocols.md)
(why rooms speak Nostr), which stay as they are; where this plan renumbers
their stages, the [milestones](#milestones) here win.

> **Relay is a platform for connecting devices, people and services, with
> file transfer as its first application.**
>
> If two things need to exchange information, Relay should provide a way
> for them to connect.

Not another messaging app, not another AirDrop clone, not another
self-hosted server: the layer that lets things communicate, whichever device
they are on, wherever they are and whatever network they happen to be on.
The name stays Relay.

## Where each part stands

| State | What |
|---|---|
| **Implemented** (on main) | Relay R1/R2 ([examples/relay](../examples/relay/README.md)): QR pairing, TCP + Noise from `forge-devproto`, notes and files between paired devices on one network, Bonjour on iPhone, the `relay` command. AI asks over Relay (protocol 2, `relay serve-ai` / `relay ask`). Chat with its `ChatRepo` backends, SQLite store, outbox and SwiftUI conversation screens. Swift for Relay has not yet been compiled or run on a phone |
| **Validated, experimental** (Linux spike, [results](research/relay-stage0-spike.md)) | iroh endpoints with relays off; 1 GB offline transfer with iroh-blobs; mDNS discovery by code number; SPAKE2 word codes with key confirmation over both endpoint ids; one wrong guess ends a code; blobs only for the endpoint that proved the code; size about +7 MB. M0 (#199): three EFF short words, no number collisions, busy and wrong-code handling, disk store and resume, the code session as a state machine checked by Lean, and transfers both ways between two Linux machines over Tailscale |
| **Approved, not built** | iroh for every connection; iroh-blobs for files; word codes `7-orbit-velvet` with a QR code; trusted devices replacing QR pairing; messages through a `PeerRepo` backend for `chat-core`; temporary rooms that collapse with the host; Relaxed, Standard and Strict room levels; rooms on Nostr (Marmot, NIP-29, NIP-11); our rendezvous and iroh relay on Hetzner; a browser receiver later; stays in ixOS |
| **Proposed** (this plan) | The four pillars and three screens; the connection resolver and three network modes; person and device identities; capability-based permissions; capability services and their declarations; device capabilities (clipboard, files, terminal, tasks, AI); the extension model; the eventual crate layout |
| **Deferred** | Our own federation protocol, our own group encryption, a full sync engine, unrestricted remote access, an extension marketplace, agents over Relay (below), any dependency on our servers for local use, a repository restructure, a separate repository, a new name |

## The four pillars

One platform, one connection layer, four groups of capabilities (not four
apps):

| Pillar | What |
|---|---|
| **A. Transfer** | Files and folders, photos and videos, text and clipboard, links and documents, large resumable transfers; later selective sync, temporary share links, several recipients, transfer collections (a project or folder as one object) |
| **B. Communicate** | Conversations with your devices, person to person messages, temporary rooms, persistent group rooms, encrypted groups, federated communities |
| **C. Connect and control** | Discovery, device status, shared clipboard, approved folders, remote terminal, task handoff, remote services |
| **D. Extend** | Services from other apps and developers, public relay discovery, self-hosted infrastructure, app to app communication, AI and automation |

### The main screens

- **Nearby:** devices and people that can be found right now.
- **Connections:** trusted devices, contacts, servers, rooms.
- **Activity:** transfers, messages, sessions and requests.

Pick a device, then choose what to do: send a file, start a conversation,
open a terminal, share a folder, ask for a task, start a room. No networking
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

The rest of Relay never cares which route was used. The person can restrict
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
- **Recovery** uses an existing scheme, not one of ours. Candidates to
  evaluate in M2: the person key on two or more devices, and NIP-06 (a
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
| Relay Device | Messages and commands between devices (the `PeerRepo` for Chat, clipboard, status) |
| Relay Session | Live sessions: temporary rooms, terminals |
| Nostr / Marmot | Persistent and community rooms (an adapter, not the transport for everything) |
| Relay Service | Capability services and extensions |

### Trust and permissions

| Level | Can |
|---|---|
| **Unknown** | Ask to connect; nothing without consent |
| **Paired** | What was granted, one capability at a time |
| **Trusted** | Lasting permission for the capabilities granted |

Permissions are per capability: trusting the laptop to receive files does
not let it run commands.

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

### B. Communications

Reuse Chat: `chat-core`'s `ChatRepo` with a `PeerRepo` over Relay Device,
no second chat engine. Kinds: conversations with your own devices, person to
person, temporary rooms, community rooms, application channels.

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
- **AI:** Relay is a transport for models on other machines (the AI asks
  that already exist, Forge AI's request shape); never required.

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

### Agents over Relay (deferred)

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
- **When:** after M2 (messaging and capability permissions), as part of the
  Extend pillar. **First test:** two independent agents exchange signed
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
estimates are for planning, not promises.

| Milestone | Contents | Was | Estimate |
|---|---|---|---|
| **M0 Foundation validation** (Linux to Linux) | Done on one Linux machine: iroh, SPAKE2 codes of three EFF words, no number collisions, busy and wrong-code handling with new codes, disk store, a 4 GB transfer resumed after the receiver was killed, 24 MB peak memory. Left: two real machines on a real network (the laptop, the iMac or Hetzner over Tailscale) and the Lean model of the code session | Stage 0 | 1 to 2 days |
| **M1 Relay Transfer** | Starts with the iPhone feasibility check moved from M0 (iOS build and size, Bonjour to the Rust endpoint, iPhone to Linux both ways, Wi-Fi speed, memory); then production core, iPhone and CLI send and receive, folders and several files, resumable disk store, trusted devices, history, cancellation and errors | Stage 1 (files) | 1 to 2 weeks |
| **M2 Personal device network** | Persistent identities (person and device), discovery and status, the resolver, Tailscale routes, direct internet, our rendezvous and relay, diagnostics | Stage 2 (servers) | 1 to 2 weeks |
| **M3 Direct communications** | `PeerRepo`, messages between devices, outbox, delivered and read, attachments, history, AI asks as messages | Stage 1 (messages) | 1 to 2 weeks |
| **M4 Temporary rooms** | Host and join, codes, members, permissions, memory only, closing, Relaxed / Standard / Strict | Stage 2 (rooms) | 1 to 2 weeks |
| **M5 Decentralised communities** | After the Nostr/Marmot spike: Nostr adapter, NIP-11, Marmot, MLS groups, NIP-29, our Nostr relay, relay choice, moderation | After stage 2 | 2 to 4 weeks |
| **M6 Device capabilities** | Clipboard, approved folders, terminal, tasks, AI inference, permissions, service discovery | New | 2 to 4 weeks |
| **M7 Platform** | Extension SDK, browser receiver, Android app, integrations, folder sync, multi-device sessions, public protocol spec | Stage 3 | Longer term |

### How the work lands

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
| Peer messaging | Chat's screens run on `PeerRepo` |
| Temporary rooms | Host and join lifecycle works |
| Nostr/Marmot spike | An encrypted group works with another client |
| Each capability | One service at a time |

## Verification

Following the rule in [foundation-review.md](foundation-review.md#where-lean-helps-and-where-it-does-not)
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
CI job once a proof exists. Each model names the Rust function it describes,
and a test checks the code against the model's transitions, so they don't
drift apart.

## Not yet, on purpose

| Temptation | Instead |
|---|---|
| Our own federation protocol | Nostr |
| Our own group encryption | Marmot / MLS, after validation |
| Our own P2P transport | iroh |
| Another chat app | Chat's engine and screens |
| A full sync engine | One-way transfers first |
| Unrestricted remote access | Capability permissions |
| An extension marketplace | The extension protocol first |
| Our own agent protocol | A2A and MCP carried over Relay |
| Needing our servers | Local first, always |

**Risks to watch:** the iOS lifecycle (reconnect and resume, never fight
Apple's background rules), the maturity of iroh-blobs and Marmot (pin
versions, test interoperability, keep an alternative), and complexity
(never six half-built systems; each milestone useful alone).

## Repository: a destination, not a move

Today everything is `examples/relay` (app, `rust/` core with the CLI,
`spike/`). Code moves into crates only when it has a second real user, as
ixOS's architecture rule says. A possible end state:

```text
crates/relay-core (identity, connections, sessions, permissions, protocol)
crates/relay-transfer, relay-device, relay-rooms, relay-services, relay-cli
services/rendezvous, services/iroh-relay, services/nostr
sdk/rust, sdk/protocol
docs/relay-*.md
```

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
