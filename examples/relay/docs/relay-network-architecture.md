# Relay network architecture

Status: Reuben's direction, 2026-11-21. This is the centre that the other
Relay idea docs refer back to. The Relay 1.0 foundations come first and
stay small ([audit, step 5](relay-audit-2026-11.md)), and nothing past
them is built until Reuben says so. Everything past "What exists today" is
a hypothesis in the sense of the
[open-ended design principles](relay-open-ended-design-principles.md):
each one says what already does it and what would prove it.

## What Relay is

> Relay is a secure communication layer connecting your devices and the
> people you trust. It provides identity, discovery, encrypted
> communication, permissions and reliable exchange. Applications,
> services and other participants build on top of that layer.

Its promise, in three parts:

- **Connect securely.** Find devices and people, make authenticated
  connections, and handle connectivity without the person seeing it.
- **Exchange reliably.** Messages, files and later other structured
  information, even when the other side isn't there yet.
- **Stay in control.** Explicit trust, permissions, revocation, privacy
  and ownership.

The goal is not to build a personal computing network or a social
platform. It is to make Relay exceptionally good at securely connecting
things, and let the larger ideas grow out of that. The architecture
stays deliberately smaller than the list of things it might support.

**Setup is an acceptance criterion for all Relay work,** not one
stage's feature: a person sees devices, people and conversations, and
Relay deals with the networking underneath. Work that is
cryptographically right but needs the person to understand networking
hasn't met the promise.

**The test for every idea:** does it need a new underlying capability,
or can it be built from what Relay already provides? If it can, build it
on that. If not, find the smallest missing primitive and prove it on its
own first.

**The design constraint:** no feature gets its own transport, identity
system, permission system or delivery queue when Relay already has one.
The exception is an existing standard that does the job better and
interoperates, such as MLS or Nostr.

## The layer: six things

1. **Identity.** Devices have cryptographic identities. People can own
   several devices. Trust and revocation are explicit.
2. **Connectivity.** Devices find and reach each other without anyone
   handling addresses, ports or routes.
3. **Secure communication.** Encrypted connections, authenticated peers,
   group encryption, explicit permissions.
4. **Reliable exchange.** Messages, files and attachments, later
   structured requests: durable queues, retries after interruption,
   stable ids, deduplication by the receiver, order where the protocol
   needs it, and explicit completion and failure.
5. **Persistence.** Conversations, identities, pending deliveries and
   task state survive restarts and network interruptions.
6. **Authorisation.** Trust admits; it grants nothing else. Every device,
   person, service and agent gets only the rights it was given.

Everything else is a use of these:

| Idea | What it really is |
|---|---|
| Relay Social | Communication between people and communities |
| Rooms | Secure group communication |
| Sites | A service reachable through Relay |
| Relay Computing | Structured requests over Relay |
| Relay Workshop | An interface for requesting and coordinating work |
| Clipboard and sync | Reliable exchange of data |
| AI agents | Participants with restricted permissions |
| Simulated chatrooms | An experiment with participants and conversations |

## What exists today

Written from the code on main, where all of the 1.0 foundations are
merged, so this part doesn't go stale. The PR numbers say which PR added
each piece. Phone proof of the whole set is the 1.0 acceptance run, not
yet done.

| Part of the layer | Today | Where |
|---|---|---|
| Identity | One iroh endpoint per device; its key is the device's only identity (#353 retires the old random id) | `node.rs`; key in the Keychain on iPhone (#348) |
| Pairing | Word codes (SPAKE2), typed or scanned as a QR code | `code.rs`, `CodeQR.swift` |
| Trust and revocation | A trusted-device list; revoked is refused for good until the person trusts again | `trust.rs` |
| Authorisation | Separate rights per device, never implied by trust: "may send here" (`receives_from`), "may ask" (`may_ask`, #344) | `trust.rs` |
| My Devices, step 1 | An introducer (#354): a device you mark "Introduces Devices" offers you the devices it trusts and revokes here what it revoked, saying so; it never trusts for you. Its own right, "Can See My Devices" (`sees_devices`), decides who may read the list ([design](relay-my-devices.md)) | `trust.rs`, `device.rs` |
| Secure communication | iroh's encrypted, authenticated connections; one protocol (ALPN) per job: word codes, device messages, files | `node.rs`, `device.rs` |
| Reliable exchange | One durable outbox for messages and files (#340): retried after an interruption, with stable ids the receiver uses to drop duplicates, in order per device, ending Done or Failed; files resumable and verified (iroh-blobs). Not exactly-once under every failure: duplicates are dropped by id, not prevented | `outbox.rs` |
| Persistence | One SQLite database, changed in transactions (#347) | `store.rs` |
| Connectivity | Bonjour nearby; private networks (Tailscale); Relay's relay and lookup (pkarr) in Hybrid only; Private never uses the servers | `resolver.rs`, `presence.rs` |
| Servers | relay.avedeus.ovh (forwards encrypted connections) and lookup.avedeus.ovh (publishes only the relay address) | Hetzner |

The rights are already a small capability model. Capabilities grow from
them rather than from a new scheme: new rights are named the same way
("may send here", "may ask", later "may open sites").

## Build on what exists

| Problem | Technology |
|---|---|
| Encrypted device connections | iroh |
| Resumable file exchange | iroh-blobs |
| Short-code pairing | SPAKE2 |
| Finding nearby devices | Bonjour / mDNS |
| Finding remote devices | pkarr |
| Persistent storage | SQLite |
| Group encryption | MLS, through an established implementation (OpenMLS) |
| Social interoperability | Nostr |

Where each one stops:

- iroh gives encrypted, authenticated transport between two devices. It
  doesn't give group encryption, identities for people, or application
  permissions.
- MLS does group key management; Relay never invents its own group
  cryptography.
- Nostr is for interoperability, never a replacement for device trust.
- SQLite and the outbox stay the base of durable delivery.

No new transport abstraction, crypto framework or messaging engine comes
in unless one of these hits a concrete limit.

**For every new requirement, before building:**

1. Does an existing library or protocol already solve it?
2. Can it work with Relay's identity and trust model?
3. Does it bring a second source of truth?
4. What does it add to run and operate?
5. Is building it ourselves genuinely necessary?

The aim is the smallest combination that gives Relay the properties it
needs, not every interesting technology. This applies in particular to
MLS for groups, Nostr for interoperability, existing secure messaging
protocols, service discovery and remote access, and task queues.

## The biggest gap: two messaging systems

Relay holds two messaging systems today:

| | Device conversations | Chat server conversations |
|---|---|---|
| Transport | iroh connections | HTTP and Server-Sent Events |
| Who is who | Device keys, word-code pairing | Server accounts and tokens |
| Where messages live | On the devices (outbox, database) | On the server |
| Encryption | Encrypted and authenticated end to end between devices | Plain HTTP as documented; the token crosses the network in cleartext ([messaging.md](messaging.md)) |

The Chats tab shows both, so a conversation there isn't end-to-end
encrypted just because Relay's device links are. Keeping server mode
frozen when Chat merged in was right at the time
([relay-chat-merge.md](relay-chat-merge.md)); the requirement now is
stronger: **all communication uses one security model, even when it is
delivered different ways.** The plan keeps the messaging engine
(chat-core) and its screens, and moves the server path onto Relay's
identities and encryption, stage N4 below.

## Easy setup

One of Relay's defining traits.

- **Nearby:** no Relay account, no server, no addresses. Show or scan a
  code, confirm trust, start talking. This works today.
- **Away from home:** no VPS, DNS, open ports or Tailscale needed. Relay
  offers a managed relay and lookup, and self-hosting stays available.
  The resolver already has the pieces (Bonjour, Tailscale, pkarr, the
  relay); what's missing is making Hybrid work without the person
  understanding them, while the privacy controls stay clear.

That points to Hybrid as the default with Relay's servers on. Today
"Use Relay's Servers" is off by default and Private never uses a relay
(decided 2026-11-19); changing the default is Reuben's call when N2
starts.

## Security claims, precisely

The relay can't read what devices send, but networks still see metadata.
Relay's claims are kept separate:

| Claim | What Relay promises |
|---|---|
| Content confidentiality | Servers can't read messages or files: device connections are encrypted end to end |
| Peer authentication | A device checks it reached the expected key; a revoked device is refused |
| Metadata privacy | Minimised, not hidden. The relay sees IP addresses, timing, traffic volume and endpoint ids of the connections it carries; the lookup holds each device's endpoint id and relay address. Private mode uses neither |
| Availability | Devices still reach each other when a server is down, where the route allows (nearby, Tailscale) |
| User authority | Revoking a device removes its access, including a transfer in progress (#344) |

The chat server path makes none of these claims until N4.

## The stages after 1.0

These stages describe possible dependencies and a useful order for
exploration, not a commitment to build every stage. Each one must justify
itself through practical use, or through evidence of a limit in the
existing layer.

M1 and M2 keep their definitions and acceptance criteria
([Reliable Trusted-Device Delivery](relay-reliable-delivery.md) and the
Personal Device Network in the [M1/M2 brief](relay-m1-m2-brief.md)); the
1.0 acceptance run proves them together on the phone, it doesn't replace
them. The stages after them are named N1 to N9 so they don't collide.

| Stage | Focus | Purpose |
|---|---|---|
| M1, M2 | Reliable Trusted-Device Delivery, Personal Device Network | As defined; with #353 and the 1.0 acceptance run, the proven device foundation ([audit, step 5](relay-audit-2026-11.md)) |
| N1 | My Devices | A person's identity across devices; trust and revoke once. Step 1, the introducer, is on main; next is a signed device list |
| N2 | Away from home | Secure communication beyond the home network, without manual networking |
| N3 | The encrypted mailbox | Somewhere things wait while a device is away: its own security, retention and availability questions ([review, refinement 3](relay-review-2026-11.md)) |
| N4 | One messaging security model | The chat server path on Relay's identities and encryption |
| N5 | Capabilities and permissions | Devices offer services safely, each behind its own right |
| N6 | The first capability | Sites, or transcription on the laptop: proof that new functions come from the layer |
| N7 | Groups and people | Rooms and Spaces on MLS, people beyond your own devices |
| N8 | Continuity and cooperation | Sync, handover, tasks (Workshop) |
| N9 | Federation and other participants | Nostr interoperability; AI agents with restricted rights, last |

Not started yet, on purpose: Social, Computing and Workshop.

## How the earlier plans map onto this

| Earlier | Now |
|---|---|
| M1 Reliable Delivery, M2 Personal Network ([future directions](relay-future-directions.md)) | Kept as M1 and M2 |
| M3 Universal Exchange | N8 |
| M4 People and Rooms | N1 (a person's identity), N7 (rooms and groups) |
| M5 Capabilities and Extensions | N5 (the model), N9 (outside developers) |
| M6 Distributed Intelligence | N8 (tasks), N9 (agents) |
| M7 Private Service Network | N5, N6 |
| M8 Open Network Ecosystem | N9 |
| Computing M3 to M9 ([relay-computing.md](relay-computing.md)) | N5, N6, N8, N9 |
| Social S1 ([relay-social.md](relay-social.md)) | N1 |
| Social S2 to S8 | N7, N9 |
| Rooms on Nostr ([relay-rooms-protocols.md](relay-rooms-protocols.md)) | N7 |
| Foundations R0 to R7 ([study](relay-foundations-study.md)) | Research behind N5 to N8 |
| Review P5 My Devices, P6 mailbox ([review](relay-review-2026-11.md)) | N1, N3 |
| Sites over Relay (parked, [audit](relay-audit-2026-11.md)) | N6 |
| Chat's server mode ([relay-chat-merge.md](relay-chat-merge.md)) | N4 |

## Research underneath

The [foundations study](relay-foundations-study.md) informs this
architecture; it isn't another implementation track. Plan 9 (resources
and services named the same way across a network), BEAM (processes,
messaging, supervision, recovery), seL4 (capabilities and explicit
authority) and distributed systems (offline work, consistency,
discovery, coordination) lend ideas; Relay doesn't recreate them.

## The other documents

Design explorations that refer back here:

- [relay-social.md](relay-social.md): people and communities.
- [relay-computing.md](relay-computing.md): structured requests and
  execution.
- [relay-rooms-protocols.md](relay-rooms-protocols.md): group
  communication.
- [relay-foundations-study.md](relay-foundations-study.md): research and
  rationale.
- [relay-future-directions.md](relay-future-directions.md): longer-term
  applications and the earlier milestones.
- [relay-audit-2026-11.md](relay-audit-2026-11.md): the 1.0 plan and Sites
  over Relay.
- [messaging.md](messaging.md): the chat server path that N4 moves.
