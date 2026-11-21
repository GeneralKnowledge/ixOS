# Relay: future directions

Status: ideas from Reuben, 2026-11-17, recorded so they aren't lost. These
are not commitments, and none of them is built until he says so. The
current milestone is [Relay M1: Reliable Trusted-Device Delivery](relay-reliable-delivery.md).

These ideas are now organised around one network in
[relay-network-architecture.md](relay-network-architecture.md) (2026-11-21).

Every direction here is a hypothesis, not a commitment: Relay has no
fixed final product category
([open-ended design principles](relay-open-ended-design-principles.md)).

A larger idea from the same day, Relay as open social infrastructure
(Spaces, portable Nostr identity, independent hosting), is recorded in
[relay-social.md](relay-social.md), parked in the same way.

The vision is a personal network operating system: devices, people,
services and eventually agents that find one another, exchange
information and do useful work together. All of it builds on M1's
dependable delivery between trusted devices.

## Strategic milestones

These are capability milestones, not a fixed order. Some M3 and M4 work
can go ahead independently once M2 is in place.

| Milestone | What |
|---|---|
| M1 Reliable Delivery | Trusted devices, duplicate handling, the outbox, device messaging (current) |
| M2 Personal Network | Discovery, presence, automatic connection choice (LAN, direct internet, relay), internet reach, reconnection |
| M3 Universal Exchange | Clipboard sync and history, folder and photo sync, browser and application handoff, transfer rules |
| M4 People and Rooms | People who own several devices, person-to-person and multi-device conversations, groups, temporary rooms, shared inboxes, presence sharing |
| M5 Capabilities and Extensions | Capability registry, per-capability permissions, plugin SDK, events and automation, a local API |
| M6 Distributed Intelligence | AI discovery, a model registry, remote inference, task routing, agents that take tasks through Relay, human approval for consequential actions |
| M7 Private Service Network | Service discovery and private routing, remote desktop and terminal, remote storage, backups, compute services |
| M8 Open Network Ecosystem | Interoperability, external participants, decentralised identity, federation, community extensions |

Other ideas from the same note:
- M2: device groups, network diagnostics that explain why a device can't
  be reached, and temporary trust for a limited time.
- M3: large-transfer optimisation (chunking, deduplication, compression).

## Five ideas to protect

1. **Universal Send.** One destination for anything you want to move:
   files, text, links, commands, tasks or application state, all going
   into the same delivery system.
2. **Devices as a collective.** Your devices are a coordinated network
   with shared capabilities and identity, not isolated endpoints.
3. **Anything can trigger anything.** An incoming file can start a
   workflow, a device appearing can start a task, and an agent can ask
   for an authorised capability.
4. **Distributed intelligence.** Your devices together provide the
   intelligence, rather than one expensive machine or a cloud provider.
5. **Permission-based everything.** Trust is not one yes-or-no switch.
   People grant specific, revocable capabilities to people, devices,
   applications and agents.

## The capability model

This is the architectural idea to introduce fairly early. A device has an
identity and connects to other identities. Those identities expose
capabilities (`files.receive`, `clipboard.write`, `ai.inference` and so
on), and permissions decide who may invoke each one. One delivery layer
carries requests and results, whatever the transport. Files, messaging,
clipboard, remote terminal, AI and plugins then share one foundation,
instead of each inventing its own authentication, networking and
delivery.

M1 prepares for this by keeping trust, delivery and conversation separate
([relay-reliable-delivery.md](relay-reliable-delivery.md#three-separate-concerns)).
Today's "may send here" grant (`receives_from`) is the first per-device
permission. Capabilities would grow from it.

## Later ideas (parked)

Recorded the same evening, after this page. None is started; the
direction after M2 is chosen once M1 and M2 are done (Reuben, 21:48).

- [Relay as a personal computing network](relay-computing.md): resources,
  capabilities and remote execution, with a proposed new M3 to M9.
- [Relay Foundational Computing Study](relay-foundations-study.md): a
  research programme (R0 to R7) on the abstractions underneath, and above
  it the wider question of what the next computing environment should be.
