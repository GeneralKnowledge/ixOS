# Relay's transfer journey

Status: **proposed** (2026-11-17), for Reuben's review. This is the
experience specification for sending and receiving in Relay, from first
launch to the tenth time. It is Phase 4's first product journey in the
[experience research](../../../docs/research/experience/README.md), and it
turns the research into one design instead of a list of tickets:

- the [LocalSend audit](../../../docs/research/experience/references/localsend.md) (#288);
- the [four transfer apps](../../../docs/research/experience/references/relay-transfer-peers.md) (#297), and Reuben's review of it;
- Reuben's six questions (2026-11-17): what the first screen offers, how
  discovery works, when the receiver accepts, how trust is shown, what
  happens on interruption, and how to repeat a transfer.

It changes no protocol decision: iroh, word codes with SPAKE2, pairing
policies, trusted devices and the revocation rule stay as the
[brief](relay-m1-m2-brief.md) says. The [outbox](relay-delivery-outbox.md),
which makes "send it when they're back" work, is its own technical decision.

## The job, and the two relationships

Someone opens Relay to **move something to another device**: a photo to the
laptop, a folder to the iMac, a file to a friend's phone. Relay has two
answers to "to whom", and the whole journey keeps them apart:

| | A trusted device | Anyone else |
|---|---|---|
| Who | Your own devices, and people's devices you chose to trust | A device you don't know, or one you revoked |
| How | One tap, no code | A word code, every time, for one transfer |
| When it's away | Relay keeps it and sends when the device is back | Both have to be there at once |
| Proof of who it is | Its key matches the one you trusted | Whoever holds the whole code; nothing more |

## Principles

1. **Start from the verb.** Send and Receive are where the person starts. A
   device is a choice inside Send, not the front door.
2. **Nothing lands without consent.** A transfer from someone who isn't
   trusted shows what it is before any bytes are written. If nobody answers,
   the answer is no.
3. **Trust is a key, shown one way everywhere.** A device looks trusted only
   when its key is one you trusted. Names are labels, never proof.
4. **Nobody retries by hand.** An interrupted transfer to a trusted device
   finishes by itself when the device is back. A code transfer keeps what
   already arrived.
5. **The second time is shorter than the first.** A device you sent to, and
   a transfer you made, can be repeated in one or two taps.

## The front door

Agreed in the [merge plan](relay-chat-merge.md#what-the-merged-app-is): the
tabs are **Chats, Send, Devices**, and Settings holds policies and servers.

**Relay opens on Send at first launch, then on the tab you last used.**
Someone who installs a transfer app wants to transfer. Someone who uses it
mostly for messages should land in Chats without a setting.

**The Send tab, top to bottom:**

1. **What:** Photos, Files, Folder, Text, and Paste when the clipboard holds
   something. One row of large buttons.
2. **Recent:** up to three trusted devices you sent to most recently, each
   one tap once something is chosen.
3. **Receive with a code:** a button at the bottom. It opens code entry with
   the scanner a tap away. Receiving from a trusted device needs no screen
   at all (below).

Under the buttons, "Others see you as **Reuben's iPhone**". Tap it to rename
the device: you need to know what the other person sees in their list.

**Success:** from launch, a photo is on its way to a nearby trusted device
in four taps (Send, Photos, the photo, the device), and a code is on screen
for anyone else in four taps.

## Sending

After **what**, Relay asks **who**, on one screen:

- **Your devices and trusted people**, nearest first, each with its state
  in words and a symbol for its kind (phone, laptop, desktop, server):
  - **Nearby**: on this network now. The transfer starts at once.
  - **Reachable**: found by another route (brief PRs 6 and 7). Starts at once.
  - **Away**: not seen. Choosing it still works: "Relay will send this when
    MacBook is back." The [outbox](relay-delivery-outbox.md) takes it from
    there.
- **Someone else, with a code**, always last, always there whatever the
  pairing policy.

**With a code**, the sender sees the code large (number and three words) and
its QR code, and below it what happens next: "Waiting for them to enter the
code", then "They're deciding", then progress or the answer ("Declined",
"They didn't answer"). The code changes by itself after a wrong guess or a
clash, as today, with the reason in a line under it.

**To a trusted device that asks first** (below), the sender sees "Waiting for
MacBook to accept"; the transfer stays in Activity until it is answered or
expires.

**Success:** sending to an Away device needs no more taps than to a Nearby
one, and the sender is told in words what will happen.

## Receiving

There are only two ways something can arrive, and the person always knows
which one it is:

1. **From a trusted device.** By default it arrives without asking (the
   brief's default grant, "receive files and notes from it: on"): a banner
   while Relay is open, a notification otherwise, and a row in Activity. Each
   trusted device has **Ask before receiving** in its permissions; with it on,
   its transfers show the offer card below.
2. **With a code you typed or scanned.** Always the offer card.

An unknown device can never push anything to you: the only way in without
trust is a code that you entered on this phone.

## The offer card

Shown after the code is confirmed and before any bytes are fetched, and for
trusted devices that ask first. It answers the receiver's seven questions:

| Question | What the card shows |
|---|---|
| Who is sending? | The device's name and kind |
| Is this a device I know? | "Trusted" with the trust symbol, or "Not a trusted device: connected with a code". For an untrusted device the name is shown as what it calls itself ("calls itself 'Reuben's iPhone'"), never styled as trusted |
| What exactly is it? | The item count and the first names ("Trip: 3 photos, 1 video"; a folder by its name and item count), with the full list a tap away |
| How much? | Total size, and the free space left after it ("24 MB · 51 GB free after") |
| Where will it go? | "Files › On My iPhone › Relay › Trip", with photos and videos also offered to Photos (below) |
| What does Decline do? | "Nothing is saved. They'll see that you declined." |
| Can I trust this device later? | Not on the card: trusting is offered after the transfer succeeds, so consent to one transfer never doubles as trust |

Buttons: **Receive** and **Decline**. The card waits two minutes, counting
down in words ("Waiting for your answer, about 2 minutes"); no answer is a
decline, and the sender sees "They didn't answer".

### Why the card can be believed

The card is only worth showing if a code holder can't make it lie. Each line
on it is bound to the session that proved the code:

- **Who:** the offer travels on the iroh connection whose TLS is keyed to
  both endpoint ids, after the SPAKE2 key confirmation, which already covers
  both ids (`code::confirm`). Nobody without the code can send an offer, and
  the offer can't be replayed onto another connection.
- **Trusted or not** comes from the receiver's own record: the sender's
  endpoint id is in its trusted list or it isn't. The offer can't claim it.
  For a trusted device the name shown is the one the receiver saved, not the
  one in the offer.
- **What and how much:** the offer carries the root hash of the iroh-blobs
  collection, and the list of names and sizes shown is read from that
  collection. Bytes that don't match the hash fail verification and are
  discarded, so the files can't differ from the list that was accepted. The
  receiver stops a transfer that exceeds the total it showed.
- **Names are cleaned before they are shown or saved:** no path separators,
  control characters or right-to-left overrides, a length limit, and every
  path kept inside its folder (already tested in `code.rs`, `inside_folder`).

Tests (Rust, on Linux): an offer before key confirmation is refused; an offer
on another connection is refused; a name with `../`, control characters or a
bidi override is cleaned; a collection whose listing differs from the offered
total is refused; a transfer that passes its declared size is stopped; an
untrusted sender calling itself by a trusted device's name is shown as not
trusted.

Today the receiver fetches as soon as it reads the offer. The change: it
fetches only the collection's index (names and sizes, a few kilobytes, kept
in the store and dropped on Decline), reports the offer to the app, and
fetches the files only after **Receive**. The sender
keeps the code's handshake open while the receiver decides (its silence timer
allows the two minutes).

**Success:** no file of an untrusted transfer is fetched or saved before Receive, and
an unanswered offer leaves nothing behind on either side.

## Pairing and trust

The three pairing policies stay ([brief](relay-m1-m2-brief.md#one-transfer-path)).
What the person sees:

- **After a code transfer** (policy Ask to pair, the default): "Trust
  'Reuben's laptop'? You'll be able to send to each other without a code."
  **Trust** or **Not Now**. Asked on both sides, after success only.
- **One look for trust,** used on the offer card, the device list, Activity
  and a device's own view:
  - **Trusted:** a filled shield with a check, and the word.
  - **Not trusted:** no symbol, and "Connected with a code" where it matters.
  - **Revoked:** only in your own device list, with **Trust Again**. A revoked
    device sending with a code looks like any untrusted device.
- **A device's view** shows its permissions in words: "Can send you files
  without asking", "Can ask your AI: off".
- **What nearby devices can see,** in the help sheet: "Nearby devices can see
  that you're sending, but not what. Only someone with the whole code can
  receive it." Nothing advertised on the network is itself a permission.

**Success:** after one code transfer and one tap on Trust, the next transfer
to that device needs no code, on both sides.

## Progress and failure

**Progress** (both sides): each item with its own bar and state, the total
with live speed and time left, and **Cancel**, which becomes **Done**. A
received folder or file opens in Files from its row.

**Every ending is named for its cause and says what to do:**

| Ending | Words (sender side) | What to do |
|---|---|---|
| Delivered | "Sent to MacBook" | Send Again |
| Declined | "MacBook declined" | |
| No answer | "They didn't answer" | Send Again |
| Cancelled | "You cancelled" or "MacBook cancelled" | Send Again |
| Paused | "Lost the connection. Relay will finish when MacBook is back" (trusted), "Lost the connection. 140 MB is already there: ask for a new code to finish" (code) | Nothing (trusted), a new code (code) |
| No space | "MacBook is out of space" or "Not enough space on this iPhone (needs 1.2 GB)" | |
| Didn't verify | "Something changed on the way, so it was discarded" | Send Again |
| Not trusted | "MacBook no longer trusts this device" | Send with a code |
| Source gone | "Trip.mov is no longer on this iPhone" | |

The causes come from Rust (`RelayError`'s kinds), the words from Swift.

**When the devices can't see each other:** "Can't see the other device?"
under the device list and on a failed transfer opens a short sheet: the same
Wi-Fi; Relay open on both (until background reach exists); guest and hotel
Wi-Fi may keep devices apart; Local Network allowed in Settings. When Local
Network is denied, Send says so with **Open Settings** instead of an empty
list. The camera on the scanner gets the same treatment.

**Success:** every ending in the table has a test that produces it and checks
its words.

## Recovery

- **To a trusted device:** nothing for the person to do. The outbox resumes
  when the device is seen again (Bonjour now, the resolver later), and an
  interrupted file picks up from the bytes already received.
- **With a code:** the sender already issues a new code after a drop; the
  receiver enters it and the transfer resumes from what is there ("140 MB
  already here").
- **Relay leaves the screen:** a transfer in progress asks iOS for time to
  finish (`beginBackgroundTask`). If iOS ends it first, it pauses and resumes
  when Relay comes back, without asking. Until background reach (brief PRs 6
  and 7), the sender sees "Keep Relay open on both devices" only while a
  transfer is running.
- **Relay is quit or the phone restarts:** queued and paused transfers are
  still there, in Activity, and carry on.

**Success:** a 200 MB transfer to a trusted laptop, interrupted by turning
the laptop's Wi-Fi off for a minute, finishes with no tap, and the bytes sent
before the break are not sent again.

## Repeat use

- **Send Again** on every Activity row: the same items to the same device,
  one tap (a new code for a code transfer).
- **Recent** devices on the Send tab (above).
- **Trust** after a code transfer is the path to never typing a code with
  that device again.
- **Received photos and videos:** **Save to Photos** on the received item,
  and a setting, "Save photos and videos to Photos", off by default. It needs
  add-only Photos access, which a free team allows.

**Success:** repeating the last transfer takes two taps from launch (Activity
row, Send Again).

## Out of scope here

- Receiving while Relay is closed (reach, brief PRs 6 and 7; no push).
- A share extension (another App ID on a free team; later).
- Apple's peer-to-peer Wi-Fi between iPhones (#297 proposal 4: a spike that
  needs two iPhones).
- Showing nearby code senders on Receive so the receiver picks instead of
  typing (LocalSend audit proposal 2): it would need a security review first.

## How it lands

This journey is built inside steps already planned, not as extra PRs. Each
keeps its tests and one short phone check.

| PR | What of this journey | Planned as |
|---|---|---|
| 1. Trusted devices | Pairing policies; Trust after a code transfer; the trust look; device permissions in words, including Ask before receiving; golden fixtures for `relay/device/1` (#297 proposal 5) | Brief PR 3, merge step 2 |
| 2. Delivery outbox | Messages and files to trusted devices that are away, resumed on presence ([relay-delivery-outbox.md](relay-delivery-outbox.md)) | Brief PR 4, merge step 3 (now also files) |
| 3. The front door | Send tab (what, then who, Recent, Receive with a code); the offer card and its session binding; progress and named endings; help sheet and Local Network notice; Activity with Send Again; Save to Photos | Merge step 4 with brief PR 5, the UI programme's Relay pilot |

PR 3 is the largest. If it outgrows one review, it splits along the journey:
sending and the offer card first, then Activity, repeat and Photos.

## The phone check for the journey

About 15 minutes, once PR 3 is built, iPhone and laptop:

1. First launch opens on Send; send a photo to the laptop with a code; the
   laptop shows the offer; accept; Trust on both sides.
2. Send a folder to the now trusted laptop: no code, no question.
3. Turn the laptop's Wi-Fi off mid-transfer, then on: it finishes by itself.
4. Send from the laptop with a code and decline on the phone: nothing saved.
   Again without answering for two minutes: the laptop says they didn't answer.
5. Send Again from Activity: two taps.

## Decisions for Reuben

1. **The first screen:** Send at first launch, then the last tab used
   (recommended), or always Chats, or always Send.
2. **Trusted devices receive without asking by default** (the brief's default
   grant, kept), with Ask before receiving per device.
3. **An unanswered offer expires after two minutes** as a decline.
