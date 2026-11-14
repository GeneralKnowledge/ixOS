# Relay rooms: which existing chat protocol to build on

2026-11-14. Part of [relay-transfer-plan.md](../relay-transfer-plan.md), under the [Relay platform plan](../relay-platform-plan.md) (milestones M4 and M5).

## What Reuben wants

Rooms on a network of servers that are "types of relays", an IRC and
Discord hybrid. Each server chooses what it supports: one is only a relay,
another lets people run their own rooms ("sub-servers") on it, another can
read and keep what is said. Members see a server's choices before they join.
End to end encryption is the default. No accounts. Build on existing
infrastructure, not our own network.

## The candidates

| | Servers | Identity | End to end groups | Ephemeral rooms | "Server chooses what it supports" | Fit |
|---|---|---|---|---|---|---|
| **Matrix** | Homeservers that federate; every server in a room keeps the room's full history | An account on a homeserver | Megolm today; MLS for Matrix is still being designed (decentralised MLS drafts, 2026) | No: history is the model | Partly (room versions, server config) | Closest to Discord (Spaces, rooms, moderation). Its Rust SDK powers Element X, a SwiftUI app on Rust through UniFFI, the same shape as ixOS. But accounts, permanent replicated history and heavy servers go against account-free and rooms that vanish. Best as a **bridge later**, not the base |
| **IRC / IRCv3** | Networks of servers; the server sees everything | A nickname (accounts optional) | None | Yes, by nature | Yes (ISUPPORT, capability negotiation) | The original "server types" model and still everywhere, but no encryption and no files. Good as an optional **gateway** for "host can read" rooms, so any IRC client can join |
| **XMTP** | A node network run by XMTP's operators, moving towards decentralisation | Inbox ids tied to wallets | MLS (libxmtp, Rust) | No | No: one network | Good MLS code to learn from, wrong network: we can't run our own servers in it the way Reuben wants |
| **Nostr** | Relays: simple WebSocket servers anyone runs, thousands public; clients pick which relays to use | A key pair, no account | **Marmot** (formerly NIP-EE): MLS groups over any relays, which see only ciphertext; Rust implementation used by the White Noise app; reviewed by Least Authority | Yes: ephemeral event kinds (20000 to 29999) that relays pass on and never store | **Yes, built in**: every relay publishes a NIP-11 information document (supported NIPs, limits, retention, payment), and NIP-29 lets a relay host groups that it moderates | **Best fit** (below) |
| **iroh-gossip** | None: peers broadcast to each other on a topic | iroh endpoint key | Not built in | Yes | No servers to choose | A primitive for small serverless rooms on a LAN. Still pre-1.0 (many breaking releases). Not the server network Reuben describes |

## Recommendation: rooms speak Nostr

Map Reuben's server types onto what Nostr already has:

| Reuben's server type | In Nostr | Can it read? |
|---|---|---|
| **Just a relay** | Any standard relay: public ones (existing infrastructure) or our own on Hetzner (an off-the-shelf relay such as strfry or nostr-rs-relay) | No: rooms on it are Marmot (MLS), ciphertext only |
| **Sub-servers for others to run** | A relay that hosts NIP-29 groups and lets people create and moderate their own | Only for groups that choose to be readable; Marmot groups ride it like any relay |
| **Host can read** | A NIP-29 group: the relay is the group's host, enforces members, admins and bans, and sees the messages | Yes, and the client says so before joining, from the relay's NIP-11 document and the group's settings |
| **Room that lives only while the host keeps it open** | The host device runs a tiny relay itself (over iroh on the LAN, or reachable through our iroh relay), with ephemeral event kinds only. When it stops, the room is gone | E2E (Marmot) by default; the host-reads variant is a NIP-29 group on that tiny relay |

What it gives:

- **Existing infrastructure**: thousands of public relays, many clients, and
  people who already have Nostr keys. A Relay room could be joined from
  another Nostr client that supports Marmot or NIP-29.
- **Capabilities are a standard**: the "what this server supports" screen
  reads NIP-11, not a format of ours.
- **MLS for groups**, through an audited implementation, not our own key
  scheme; a removed member can't read what follows.
- **No accounts**: a key pair per person, made on the device.

What stays Relay's own:

- **Files** go device to device over iroh (stage 1), never through a Nostr
  relay. A room message that shares a file carries an iroh-blobs ticket.
- **Messages between your own trusted devices** stay direct over iroh
  (stage 1), as planned.
- **Security levels** (Relaxed, Standard, Strict) are client behaviour, so
  they are Relay's, written into the room's settings so every Relay client
  enforces them.
- **Chat reuse** still holds: a `NostrRepo` backend for `chat-core`'s
  `ChatRepo` shows rooms in Chat's conversation screen.

Costs and risks:

- Two keys per device: iroh's Ed25519 endpoint key and a Nostr secp256k1
  key. The device's Nostr key signs a statement of its iroh key so others can
  link them.
- Nostr relays use WebSockets, so the internet path is TCP to the relay; the
  LAN path for a host-kept room is our tiny relay over iroh.
- Marmot is young (first apps 2025). A spike must prove the Rust
  implementation, its size on iOS, and that it works with another client.
- Public relays have their own rules and may drop or rate-limit; rooms
  should list two or three relays, ours first.

## A spike before stage 2

1. A Marmot group of three on Linux through a local relay and through our
   own relay on Hetzner; remove a member; check the removed one can't read on.
2. A NIP-29 group on a relay that supports it; read its NIP-11 document and
   show "the host can read this room".
3. A host-kept room: the tiny relay inside `relay`, over iroh, with
   ephemeral kinds; stop it and check every client sees the room close.
4. Build the Nostr and Marmot crates for iOS (on the laptop, through the
   build queue) and note the size.

## Sources

- Matrix and MLS: <https://arewemlsyet.com/>,
  <https://datatracker.ietf.org/doc/draft-kohbrok-mls-dmls/>
- Nostr NIP-29 relay groups and Marmot:
  <https://nostrdev.com/nostr-group-chat-protocols/>,
  <https://leastauthority.com/white-noise-marmot-protocol-review-summary-report/>
- XMTP decentralisation: <https://xmtp.org/decentralization>
- iroh-gossip: <https://docs.iroh.computer/connecting/gossip/>
