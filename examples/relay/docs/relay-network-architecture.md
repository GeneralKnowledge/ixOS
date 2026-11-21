# Relay network architecture

Status: Reuben's direction, 2026-11-21. This is the centre that the other
Relay idea docs refer back to. It changes no commitment: the Relay 1.0
foundations come first and stay small
(the audit doc, relay-audit-2026-11.md, step 5, which lands with the 1.0 PRs), and nothing below is built
until Reuben says so. Everything past "What exists today" is a
hypothesis in the sense of the
[open-ended design principles](relay-open-ended-design-principles.md):
each one says what already does it and what would prove it.

## The idea

> Relay is a private network of cooperating devices, people and
> services.

Social networking, remote computing, private sites, rooms and device
handover aren't separate projects. They are things one network can
support, sharing one identity system, one set of permissions, one way to
find each other and one way to talk.

**The principle to protect:** don't build a separate networking system
for each new Relay idea; build one network that supports all of them.
That doesn't mean one giant app or one monolithic protocol. It means
common primitives, the same security everywhere, data models that fit
together, and services that compose.

## What exists today

Written from the code, so this part doesn't go stale. "Main" is merged;
the PR numbers are the 1.0 foundations still open.

| Primitive | Today | Where |
|---|---|---|
| Device identity | One iroh endpoint per device; its id is the device's identity | `node.rs`; key in the Keychain on iPhone (#348) |
| Trust and revocation | A trusted-device list; revoked is refused for good until the person trusts again | `trust.rs` |
| Grants per device | Separate rights, never implied by trust: "may send here" (`receives_from`), "may ask" (`may_ask`, #344) | `trust.rs` |
| One protocol per job | Each job is its own ALPN on the one endpoint: word codes (`forge-relay/code/1`), device messages (`forge-relay/device/1`), files (iroh-blobs) | `node.rs`, `code.rs`, `device.rs` |
| Delivery | One outbox: messages and files wait, retry and arrive once, in order (#340) | `outbox.rs` |
| Conversations | Each trusted device has a conversation; files are its attachments | `device_chat.rs` |
| Persistent data | One SQLite database, changed in transactions (#347) | `store.rs` |
| Presence | Nearby, reachable or offline, kept across launches; a device can say what it offers (its own claim, granting nothing) | `presence.rs` |
| Finding a device | The resolver tries local, private networks (Tailscale), then Relay's own relay and lookup servers; Private mode never uses the servers | `resolver.rs` |
| Servers | relay.avedeus.ovh (forwards encrypted bytes) and lookup.avedeus.ovh (publishes only the relay address) | Hetzner |

The grants are already a small capability model. The architecture grows
from them rather than designing a new scheme: new rights are named the
same way ("may send here", "may ask", later "may open sites").

## Shared foundation

Every capability below uses these, and none reimplements them:

1. **Identity and trust.** Device identity today; a person's identity
   (several devices) from My Devices.
2. **Connectivity.** One endpoint, the resolver, the relay.
3. **Encryption and privacy.** iroh's encrypted connections between
   devices; MLS for groups later; servers only ever see encrypted bytes.
4. **Persistent data.** The database and the outbox.
5. **Discovery and presence.**
6. **Permissions and policy.** Grants per device, later per person,
   Space and agent; each one revocable.

## Five connected concepts (proposed)

### 1. Capabilities and services

A device offers capabilities: host a site, store files, transcribe
audio, run a model, run a task, share a folder, provide an API, act as a
gateway. A capability is a protocol on the endpoint plus a grant that
says who may use it. A private site is one kind of service; a
transcription request is another, found, authorised and connected the
same way.

- **Grows from:** the grants, one ALPN per job, presence's "what it
  offers".
- **Proved by:** two small capabilities on the same machinery: Sites
  (parked in the audit doc) and transcription on the laptop.

### 2. Tasks and cooperation

A task asks a device with a capability to do work and return a result
that arrives even if the connection drops. Relay Workshop is the
interface over this, not its own device-to-device system.

> The phone picks a recording and creates a transcription task. Relay
> finds an authorised device that offers transcription. The laptop
> accepts and runs the model locally. Relay delivers the result back and
> keeps it in the right conversation or Space.

- **Grows from:** the outbox (delivery that survives restarts) and the AI
  asks (#344's "may ask").
- **Proved by:** the transcription task surviving the laptop going
  offline halfway (Workshop's failure cases).

### 3. Identity and Spaces

Where Social and Rooms meet. A person has an identity; a device belongs
to, or is trusted by, a person. A Space is a shared place with members,
permissions and resources: conversations, rooms, files, services and
apps, where people and devices cooperate, not just a chatroom. Nostr is
the optional public-facing identity and interoperability layer; MLS
encrypts group conversations and membership.

- **Grows from:** My Devices (one identity for one person's devices).
- **Proved by:** two people sharing a Space that holds a conversation
  and one service, and a removed member can't read what follows.

### 4. Continuity and presence

Moving between devices feels natural: the clipboard follows you, files
and folders sync, browser sessions and apps hand over, tasks continue
when a device goes offline.

- **Grows from:** presence, the outbox, and the mailbox (somewhere work
  waits while a device is away).
- **Proved by:** a copied link pasted on the laptop, and a task started
  on the phone that finishes while the phone is asleep.

### 5. Participants beyond people

An AI agent is another kind of participant: its own identity, joining a
Space with permission, taking tasks, running a service or talking in a
conversation, held to exactly the same grants as anyone else. Simulated
chatrooms are one experiment on this model, not a product.

- **Grows from:** capabilities, tasks and Spaces, with an audit trail.
- **Proved by:** an agent that can do only what it was granted, and
  whose every action can be traced. This carries the most risk, so it
  comes last and never with a shortcut around the grants.

## Research underneath

The [foundations study](relay-foundations-study.md) informs this
architecture; it isn't another implementation track. In particular:
Plan 9 (resources and services named the same way across a network),
BEAM (processes, messaging, supervision and recovery), seL4 (capabilities
and explicit authority) and distributed systems (offline work,
consistency, discovery, coordination). The aim is to borrow their best
ideas, not to recreate them.

## Stages after 1.0 (proposed)

Named N1 to N8, because M1 and M2 already mean reliable delivery and the
personal network in the existing docs and PRs.

| Stage | Focus | What it unlocks |
|---|---|---|
| 1.0 | Device foundations | A trusted, persistent device network |
| N1 | My Devices | One identity for your devices: learn, trust and revoke once |
| N2 | Away from home and the mailbox | Reliable reach beyond the home network; work waits while a device is away |
| N3 | Capabilities | Services devices offer and who may use them (Sites first) |
| N4 | Cooperation | Tasks and remote execution; Relay Workshop |
| N5 | Shared Spaces | People, memberships, conversations and services in one place |
| N6 | Continuity | Handover, sync and activity that carries on |
| N7 | Federation | Nostr interoperability and hybrid hosting |
| N8 | Other participants | AI agents and experimental network behaviour |

Reuben's draft put Shared Spaces straight after Away from home. This
puts Capabilities and Cooperation first: they need only one person's own
devices, which My Devices provides, while Spaces bring in other people,
membership and MLS. Spaces can then hold services that already exist.
The order is his call.

## How the earlier plans map onto this

| Earlier | Now |
|---|---|
| M1 Reliable Delivery, M2 Personal Network ([future directions](relay-future-directions.md)) | Done or in 1.0 |
| M3 Universal Exchange | N6 Continuity |
| M4 People and Rooms | N1 (a person's identity), N5 (rooms and groups) |
| M5 Capabilities and Extensions | N3 (the model), N7 (outside developers) |
| M6 Distributed Intelligence | N4 (tasks) and N8 (agents) |
| M7 Private Service Network | N3 (services) |
| M8 Open Network Ecosystem | N7 |
| Computing M3 to M6 ([relay-computing.md](relay-computing.md)) | N3, N4 |
| Computing M7 to M9 | N7 |
| Social S1 ([relay-social.md](relay-social.md)) | N1 |
| Social S2 to S7 | N5, N7 |
| Social S8 | N7 |
| Rooms on Nostr ([relay-rooms-protocols.md](relay-rooms-protocols.md)) | N5 |
| Foundations R0 to R7 | Research behind N3 to N6 |
| Review P5 My Devices, P6 mailbox ([review](relay-review-2026-11.md)) | N1, N2 |
| Sites over Relay (parked, audit doc) | N3, first capability |

## Security boundaries

- Trust admits a device; it grants nothing else. Every capability has its
  own grant, and revoking trust ends them all.
- Nothing listens on the public internet. Services listen on their host
  only and are reached over Relay's encrypted connections.
- Servers are untrusted couriers: the relay forwards encrypted bytes, the
  lookup publishes only the relay address, and the mailbox (N2) holds
  encrypted items it can't read.
- Agents get no authority a person couldn't grant, and every action is
  recorded.

## The other documents

These are design explorations that refer back here:

- [relay-social.md](relay-social.md): social experiences and federation.
- [relay-computing.md](relay-computing.md): the capability and execution
  model.
- [relay-rooms-protocols.md](relay-rooms-protocols.md): encrypted group
  communication.
- [relay-foundations-study.md](relay-foundations-study.md): research and
  rationale.
- [relay-future-directions.md](relay-future-directions.md): longer-term
  applications and the earlier milestones.
- The audit doc (relay-audit-2026-11.md): the 1.0 plan and Sites over
  Relay.

Relay Workshop is the first substantial app that uses several shared
capabilities at once.
