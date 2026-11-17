# Relay M1: Reliable Trusted-Device Delivery

Status: agreed with Reuben on 2026-11-17. This is Relay's current
milestone. It supersedes the order of the remaining PRs in
[relay-m1-m2-brief.md](relay-m1-m2-brief.md) (PR 3 onwards) and in
[relay-chat-merge.md](relay-chat-merge.md) (steps 2 and 3). The earlier
word-code work, called M1 in the brief, passed on an iPhone on 2026-11-17
and is the base this milestone builds on.

**Objective.** A person can send a file or a message to a trusted device,
even when that device is unavailable, and trust that it arrives correctly
once the device reconnects.

The next milestone, M2 Personal Device Network (discovery, presence,
connection choice, internet reach, reconnection), waits for this one.
Ideas beyond that are in [relay-future-directions.md](relay-future-directions.md).

## Three separate concerns

- **Trust**: who may talk to this device (`rust/src/trust.rs`).
- **Delivery**: what is waiting, what has been sent, and what needs a
  retry (the outbox, [relay-delivery-outbox.md](relay-delivery-outbox.md)).
- **Conversation**: the history and state of messages that a person sees.

Files and messages share the delivery machinery but keep their own
lifecycles. No concern reaches into another's storage.

## Four PRs, in order

| Stage | Work | Outcome |
|---|---|---|
| 1 | Trusted devices, completed | Listening and sending at the same time is reliable; the pairing policies work as configured; revocation is verified on a phone |
| 2 | Duplicate receive handling | Retries and repeated transfers don't leave duplicate files, and Relay doesn't keep a second copy of every received file |
| 3 | Persistent delivery outbox | Files and messages wait while a device is away, and go when it reappears |
| 4 | Device conversations | Each trusted device has its own conversation in Chats, on the existing Chat engine, delivered through the outbox |

## Acceptance criteria

**1. Trust and the device lifecycle**
- Two devices can listen and send at the same time.
- The pairing policies work as configured.
- Revoking a device stops any further communication with it.
- Revocation is tested on physical devices.

**2. File integrity**
- Retrying a transfer doesn't create a duplicate received file.
- Different files with the same name are never discarded by mistake.
- Interrupted transfers recover safely.
- A received file is verified before it counts as complete.
- Relay's blob store keeps no second copy once a receive has ended. Every
  owner gets one named tag (`relay/delivery/<id>`, `relay/receive/<id>`),
  never one tag per blob, and the tag is deleted only when that item
  reaches its end state. On the iPhone, a 200 MB receive adds about
  200 MB to Relay's storage, not 400 MB.

**3. Delivery outbox**
- Queued files and messages survive app restarts.
- An offline device gets its queued items when it reappears.
- Retries don't deliver anything twice.
- The person sees pending, sending, delivered and failed.
- Messages and files have separate delivery state machines on shared
  queue infrastructure.

**4. Messaging**
- Each trusted device has its own conversation.
- The existing Chat engine is reused.
- Messages can be sent to a device that is unavailable.
- After reconnecting, messages arrive in the right order.
- Retrying a delivery never duplicates a message.

## Out of scope

- Delivery in the background while Relay is closed. Foreground delivery
  must be dependable: Relay opens, sees a trusted device again, and
  drains its queue.
- Public or internet-wide device discovery.
- Temporary rooms and group messaging.
- Remote terminal, clipboard and AI-agent capabilities.
- Relay's own public relay infrastructure.

## The final demonstration

The milestone is complete when this whole journey passes:

1. Pair an iPhone and a Linux laptop.
2. Take the laptop offline.
3. Send a message and several files from the iPhone.
4. Close Relay on the iPhone and open it again.
5. Bring the laptop back online and open Relay on it.
6. Check that everything arrives once, in order, with the files intact.
7. Revoke the iPhone, and check that it can no longer send to the laptop.
