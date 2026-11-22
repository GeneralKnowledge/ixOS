# N3: The encrypted mailbox

Status: plan, 2026-11-22. This is stage N3 in the
[network architecture](relay-network-architecture.md), started after N2
passed on the phone on 2026-11-22. Its requirements come from the review's
[refinement 3](relay-review-2026-11.md): the mailbox is an untrusted
courier, not a participant. It is built together with
[N4, one messaging security model](relay-chat-on-relay.md); the steps
interleave as that plan's table shows. Nothing here is built until Reuben
has read this plan.

## The goal

You send something to one of your devices or a trusted person, and it
arrives even when that device is closed, asleep or offline at the moment
you send. You don't have to keep Relay open until the other side turns up.

Today both sides have to be running at once. The outbox keeps an item and
retries for 30 days, but only while the *sender* runs Relay. The iPhone
runs Relay only in the foreground, so a photo sent from the phone to a
sleeping laptop waits until both are awake together. The other way round,
the laptop can't hand anything to a phone in a pocket.

The mailbox closes that gap. When a direct route fails, the sender leaves
the item, encrypted, with Relay's servers. The recipient collects it the
next time it runs.

## What the mailbox can and can't see

This is said in the same plain way as N2's server claims, and it goes in
the app's Connections footer.

- **It can't read anything.** Each item is encrypted to the recipient
  device before it leaves the sender. The server holds ciphertext it has
  no key for.
- **It can't forge or alter anything.** The sender signs each item inside
  the encryption. The recipient checks that the signer is a device it
  trusts, and drops anything else.
- **It does see** which device left something for which device, when, and
  roughly how big it was. That is the same metadata the relay already
  sees for a live connection.
- **If it's down or compromised,** delivery falls back to the direct
  routes, as today. A compromised server could delete or withhold items,
  but it couldn't read or fake them. The sender's outbox still holds
  each item until the recipient confirms it, so a lost item is sent again.

## Design

### The mailbox is an iroh endpoint

The mailbox server is a small Rust program, `relay-mailbox`. It runs on
the Hetzner box next to `iroh-relay` and the lookup server, and speaks a
new protocol, `relay/mailbox/1`, over iroh. That gives three things for
free:

- **Authentication.** An iroh connection proves the dialler's endpoint
  key, so a device collects only items addressed to its own endpoint id.
  No accounts, tokens or passwords are needed.
- **Reachability.** It is dialled like any device, by key, and through the
  relay if need be. No new port or Caddy route is needed.
- **One identity.** Devices already have keys, so nothing new is set up.

Its address is one more entry in Relay's servers. "Relay's Servers" adds
it, "No Servers" and the Private and This Network Only modes never use
it, and Your Own Servers gains an optional Mailbox field (the CLI's
`relay servers --mailbox`).

### A sealed item

```
SealedItem {
  version: 1,
  id:        the outbox delivery id (16 bytes, the same one used direct),
  to:        recipient endpoint id,
  from:      sender endpoint id,
  expires:   unix ms,
  sealed:    HPKE(recipient mailbox key, signed(payload, by sender's endpoint key)),
}
```

- **The mailbox key.** Each device makes a separate X25519 key for the
  mailbox. It sends that key, signed by its endpoint key, to trusted
  devices in the Hello they already exchange. The server never sees it,
  so only trusted devices can seal items the device will open. Keeping it
  separate from the endpoint key avoids reusing one key for two kinds of
  cryptography, and lets it be rotated without re-pairing.
- **Encryption.** HPKE (RFC 9180) base mode, X25519 with ChaCha20-Poly1305.
  The outer `id`, `to`, `from` and `expires` are bound in as associated
  data, so the server can't move an item to someone else or stretch its
  life.
- **The payload** is the outbox's `Payload` unchanged (a message, a note,
  or files). Files go in as their bytes, sealed the same way and streamed
  in chunks, with iroh-blobs' BLAKE3 hash checked after opening.
- **Pure module.** Sealing and opening live in `rust/src/mailbox.rs` as
  pure functions with golden fixtures, like trust and the device list.

### Server rules

| Rule | Value (recommended) |
|---|---|
| Kept for | 7 days, then deleted unread. The sender's outbox keeps the item and leaves it again or waits for a direct route (see Sending) |
| One item | Up to 100 MB. Bigger files wait for a direct route, as today |
| Per recipient | 1 GB held at once, shared out per deposit pass (below), so one sender can use at most a quarter of it. A full mailbox refuses new items, and the sender keeps them |
| Per deposit pass | 200 items an hour, and at most 256 MB held at once |
| Duplicates | The same (`from`, `id`) is stored once |
| Receipts | Small fixed-size records, one per (`from`, `id`), kept outside the per-recipient quota so a full mailbox still takes them |
| Deleting | Only when the recipient acknowledges the item, or when it expires |
| Storage | SQLite for the index, one file per sealed item, nothing else |

### Who may leave items: deposit passes

Only devices the recipient trusts can leave anything. Otherwise anyone
with an iroh key could fill a closed device's mailbox and crowd out the
devices that matter.

- For each device it trusts, the recipient makes a random 32-byte
  **deposit pass** and sends it to that device in their Hello, over the
  connection that is already encrypted. It registers only the pass's
  SHA-256 with the mailbox.
- Passes are scoped to their recipient. Only the recipient can register
  or deregister its passes: the server takes the recipient from the iroh
  connection, never from the request. A deposit must carry a pass
  registered by the device in the item's `to`, or the server refuses it,
  so a pass can't be used for anyone else. The
  quotas above apply per pass, so one trusted device, even a misbehaving
  one, can't use up the whole mailbox.
- Revoking a device deregisters its pass at once, and forgetting a device
  does the same. A device that rotates its mailbox key rotates its passes
  too.
- The server never learns the trust graph in advance: a pass is just a
  hash until someone uses it. It learns which pairs actually send, as the
  relay does.
- The recipient still opens only what a trusted device signed, and drops
  the rest. The pass is about storage; the signature is about identity.

### Sending

The outbox stays the one queue (review refinement 2). The scheduler gains
one step:

1. Try the direct routes as now.
2. If the device can't be reached, and the item is within the size limit
   and Relay's mailbox is in use, seal it and leave it in the mailbox. In
   the conversation the item shows **Left for <device>**, and in history
   **In the mailbox**.
3. The item stays in the outbox until the recipient confirms it, either
   when it next connects directly or with a receipt it leaves in the
   sender's own mailbox. Then it is Sent or Done, as today.
   - If no confirmation arrives within the mailbox's 7 days, the sender
     leaves the item again, up to the outbox's 30 days. That covers
     both an item that expired unread and a receipt that was lost or
     expired after the item was collected. A recipient that already
     has the id (its 45-day memory) doesn't save it again: it leaves a
     fresh receipt and acknowledges. An item with no
     confirmation after 30 days ends **Not confirmed**, not "Not
     delivered": the sender can't tell a lost item from a lost receipt,
     and the label says only what it knows.
4. A direct route that comes back first still delivers. The recipient's
   45-day memory of ids means the copy in the mailbox is then not saved
   again: the recipient leaves a fresh receipt and acknowledges it, so
   only one is ever kept and the server deletes the copy.

### Collecting

- **iPhone:** when Relay opens or comes to the foreground, and then every
  30 seconds while it's open. There is no background fetch or push for
  now: push needs APNs, which needs a paid Apple team.
- **Laptop (`relay listen`):** every 30 seconds, and straight away after a
  network change.
- For each item, in this order:
  1. open it and check the signer is trusted;
  2. save it through the same path as a direct delivery (exactly once by
     id);
  3. leave a small sealed receipt in the sender's mailbox;
  4. only then acknowledge it, so the server deletes it.

  The receipt always goes before the item is deleted. If the recipient
  stops between any two steps, the item is still there next time; its id
  is in the 45-day memory, so it isn't saved again, and the receipt is
  left again (the server stores one per id) before the acknowledgement.
- A revoked sender's items are dropped and acknowledged without being
  saved. A device that has been revoked no longer has the recipient's
  current mailbox key once that key rotates on revoke, so it can't seal
  anything new to it.

## Steps

Each step is a PR, Linux-tested before the phone sees it.

1. **Sealed items.** `mailbox.rs`: sealing and opening, the mailbox key
   and its signed announcement, golden fixtures. The tests cover wrong
   recipient, wrong signer, tampered metadata, expiry and replay. Nothing
   is on the wire yet.
2. **The server.** The `relay-mailbox` binary: `relay/mailbox/1`,
   deposit, list, fetch, acknowledge, expiry, quotas and duplicates, with
   in-process tests (two endpoints and a server in one test). The tests
   include a device registering a pass for someone else (refused), a
   pass used for another recipient (refused), and an acknowledgement
   from anyone but the recipient (refused).
3. **The client.** The mailbox key goes in the Hello, the outbox gains its
   new step, and collecting and receipts are added. The CLI gets
   `relay status` lines and `relay servers --mailbox`. An end-to-end test
   covers a sender, a closed recipient and the server, then the recipient
   opening.
4. **Running it.** Deploy `relay-mailbox` on Hetzner as a systemd service,
   only with Reuben's go-ahead for the server. Then Relay's Servers
   includes it.
5. **The phone run** (about 15 minutes, Device Lab, 5G with Tailscale
   off):
   - stop the laptop's listener, send a message and a photo from the
     phone, and close Relay; start the listener, and both arrive once;
   - close Relay on the phone and send from the laptop; open Relay, and
     both arrive once;
   - an item older than its expiry is gone, and the sender is told it's
     waiting again;
   - with the mailbox server stopped, delivery still works directly
     (Wi-Fi), and the outbox keeps the rest;
   - on the server, the stored files are unreadable ciphertext.

## What N3 doesn't do

- No push notifications or background fetch on iOS. That needs a paid
  team, and it's a later stage.
- No mailbox for chat-server conversations. Those move onto Relay in N4,
  and then get the mailbox for free.
- No groups. N7 (MLS) will need its own fan-out, but sealed items to each
  member are enough until then.
- No change to word codes, which stay live-only.

## Decisions for Reuben

These go ahead as recommended unless he says otherwise:

1. **Who may leave items:** only trusted devices, through deposit passes
   (above). Each pass has its own quota, so no one sender can fill a
   mailbox.
2. **How long items are kept:** 7 days.
3. **Biggest item through the mailbox:** 100 MB, so photos and short
   videos fit and larger files still go direct.
