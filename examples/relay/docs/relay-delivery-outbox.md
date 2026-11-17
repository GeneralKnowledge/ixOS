# Relay's delivery outbox

Status: **proposed** (2026-11-17), for Reuben's review. A technical decision
that the [transfer journey](relay-transfer-journey.md) depends on: how
messages and files reach a trusted device that is away, without the person
retrying and without duplicates. It is built in brief PR 4 (Chat merge
[step 3](relay-chat-merge.md#step-3-messages-between-trusted-devices-on-chat-core)),
which now covers files as well as messages.

Inputs: the [four transfer apps](../../../docs/research/experience/references/relay-transfer-peers.md)
(#297, proposal 2: retry on presence, catch up after being apart), and
Reuben's review of it (2026-11-17): share the scheduling and the durable
queue, keep the message and file state machines separate, and answer six
questions before building.

## The decision

**One scheduler and one queue interface; two payload state machines.**

| Part | Shared? | What it is |
|---|---|---|
| **Scheduler** | Shared | Decides *when* to try a recipient: on presence, on reconnect, on return to the foreground, and on a backoff timer. One worker per recipient, never two at once |
| **Queue interface** | Shared | What the scheduler asks of any pending item: who it is for, when it is next due, and how to record an attempt. Both kinds implement it |
| **Message state** | Separate | chat-core's outbox and store, through a `PeerRepo` over `relay/device/1`. Ordering, delivered and read marks, edits and deletes |
| **File state** | Separate | A new `deliveries` table in Relay's store and the iroh-blobs store. Offer, answer, resumable transfer, verification, storage |

Each item has **one** durable home: a message lives in chat-core's store
(its `Sending` status is the queue entry, as for the chat server today), a
file delivery in Relay's `deliveries` table. The scheduler reads both through
the queue interface and keeps no copy, so the two can never disagree about
what is pending.

Why not one table for everything: messages already have a tested, durable
outbox in chat-core, with idempotent ids and relaunch recovery. A second copy
of their state would be the double bookkeeping the review warns about. Why
not two independent schedulers: both would race to connect to the same
device on the same presence event, and backoff would be counted twice.

## The scheduler

- **Triggers:** the device seen (Bonjour now; the resolver from brief PR 6),
  a `relay/device/1` connection coming up in either direction, the app
  becoming active, a new item queued, and the backoff timer.
- **Backoff:** per recipient, 1 s doubling to 5 minutes, with jitter. A
  presence or connection trigger **resets it and tries at once** (Daak's
  rule). The timer is only a fallback for presence we didn't hear about.
- **Single flight:** one worker per recipient. A trigger while it runs sets
  "run again" and is otherwise ignored, so ten presence flaps cause one
  extra pass, not ten connections.
- **One connection per recipient** carries everything due: messages first
  (small, and a big file must never hold them up), then files.
- **Both directions on connect.** When two trusted devices connect, each side
  flushes what it has for the other. That is the "catch up after being apart"
  of #297: because every item is queued durably until acknowledged, catching
  up is each side emptying its queue, with no separate sync log.
- Runs only while Relay runs. Background reach comes with brief PRs 6 and 7.

## Messages

States (chat-core's, unchanged): **Sending → Sent → Delivered → Read**, or
**Failed** with a cause.

- A message is **Sent** when the other device has written it to its store
  and acknowledged it; **Delivered** and **Read** keep the meaning they have
  with the chat server, reported back over the same connection.
- Edits, reactions and deletes are themselves messages with their own ids,
  queued behind the message they change.

## Files

States, in Relay's `deliveries` table:

```
Queued → Offered → Accepted → Transferring → Verifying → Delivered
            ↓          (Ask before receiving only)  ↓
         Declined / Expired          Paused (lost connection, iOS suspended)
                                     → back to Transferring on the next pass
Any state before Delivered → Cancelled (either side) or Failed (cause)
```

- **Queued:** the items are imported into the blob store when queued. On
  iPhone they are **copied** in (a picked file's security-scoped access ends;
  `ForgePickedFile` already copies). On computers they are **referenced**
  where they lie, and checked against their hash when sent.
- **Offered:** the root hash and listing go to the recipient. A trusted
  device that receives without asking accepts at once; one with Ask before
  receiving shows the offer card and the item waits for the answer.
- **Transferring:** iroh-blobs fetches what the recipient doesn't have yet,
  so every pass resumes from the verified bytes already there.
- **Verifying:** BLAKE3 over the collection; then the files are moved into
  `Documents/Relay` and the recipient acknowledges.

## Reuben's six questions

### 1. What happens when a device reconnects several times?

Each reconnect is one trigger; the single-flight worker folds triggers that
arrive while it runs into one more pass. A file resumes from the recipient's
verified ranges, so at most the bytes in flight when the link dropped are
sent twice, never the file. A reconnect does not count as a failed attempt,
so it never pushes the backoff out.

### 2. How are duplicate deliveries prevented?

- Every item has an id chosen by the sender when it is queued (128 random
  bits), and keeps it through every retry and relaunch.
- The recipient writes the item **then** acknowledges. If the
  acknowledgement is lost, the sender retries; the recipient finds the id it
  already has, stores nothing, and acknowledges again.
- Messages: chat-core's store already rejects a repeated id (the chat server
  does the same today). Files: the recipient keeps `(sender, delivery id,
  root hash)` for delivered files.
- The recipient keeps those ids for **longer than any sender keeps a pending
  item** (45 days against 30), so no retry can outlive the memory of it.

### 3. What happens when a message or file is deleted while offline?

- **Before it is sent:** the item leaves the queue and is never sent. Nothing
  reaches the other device.
- **While it is on the wire:** it may land. A message is then followed by its
  delete, queued like any change; a file transfer is cancelled, and the
  recipient discards the partial bytes and shows "Cancelled by iPhone".
- **After delivery:** a message delete is queued as a change ("deleted for
  both", as Chat does today); a delivered file is the recipient's, and
  deleting it on the sender changes nothing there.

### 4. How long are pending items kept?

- **30 days.** Then the item ends as "Not sent: MacBook wasn't seen for 30
  days", with Send Again. The person can cancel it sooner.
- **Storage:** queued files copied into the store count against the phone's
  space. Queuing one that would leave less than 1 GB free is refused with the
  space it needs. Activity shows how much is waiting.
- A paused incoming file's partial bytes are kept as long as its sender keeps
  the item, and dropped when it ends.

### 5. What happens when a file is no longer available?

- **iPhone:** it can't happen after queuing, because the bytes were copied
  in. Before that, the picker's error is shown.
- **Computer:** the referenced file is checked against its hash on each pass.
  Moved or deleted: "Trip.mov is no longer on this Mac". Changed: "Trip.mov
  changed since you sent it". Either way the item fails, with Send Again,
  which takes the file as it is now.

### 6. Is ordering guaranteed per recipient, per conversation or across everything?

- **Messages: in order within a conversation.** A device conversation is one
  sender-to-recipient stream: the recipient shows them by the sender's
  sequence, and the scheduler sends a recipient's messages oldest first, one
  at a time. Changes (edits, deletes) always follow the message they change.
- **Files: in queue order per recipient, but never ahead of or behind
  messages.** A message sent after a 2 GB file arrives first.
- **Nothing across recipients.** Two devices progress independently.
- **Revoked recipient:** its pending items end as "No longer trusted", and the
  recipient refuses anything it still receives from a revoked sender.

## Tests

Two Relay cores in one process on Linux, with a fake presence source and a
fake clock, as the chat server tests do today. One test per behaviour:

| Test | Proves |
|---|---|
| Laptop off, three messages and a file queued, laptop on | All four arrive once, messages in order, before the file finishes |
| Acknowledgement dropped after the recipient stored | One copy on the recipient; the sender ends as Sent |
| 20 presence flaps during a transfer | One worker; the file's bytes are sent once, apart from what was in flight |
| Link cut at 50%, restored | Resumes from the verified bytes; no tap |
| Both cores killed mid-transfer and restarted | The queue and partial bytes survive; the transfer finishes |
| Message deleted while queued | Never sent |
| Message deleted while in flight | Lands, then is deleted on both sides |
| File cancelled while queued, and mid-transfer | Never offered; recipient discards partial bytes and says who cancelled |
| 31 days pass on the fake clock | Item ends as Not sent, with Send Again |
| Referenced file moved, and changed | Fails with the right cause |
| Recipient revoked with items pending | Items end as No longer trusted; nothing more crosses |
| Ask before receiving, unanswered | Item waits as Offered; sender shows Waiting for MacBook to accept |
| Queue refused when it would leave under 1 GB free | Refused with the space needed |
| Golden fixtures | Each `relay/device/1` message decodes from committed bytes (#297 proposal 5) |

The Lean model of delivery in the [platform plan](relay-platform-plan.md)
(each message stored once, its final state reported once) is extended to
files: each delivery is written once on the recipient, and ends in exactly
one of Delivered, Declined, Expired, Cancelled or Failed.

## Order of work

1. **Spike** (already step 3's first task): a `PeerRepo` between two cores
   through chat-core's outbox, now with the scheduler's presence trigger. If
   chat-core's outbox can't take an outside trigger without changes beyond
   a wake-up call, say so before building further.
2. Messages to trusted devices on the scheduler.
3. Files on the same scheduler, with the `deliveries` table.
4. The tests above, in the same PRs as the code they cover.
