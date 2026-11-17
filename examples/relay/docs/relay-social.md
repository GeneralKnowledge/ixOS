# Relay Social: open social infrastructure (idea)

Status: an idea from Reuben, 2026-11-17 ("another mud at the wall idea"),
recorded so it isn't lost, then reviewed by him the same evening (kept,
with the clarifications below). **Nothing here is built until he says so.**
It is a proposed long-term direction for Relay and a workstream of social
work packages (S0 to S8), not a replacement for Relay's roadmap. The current milestone,
[M1 Reliable Trusted-Device Delivery](relay-reliable-delivery.md), and then
M2 Personal Device Network come first. The architectural parent stays
[relay-platform-plan.md](relay-platform-plan.md). Other parked ideas are in
[relay-future-directions.md](relay-future-directions.md).

## How this fits Relay's milestones

S0 to S8 are **work packages that consume Relay's capabilities**, not a
second milestone system. S0 can begin as documentation when Reuben
authorises it; S1 onwards are each gated by the Relay platform foundation
they use, and Social never reimplements one:

| Social package | Waits for |
|---|---|
| S1 Portable person identity | M4 People and Rooms (person identity, several devices per person) |
| S2 Public social objects | S1, and M2's internet reach |
| S3 Spaces and membership | M4 (rooms and membership) |
| S4 Moderation and governance | S3, and M5 permissions |
| S5 Private Spaces | M4 rooms on Marmot/MLS; messaging stays chat-core |
| S6 The client | S2 and S3 |
| S7 Independent hosting and migration | S3, S5, and the portability cases below |
| S8 Federation and developers | M5 Capabilities and Extensions, M8 Open Network Ecosystem |

Person identity, room membership, messaging and capability permissions
belong to the Relay platform; Social uses them.

## Relay and Relay Social

**Relay is the infrastructure. Relay Social is the first-party social
application.** The Relay app is centred on Chats, Send and Devices: a
personal network. Social brings a different interaction model (Pulse,
Explore, Spaces, Inbox, Identity). They share infrastructure, identity,
permissions and services, and could share an SDK and account experience,
but their interfaces are not merged into one app by default:

- **Relay:** personal connectivity and device management.
- **Relay Social:** people, communities and publishing.
- **Third-party applications:** other experiences on the same
  infrastructure.

## Product statement

> Relay connects people, communities and applications through a shared,
> open social network, without requiring them to surrender ownership of
> their identities, relationships or spaces.

This is not a decentralised Facebook clone. It is infrastructure on which
different social networks, communities and applications can run, and the
default Relay app would be the first complete social experience built on
it.

## The vision: a network of spaces, not a universal feed

Relationships and spaces are the primary structure. A feed is only one way
of viewing activity across them, and the person controls it
(chronological, priority, unread, chosen topics). There is no
engagement-maximising algorithm by default, and any recommendation says why
it appears and can be turned off.

**Five fundamental objects:**

| Object | Meaning |
|---|---|
| Person | A human identity, with profiles, keys and relationships |
| Space | A persistent social environment owned by a person or organisation |
| Relationship | A connection between people, spaces or services, with its own permissions |
| Object | Something created or shared: a post, image, discussion, event, document or project |
| Activity | Something that happens: publishing, replying, joining, reacting, inviting, membership changes |

**A Space** is a persistent social context: discussions, announcements,
members and roles, files, events, audio and video rooms, tools, and its own
moderation and retention rules. Three different things, with different
permission models:

| Part | What | Example (a music community) |
|---|---|---|
| **Space** | Identity, membership, governance and context | The community, its members, roles and rules |
| **Space content** | Posts, discussions, files and events, authored by people | Discussions, an events calendar |
| **Space applications** | Services running inside the Space under explicit permissions (Relay capabilities, M5) | A shared playlist, a listening room |

Membership doesn't grant an application anything: an application gets only
the capabilities it is granted in that Space. Its owner decides where it runs; members
decide whether they trust that operator. Space types (public community,
private group, personal, project, creator, temporary event) are templates,
not separate products.

**The client's five areas:** Pulse (your chosen view of activity), Explore
(replaceable discovery tools; a provider can recommend but never grant
membership or trust), Spaces, Inbox (one inbox for messages, group
conversations, notifications, invitations and permission requests, built on
the existing Chat integration), and Identity (profiles, with separate
profiles for separate contexts).

## Ownership and hosting: hybrid

- **Relay-hosted:** the easy default. Relay runs hosting, discovery,
  indexing and optional media; people keep a portable identity and can
  leave.
- **Independently hosted:** organisations and communities run their own
  infrastructure and rules, using Relay's client and protocols without
  Relay's servers.
- **Personal infrastructure:** optional, for advanced users (home server,
  VPS).

Owning a key doesn't mean owning every copy of a post. Authority per
resource: identity by the person's signing keys; device identity by its
device key; profile signed by the person; relationships user-controlled and
exportable; community governance by the Space's declared rules; community
content by its authors' rights and the Space's retention; hosting and
indexes by the provider; private group messages encrypted for members;
local data stored locally and exportable. The official service is a
convenience provider, not the root authority.

**Business model:** open-source core, usable without paying; paid managed
hosting, community Spaces, storage and media, private indexing,
organisation admin, backups and migration, availability. No advertising, no
selling the social graph, no paid reach.

## Architecture

Social is a new family of capability services above Relay's existing
layers:

```
Client applications     Relay Social, community clients, creator tools, third-party apps
Social app services     Pulse, profiles, Spaces, publishing, membership, discovery, moderation
Social protocol layer   identity bindings, signed events, social graph, Space protocols, federation adapters
Relay platform          person/device identity, trust, permissions, sessions, Chat, Transfer, service discovery
Infrastructure          official hosting, independent operators, personal nodes, iroh, storage
```

**Boundary:** profiles, posts, social graphs, Spaces and moderation never go
into Relay Core. Relay Core knows who is connecting, which identity and
device, what they may do, how a connection is made and how data is
delivered. This is the same split M1 keeps between trust, delivery and
conversation.

**The social graph and the device network stay separate.** Following
someone creates no trusted-device connection; joining a Space grants no
access to anyone's laptop; being an admin grants no access to members'
files.

**Domain entities (concepts, not ten tables):** Person, Device, Space,
Membership, Relationship, SocialObject, Activity, Grant, ModerationDecision,
StorageReference. Adapters translate protocol events into a
protocol-independent `SocialObject` and back, and keep what a protocol
can't express instead of pretending it is equivalent.

## Protocols: adapters over existing standards

| Concern | Choice |
|---|---|
| Person identity | Nostr keys |
| Device identity | iroh keys (as today) |
| Person and device binding | A signed Relay identity statement |
| Public content and follows | Nostr (NIP-01, 02, 10, 18, 23, 65; NIP-11 relay info) |
| Private messages | chat-core first; NIP-17 compatibility later |
| Encrypted groups (private Spaces) | Marmot over MLS (RFC 9420) |
| Public, moderated Spaces | NIP-29 (a draft: relay-enforced governance, so hosting authority and migration limits must be shown) |
| Transport and media | iroh and iroh-blobs; the connection resolver picks the route |
| External federation | ActivityPub adapter, later; not the internal model |
| Service integrations | Relay Service protocol |

Rooms on Nostr (Marmot, NIP-29, NIP-11) are already approved in the
platform plan, so this extends an existing choice rather than adding one.

**Governance is not encryption:** NIP-29 describes relay-managed group
membership and moderation; it does not by itself provide end-to-end
encryption. A public or private NIP-29 group must not be described as
E2EE merely because it has membership controls. Private Spaces require a
separate, correctly integrated end-to-end encryption layer such as
Marmot/MLS, with its own membership, key-rotation and removal guarantees.
The compatibility profile and UI must state these properties separately.

The table is a start. S0's compatibility profile says, for each protocol
feature, what Relay implements, what it only understands, what it keeps but
can't show, what it can publish, and what happens when an external client
behaves differently, so Relay never looks compatible while silently losing
information. This matters most for NIP-29 (a draft) and for the gaps
between Nostr and ActivityPub.

**Not to build:** a new public event protocol, new encryption, a new
federation protocol, a replacement for iroh, a second messaging engine, or a
proprietary identity that locks people in.

**Content lifecycle:** the author signs, providers distribute. Edits are new
signed versions; deletion is a signed request or tombstone that the
author's own services honour, while independent providers keep their own
retention (to be disclosed). Copies already received can't be guaranteed to
disappear.

**One person, several devices:** distinguish the long-term person identity
key from the keys used by individual devices. The identity key (or a
documented recovery mechanism rooted in it) authorises device associations;
each device keeps its own revocable key. Where supported, scoped or
time-limited delegated signing keys can reduce exposure of the identity key.
The protocol profile must specify which key signs each event, how rotation
and revocation work, and what happens when the identity key is lost. Do not
silently treat an iroh device key as the person's Nostr identity key.
Person identity and recovery come before profiles depend on them (deferred
today until people and groups need them, M4).

## Portability, precisely

"Portable" covers six different problems, and nothing promises seamless
migration until each case is tested:

| Kind | What it means | Hard part |
|---|---|---|
| Identity portability | The person's key and profile work with another client or provider | Recovery, device associations |
| Membership record portability | Who belongs to a Space, with which role, moves with it | Signed membership history across operators |
| Content exportability | Authored content can be exported and republished | Others' content follows its authors' rights |
| Media availability | Files stay reachable after a move | Media lives with providers; availability is reported, not assumed |
| Private encryption-state migration | An encrypted Space keeps working after a move | MLS group state, device membership and key rotation need an explicit procedure: not copying a database |
| Governance continuity | Rules, moderators and decisions carry over | NIP-29 governance is relay-enforced; a fork isn't a migration |

## Safety and moderation

Five systems: identity security (keys, revocable devices, documented
recovery, pseudonyms allowed, no legal name or phone needed); relationship
controls (follow, subscribe, mute, block, restrict, accept or reject
messages, who may mention or invite); Space governance (a policy shown
before joining: operator, admins, membership, who publishes and moderates,
retention, encryption, operator access, appeals, migration); layered
moderation (user, Space, hosting operator, client, discovery provider; a
Space ban isn't a network ban); abuse resistance (rate limits, invitation
limits, bot detection, reports, link warnings, optional attachment
scanning, audit records). No universal reputation score.

## Deployment

Logical services: identity, event relay, index, Space, media,
notification, federation adapters, admin. Not eight microservices: one Rust
application with SQLite or PostgreSQL at first, split only when scaling or
ownership needs it. No Kubernetes, distributed database or event bus at the
start. Progression: local instance, one official server, an independent
operator, several relays and indexes, personal hosting, federation.

## Workstream S0 to S8 (proposed)

| Phase | What | Exit |
|---|---|---|
| S0 | Architecture baseline: social architecture doc, protocol ADRs, data model and event lifecycle, threat model, compatibility test strategy. Docs only | It can be built without new networking or cryptography |
| S1 | Portable person identity: Nostr key, recovery, device association and revocation, signed profiles, import and export | One identity works in two Relay clients and survives a device replacement |
| S2 | Public social objects (first Nostr adapter): posts, replies, reposts, reactions, follows, edits and deletions, several relays, local store and index | Publish on one instance; read, verify and interact from another independent client |
| S3 | Spaces and membership: create, discover, join, invite, roles, channels, NIP-29 adapter, operator and policy shown | A Space hosted independently works for two clients with no central Relay account service |
| S4 | Moderation and governance: moderator rights, remove and restrict, block and mute, reports and appeals, audit, rate limits | An independent operator runs a usable community without Relay's moderators |
| S5 | Private Spaces: Marmot/MLS, device-aware membership, removal and key rotation, Chat integration, attachments through Relay Transfer | A removed member can't read later messages; Relay messaging keeps working |
| S6 | The full client experience: Pulse, Explore, Spaces, Inbox, Identity, search, several indexes, user-built views | Someone can discover, join, publish and talk without knowing what a relay is |
| S7 | Independent hosting and migration: operator package, backups, export, Space migration, provider choice, diagnostics, limits | A community changes provider without losing identity, members or portable content |
| S8 | Federation and developers: ActivityPub bridge, public API, third-party clients, Space app manifests, extension permissions | Third parties build interoperable apps without the official client |

S6 is the complete first-party client experience, not a gate that prevents
earlier end-to-end testing. S2 and S3 will need small temporary or minimal
test clients, command-line tools, and/or development screens to prove
publishing, verification, membership and independent hosting before S6.
Those are validation tools, not an attempt to build the finished client
early.

Each phase is several small PRs with their own tests, fixtures and phone
checks; never one giant Social PR.

### Relay Social 0.1 (the first usable version)

Create an identity, edit a profile, follow someone, post text, reply,
react, create a public Space, join and leave it, post a discussion there,
browse activity from people and Spaces, use two independently hosted
instances, export identity and data, verify every signature. No video,
recommendations, marketplace, monetisation, AI moderation or extensions.

### Platform acceptance tests

Identity made on instance A works on B; a post published on A is read on
B; a relay disappears and another still has the post; a follow list moves
to a new client; a Space runs on an independent host; leaving a Space is
respected; a ban is enforced; a block holds; a revoked device stays
separate from the social identity; after an MLS member is removed, they
cannot send valid group messages or decrypt messages created after removal
(and the test documents the exact MLS epoch/key-update boundary); a missing
media provider is reported accurately; an ActivityPub user
replies to a Relay post; a client keeps an extension it doesn't understand.
And the failure test: **take the official Relay infrastructure offline**,
and people with identities on independent infrastructure keep working.

### Parked within the idea

A custom federation protocol, a global recommendation algorithm, tokens or
cryptocurrency, a global reputation score, advertising, a distributed
database, own voice and video infrastructure, a marketplace, autonomous AI
agents, universal sync.

## Three safeguards (Reuben's)

1. Don't interrupt the current milestones: finish reliable delivery and the
   personal device network first.
2. Don't build a proprietary social protocol: Nostr, Marmot/MLS, iroh and
   later ActivityPub, through adapters.
3. Make independence demonstrable: the first meaningful release works
   across two independently operated instances.

**Next step, when Reuben says so:** S0, a small docs PR. This file stays the
overview and doesn't grow into an implementation specification; S0 adds:

| Document | Purpose |
|---|---|
| `relay-social-domain.md` | Person, Space, Relationship, Object, Activity, and permissions |
| `relay-social-compatibility.md` | Protocol support and interoperability rules (above) |
| `relay-social-security.md` | Threat model, moderation, identity, private Spaces |
| `relay-social-adr.md` | Decisions, and the dependencies on Relay's milestones |

No S1 or implementation PR before that, and none of it ahead of M1:
reliable delivery is what makes a social network on Relay useful rather
than only decentralised in theory.
