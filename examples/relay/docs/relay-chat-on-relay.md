# N4: One messaging security model

Status: plan, 2026-11-22. This is stage N4 in the
[network architecture](relay-network-architecture.md). Reuben asked for
it to be built at the same time as [N3, the mailbox](relay-mailbox.md).
Nothing here is built until he has read this plan.

## The goal

Every conversation in Chats runs on the same security model: Relay's own
identity (each device's key), Relay's encrypted transport (iroh), and
Relay's trust and rights. That covers your devices, the people you trust,
and rooms with several people. There are no passwords, bearer tokens or
plain HTTP, and there's no server address to type.

## What exists today

Chats has two kinds of conversation with two security models:

| | Device conversations | Chat-server conversations |
|---|---|---|
| Identity | The device's endpoint key, in the Keychain | An account on the server: a random bearer token, in the Keychain |
| Transport | iroh: QUIC, encrypted and authenticated end to end | Plain HTTP on port 8787, unless someone puts a TLS proxy in front |
| Who can read it | Only the two devices | The two devices, the server, and anyone on the path (no TLS) |
| Who may send | Trusted devices, by the trust rules | Anyone holding the token |
| Offline | The outbox, and the mailbox from N3 | The server stores and forwards |
| Joining | Pair once with a word code | Type a URL into "Join a Chat Server" |

The chat server is the weak half. Its token travels in the clear, the
server reads everything, and its join form is the one that confused
Reuben during the N2 run, when he typed a relay address into it. N2's
upgrade notice already says the chat server's path is plain HTTP until
this stage.

## The plan

### 1. People are trusted devices with fewer rights

A conversation with a person is a conversation with their device, paired
once with a word code like your own devices. The trust record already
holds rights per device. A person's device gets messages and files.
Introducing devices and seeing your device list stay off, as they are by
default now. Chats shows the person's name rather than their device's
name.

This is mostly naming and one setting, "This is a person" or "This is one
of my devices", set on the trusted device's screen (Whose Device) or with
`relay person <device> NAME` (`--mine` to undo). The delivery, the outbox
and N3's mailbox already work for it. Someone with several devices of
their own is N7's question (a person's identity across devices), not
this stage's.

### 2. Rooms move onto iroh

Rooms with several people still need a host, a device that keeps the room
and passes messages on. That is the chat server's real job, so it stays,
but on Relay's model:

- **The host is an iroh endpoint.** The chat server speaks a new protocol,
  `relay/room/1`, over iroh. It's dialled by key, through the relay if
  need be, like any device. The host can be the laptop, the Hetzner box,
  or later any device.
- **Your account is your key.** The iroh connection proves who is
  connecting, so the host knows you by your endpoint id plus a display
  name. There are no tokens and no `POST /accounts`.
- **Joining is by code.** The host shows a word code (`relay room invite`
  on its command line). You enter it under Chats → Join a Room, and it
  pairs you with the host like a device pairing, with the room's own
  rights. No URL is typed. The Settings form "Join a Chat Server" goes.
  The code is four words, works once and lasts ten minutes; the host keeps
  only its SHA-256 and limits wrong guesses. The phone finds the host
  without an address: it tries the code with the hosts it already knows,
  its trusted devices that offer rooms and the rooms entry in Relay's
  servers.
- **Same features.** The repo behind Chats keeps its interface
  (`chat_core::ChatRepo`). Only its transport changes, from HTTP and SSE
  to request/response and an event stream over `relay/room/1`: each
  iroh stream carries the same HTTP/1.1 API as before, so read cursors,
  edits, reactions, attachments and search all stay unchanged.
- **What the host can see** is said plainly, as for N2's servers: "The
  room's host passes on and keeps its messages. They're encrypted on the
  way, but the host can read them." End-to-end encrypted rooms, where the
  host can't read, are N7 (MLS). This stage stops anyone *between* you
  and the host from reading, and stops anyone without your key from
  posting as you.

### 3. Plain HTTP goes

- The chat server listens on iroh only by default. `--http` keeps the old
  API for tests, `curl` and the frozen Android example, bound to
  localhost unless asked otherwise.
- **Moving an existing account.** The order matters, so the app keeps
  working at every step:
  1. Upgrade the server, and keep running it with `--http` on the
     address the phone already uses (for the laptop,
     `--http 0.0.0.0:8787`).
  2. The new app moves its account once: over `relay/room/1`, with its
     old token (`POST /accounts/claim`), so the token is never sent in the
     clear again. The account becomes the device's key, the token is
     deleted on the server and in the Keychain, and your rooms, messages
     and read state stay, on the server. The HTTP listener from step 1 is
     only for older apps until then.
  3. `chat-server accounts` on the server lists any account still on a
     token. Once none are left, restart it without `--http`.
- **If the server is lost.** The phone keeps its own SQLite copy of each
  room. It is a cache: once a conversation passes 500 messages, the store
  drops its oldest sent messages (unsent ones are always kept). Only the
  messages it has retained stay readable, marked as read-only; anything
  it dropped was only on the server. `chat-server export` writes the full history
  out, so a server you're retiring can be kept as a file.

**Built so far (step 3, 2026-11-22).** The app's rooms go over
`relay/room/1` from Relay's own node (`rooms.rs`: the chat repo's HTTP
client, one kept iroh connection, a stream per request, the event stream
as one long answer). An account on the old HTTP API moves by itself once
Relay is open: the app asks its server where its rooms are (`GET
/host`: the host's key, relay and direct addresses), claims the account
over `relay/room/1` with its token, then forgets the token; rooms,
messages and read state stay, here and on the host. Settings, Join a Room
takes the code, a name and the host (its server address, or its key as
`chat-server invite` prints it; empty uses the current account's server).
Finding a host from the code alone (trusted devices that offer rooms, a
rooms entry in Relay's servers) is still to come; so is a room host on
Hetzner.

## How N3 and N4 are built together

They share one piece of groundwork: the trust record and the outbox. Apart
from that they don't depend on each other, so the steps interleave and
each one is a PR that is Linux-tested before the phone sees it.

| Step | N3, the mailbox | N4, one security model |
|---|---|---|
| 1 | Sealed items: `mailbox.rs`, the mailbox key, golden fixtures | People: "person or my device" on trust, names in Chats |
| 2 | The `relay-mailbox` server, with in-process tests | The chat server on `relay/room/1`: key accounts and joining by code, with in-process tests |
| 3 | The client: Hello carries the mailbox key, the outbox leaves items, collecting and receipts | The repo behind Chats on `relay/room/1`; Join a Room by code; moving token accounts; HTTP off by default |
| 4 | Running both on Hetzner (only with Reuben's go-ahead for the server) | |
| 5 | One phone run for both (about 20 minutes, Device Lab, 5G with Tailscale off) | |

The phone run covers:

- each mailbox check in [relay-mailbox.md](relay-mailbox.md);
- pairing with a second person's device, played by the laptop's spare
  "tablet" instance, as a person: it can message and send files, but
  can't see your devices;
- joining a room on the laptop's host by word code, with messages both
  ways, and a reaction and an edit arriving live;
- an old chat-server account moved over, with its rooms and history
  intact and no token left in the Keychain;
- on the laptop, a capture of the room traffic showing no readable text.

## Decisions for Reuben

These go ahead as recommended unless he says otherwise:

1. **Rooms keep a host that can read them until N7.** That is the honest
   step between today (the host and the network can read them) and MLS
   (nobody but the members can). The alternative, dropping rooms until N7,
   would lose a working feature.
2. **The host for rooms is the laptop first, then Hetzner.** The laptop
   needs nothing new. Hetzner makes rooms work when the laptop sleeps.
3. **A person is one device for now.** Several devices per person waits
   for N7.
