# Relay Social: open social infrastructure (idea)

Status: an idea from Reuben, 2026-11-17 ("another mud at the wall idea"),
recorded so it isn't lost. **Nothing here is built until he says so.** It
is a proposed long-term direction for Relay and a new workstream (S0 to S8),
not a replacement for Relay's roadmap. The current milestone,
[M1 Reliable Trusted-Device Delivery](relay-reliable-delivery.md), and then
M2 Personal Device Network come first. The architectural parent stays
[relay-platform-plan.md](relay-platform-plan.md). Other parked ideas are in
[relay-future-directions.md](relay-future-directions.md).

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
moderation and retention rules. Its owner decides where it runs; members
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

**Not to build:** a new public event protocol, new encryption, a new
federation protocol, a replacement for iroh, a second messaging engine, or a
proprietary identity that locks people in.

**Content lifecycle:** the author signs, providers distribute. Edits are new
signed versions; deletion is a signed request or tombstone that the
author's own services honour, while independent providers keep their own
retention (to be disclosed). Copies already received can't be guaranteed to
disappear.

**One person, several devices:** the person's key signs device
associations; each device keeps its own key, so devices are revoked
individually and the identity survives a lost device. Person identity and
recovery come before profiles depend on them (deferred today until people
and groups need it, M4).

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
| S6 | The client: Pulse, Explore, Spaces, Inbox, Identity, search, several indexes, user-built views | Someone can discover, join, publish and talk without knowing what a relay is |
| S7 | Independent hosting and migration: operator package, backups, export, Space migration, provider choice, diagnostics, limits | A community changes provider without losing identity, members or portable content |
| S8 | Federation and developers: ActivityPub bridge, public API, third-party clients, Space app manifests, extension permissions | Third parties build interoperable apps without the official client |

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
separate from the social identity; a removed MLS member can't send into the
group; a missing media provider is reported accurately; an ActivityPub user
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

**Next step, when Reuben says so:** S0, the architecture baseline (ADRs,
domain model, compatibility profile, acceptance tests), as a docs PR.
