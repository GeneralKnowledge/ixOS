# Relay: unified delivery (review step 4, P2)

Status: plan, 2026-11-19. Consolidation only: this removes a concept (the
note) and a queue (chat-core's outbox for device conversations); it adds no
feature. The review it follows is
[relay-review-2026-11.md](relay-review-2026-11.md) (section 2, refinement 2).
SQLite (P3) comes after it and keeps the shape described here.

## The problem today

There are two ways to send a trusted device text and two queues behind them:

| | Sent from | Queue | Shown in |
|---|---|---|---|
| Note | Devices → device → Write a Note, `relay send <device> --note` | Relay's outbox (`Record::deliveries`) | History, and the device's "Between You" |
| Message | Chats → device conversation | chat-core's outbox, woken by Relay's scheduler | Chats |
| Files | Devices → device → Send Files, `relay send <device> PATH...` | Relay's outbox | History, and "Between You" |

So the same text can go two ways and appear in two places, and items to one
device sit in two queues that don't know each other's order.

## The decisions

**1. A note is a message.** Text to a trusted device is always a message in
that device's conversation. `DeviceMsg::Note` is no longer sent; one that
arrives from an older build is added to the conversation as a message (the
only devices paired today are Reuben's, all updated by the combined build,
so this is a one-release courtesy). `send_note_to_device` and
`relay send <device> --note` post a message.

**2. Files sent to a trusted device are an attachment in its
conversation.** chat-core already has attachments (`Attachment`: id, name,
type, size) and Chats already shows photos, documents and voice notes.
Sending files to a device posts a message with those attachments; the bytes
still travel by the outbox and iroh-blobs, as today, and the receiver saves
them in Documents/Relay as today. A folder is one attachment (a collection),
shown as a folder that opens in the Files app. Both sides use one message
id, `file-<delivery id>`; a file's attachment id is the SHA-256 of its
bytes, a folder's is derived from the delivery, with type
`inode/directory`. The conversation's log keeps where each attachment is on
this device (the saved file on the receiver, the file that was sent on the
sender, which may be gone later), `RelayCore::device_file` returns it, and
`PeerRepo::download_attachment` copies it for chat-core, which checks it
against the id. Files are sent from the device's Send Files; attaching
files to a message in Chats (`upload_attachment`) is left for later, so
chat-core's 25 MB attachment limit never applies to device files.

**3. One durable queue: Relay's outbox.** Every item for a trusted device,
message or files, is a `SavedDelivery` in `Record::deliveries`, saved
before anything is dialled, and one worker per device sends them oldest
first, so messages and files to one device keep the order they were sent
in. chat-core's outbox only hands a device message over: `PeerRepo`'s
`post_message` queues the delivery and returns the message as Sending, which
is local and never fails for a network reason. The scheduler, backoff,
30-day expiry, cancellation and the exactly-once rule (save, remember the
id for 45 days, then answer) are the outbox's, unchanged.

**4. Two lifecycles on that queue, not one state machine.** They share
scheduling, retry, identity and persistence, and keep their own states:

| Messages | Files |
|---|---|
| queued: saved in the outbox | queued: saved, bytes in the blob store |
| sent: on the wire to the device | transferring: the device fetches what it doesn't have (a retry resumes) |
| delivered: the device saved it and answered | verified: every blob's hash checked on the device |
| | saved: the device saved the file and answered |
| failed: refused, no longer trusted, cancelled, or 30 days | failed: the same causes, or verification failed |

"Acknowledged" (the other person has seen it) is not built in this step:
it is a read receipt, a feature, and waits for its own decision. The
message lifecycle leaves room for it after delivered.

In the record each delivery keeps a `kind` with its own state enum
(`MessageState`, `FileState`), never one shared list of states. The SQLite
schema (P3) keeps the same split: one `deliveries` table for scheduling,
with the per-kind state in its own column checked against that kind's
values.

**5. History becomes activity, conversations hold what went between two
devices.** The History list keeps word-code transfers and file deliveries
(with progress), and no longer shows notes. A device's screen drops "Between
You" and "Write a Note" for **Message**, which opens its conversation; Send
Files stays and lands in the conversation.

**6. Migration, once, on the first open after the update:**

- a pending Note delivery becomes a pending message delivery with the same
  id, added to the conversation as Sending;
- a pending Files delivery stays queued and gets its attachment message;
- notes already in the history (sent and received) are copied into their
  device's conversation, in time order, with their original ids and times,
  and leave the history;
- notes to or from a device that is no longer listed stay in the history;
- received-id memory is kept as it is, so a retry from a device on the old
  build is still answered once.

The migration is tested from a saved record with each case, and running it
twice changes nothing.

## What does not change

- Word-code transfers: they are for devices that aren't trusted, and stay as
  they are.
- Trust, revocation, the resolver, presence, and Relay's own servers.
- Server conversations in Chats: they keep chat-core's outbox.
- Foreground delivery only; offline delivery waits for the mailbox (P6).

## The work, in order

1. **One queue for messages** (Rust, CLI): `PeerRepo::post_message` queues a
   message delivery; the worker sends `DeviceMsg::Message`; the log marks
   it delivered. `--note` becomes a message. Tests: messages and a file to an
   away device arrive once, in order, after a relaunch; a retry whose answer
   was lost shows once; revocation ends both kinds.
2. **Files as attachments** (Rust): the attachment message on both sides,
   `download_attachment` from the saved file or the device. Tests: a file and
   a folder appear in both conversations with the right size and open from
   the saved path; a damaged blob fails as "verification failed".
3. **Migration** (Rust): the cases above, from saved records.
4. **Screens** (Swift): the device screen's Message button, files in Chats,
   notes gone from History. Model tests on Linux; the iPhone build in the
   next combined build.
5. **Phone check** (one sitting, about 10 minutes, needs the iPhone): with
   `relay listen` stopped, send the laptop a message, a photo and a second
   message from the phone; start `relay listen`; all three arrive once, in
   order, in the laptop's conversation (`relay messages <phone>`), and the
   photo opens. Then the same from the laptop to the phone.

Each step is its own commit on one PR, with the ledger and README updated
in the step that changes what they say.
