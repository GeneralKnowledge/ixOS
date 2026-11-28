# Group chats

Status: design, 2026-11-25; delivery decided (fan-out now, MLS later). Reuben asked for group chats in Relay
("Relay group chats should be added"). This is the first part of stage N7
in the [network architecture](relay-network-architecture.md) (groups and
people). It goes ahead as recommended below unless he says otherwise; the
choices that are his are listed at the end.

## What a group chat is

A conversation in Chats with three or more people or devices, where:

- only the members' devices can read it: no host, no server, and no
  mailbox can;
- a member who is removed, or leaves, gets nothing sent after that;
- it works with the apps open, like every Relay conversation (delivery is
  foreground only; waking a closed app needs a paid Apple account);
- a member who is away gets what they missed when they come back, from
  the senders' outboxes or their own mailbox.

Rooms already give Chats conversations with several people, but a room is
kept by a host that can read it ([N4](relay-chat-on-relay.md)). Groups are
the private kind; rooms stay for large or open conversations that a host
keeps.

## The choices

### 1. Delivery: one copy to each member, over the connections Relay has

A message to a group is sent once to each other member, on
`relay/device/1`, through Relay's outbox, exactly as a message to one
trusted device is sent today. Each copy is end-to-end encrypted by the
connection it travels on (iroh's QUIC with TLS 1.3, between the two
devices' keys), or sealed to the member's mailbox key (HPKE) when it waits
in their mailbox.

This needs no new cryptography and no new transport, and it already has
what groups need:

- **Removal needs no rekeying.** There is no shared group key to rotate: a
  removed member is no longer sent to. The admin stops at once; each other
  member stops as soon as it has the admin's update, so a copy sent in
  that short gap can still reach the removed member.
- **Offline and retries** are the outbox's: per-recipient order, exactly
  once on the receiver by message id, the mailbox while a member is away.
- **Delivery status per member** falls out of it ("Delivered to 3 of 4").

The cost is one send per member, which is fine for the groups Relay is for
(family, friends, a team, your own devices). Groups are capped at
**32 members** while delivery works this way.

**Why not MLS now.** The architecture names MLS (OpenMLS) for group
encryption. MLS earns its complexity when one encrypted copy goes to a
server that fans it out, for large groups. Relay has no such server for
private groups, and MLS brings what Relay would then have to run: commits
that must be put in one order by a delivery service, an epoch state on
every device that must be kept and migrated, and recovery when a device
misses a commit. Fan-out over pairwise connections invents no group
cryptography either; it reuses the per-device security Relay already has.
MLS comes in when a group outgrows fan-out, or when host-kept rooms should
stop the host reading them, and then it plugs in under the same group
record and Chats screens.

### 2. Group identity and membership

A group is a record in Relay's database (`relay.db`, a `groups` table),
the same on every member's device:

| Field | What |
|---|---|
| `id` | 16 random bytes, chosen by the creator; the conversation is `group:<hex>` |
| `name` | Shown in Chats; the admin can rename it |
| `admin` | The endpoint id of the device that created it |
| `members` | Endpoint id and display name of each member, the admin included |
| `version` | Raised by one on every change; a member keeps the highest version it was sent |

Only the admin changes the group: it sends the whole record, as
`GroupUpdate`, to every member (old and new) whenever it adds, removes or
renames. A member accepts an update only from the admin, and only with a
higher version than it has. A member can always leave: it tells everyone,
and the admin's next update leaves it out.

**Who can be added.** The admin picks members from its own trusted people
and devices, so every member was paired with the admin by word code.
Members who never paired with each other still talk inside the group,
because the admin they trust introduced them: a device accepts a group
message from a sender that is in that group's member list, and only for
that group. That doesn't make them trusted devices: they can't send files
outside the group, message you one to one, ask your AI or see your
devices. Chats shows such a member's name with "via <admin's name>".

Revoking or forgetting a device stops it in every group too: you refuse
its messages, and if you are the admin you remove it.

### 3. On the wire

Two new messages on `relay/device/1`, in the same CBOR as the rest, with
fixtures:

- `GroupMessage { group, id, body, reply_to }`: a message in a group, by
  the id chat-core gave it, kept through retries.
- `GroupUpdate { group, version, name, admin, members }`: the group as the
  admin now has it.

A device offers `"groups"` in its `Status`. Devices without it are older
versions: the admin can't add them until they update.

Each member orders the conversation by when messages reached it, as device
conversations do. Two members can see two messages sent at the same moment
in a different order. Copies that came straight and copies that waited in
a mailbox can interleave, so even one sender's messages are not promised
in order.

A message can arrive before the update that tells the device about its
group or its sender (the update went by mailbox, the message straight).
It is held, at most 200 for a week, and shown once the admin's update
makes the sender a member; one that no update explains is dropped unseen
when it expires. Updates can overtake each other too (one waits in a
mailbox, the next comes straight): an older update that arrives late still
explains a message held before this device took the newer one, so a
member's message sent before their removal isn't lost.

**How long a copy waits.** A member away for long doesn't get everything:
the sender's outbox gives up on a copy after 30 days, and a copy in the
member's mailbox expires after 7. Such a copy counts as not delivered.

### 4. How it fits Chats

- A group is a conversation on chat-core's engine, like a device
  conversation: a `GroupRepo` beside `PeerRepo`, with Relay's database as
  the truth and chat-core's store as its cache. Search, unread counts, the
  New Messages pill and replies work unchanged.
- **New Group** in Chats' + menu: a name and members picked from your
  trusted people and devices.
- **The group's screen** (tap its title): members and whether each is
  reachable; Add Members, Remove and Rename for the admin; Leave Group for
  everyone.
- Each message shows its sender's name. Under your own: "Delivered to 3 of
  4" until all have it.
- On the command line: `relay group create NAME DEVICE...`,
  `relay group add|remove GROUP DEVICE`, `relay group leave GROUP`,
  `relay group send GROUP TEXT`, `relay groups`.

## The steps

Each step is a PR, Linux-tested before the phone sees it.

1. This design.
2. The core in Rust: the groups table, the messages (`GroupMessage`,
   `GroupUpdate`, and `GroupLeave` for a member leaving), fan-out through
   the outbox and the mailbox, the receiving rules, `GroupRepo` and the
   CLI. Members must be trusted devices of the sender in this step;
   members introduced by the admin come with step 4.
   Tested with three or four in-process devices: a message reaches every
   member, a removed member gets nothing new, a non-member is refused, an
   update not from the admin is refused, a member away gets it on return.
3. The iPhone: the FFI, New Group, the group's screen (Group Info),
   sender names, delivery counts. Edits, reactions and taking back a
   delivered message aren't offered in groups, as in device
   conversations.
4. Members introduced by the admin. This needs each member's mailbox key
   passed on by the admin inside its group update, which arrives over the admin's authenticated connection or sealed mailbox (so a copy can be
   sealed to a member the sender never paired with), and acceptance scoped
   to the group: a group-only right on
   `relay/device/1`, dialled by key, deposit passes for their mailbox),
   and photos and files in groups (one blob, offered to each member).
5. One phone run: the phone and two laptop instances ("create" and
   "tablet") in a group, messages all ways, one member offline then back,
   a member removed.

## Decisions for Reuben

These go ahead as recommended unless he says otherwise:

1. **Fan-out now, MLS later.** Decided by Reuben, 2026-11-25.
2. **Members who haven't paired with each other can talk inside the
   group, introduced by the admin** (recommended), or every member must
   have paired with every other first.
3. **Only the admin adds, removes and renames** (recommended), or any
   member can.
