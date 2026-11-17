# Reference deep audit 4: four transfer apps, for Relay

Status: 2026-11-17. Phase 1b of [ixOS Experience Research](../README.md), added after the [LocalSend audit](localsend.md). Research only: no change to Relay.

**References**, all read from source at the commit shown, none run:

| Project | Commit | Licence | Stack | Why it is here |
|---|---|---|---|---|
| [iroh-drop](https://github.com/chad/iroh-drop) | `62d460f` (2026-08-04) | MIT or Apache-2.0 | Rust (iroh 1.0, iroh-blobs 0.103, iroh-gossip 0.101), SwiftUI app for macOS that talks to a helper daemon | The same transport stack as Relay |
| [RelayPonyCore](https://github.com/norsehorse-dev/RelayPonyCore) | `72d4f8f` (2026-07-08) | Apache-2.0 | Swift package (the iOS app's open core; the app itself is closed), age encryption over Bonjour and Network.framework | A shipping iPhone app, with the trust parts published |
| [Drift](https://github.com/vsamarth/drift) | `f840ce5` (2026-05-05) | MIT | Rust on iroh 0.97, Flutter app; no iOS app yet | AirDrop-like transfers with codes and resume |
| [Daak](https://github.com/darshitpp/daak) | `de23b51` (2026-11-16) | Apache-2.0 | Rust core over UniFFI, React Native app, Noise (`snow`) plus SPAKE2; sideloaded iOS build | The same Rust-core-plus-UniFFI shape as ixOS. It describes itself as "fully vibecoded" and has two stars: read it for ideas, trust none of its protocol |

**ixOS app:** Relay on main (`560eecb`, after #268). Relay's decisions (iroh, word codes with SPAKE2, pairing policies, trusted devices in brief PR 3, wait-until-seen messages after the [Chat merge](../../../../examples/relay/docs/relay-chat-merge.md)) are taken as given. **Evidence:** S (source) on both sides.

The headline: none of the four does something Relay has decided against for a good reason, and Relay's security is already at least as strong as each. What they add is behaviour around the transfer: asking before anything lands, catching up after being apart, retrying cleverly, and saying plainly what a broadcast exposes.

## What they do better, and why

### 1. Nothing lands until the person says yes (iroh-drop, Drift, LocalSend)

**Reference:** iroh-drop shows a card with name, size, sender and a countdown; nothing is written to disk until Accept, and **a card nobody answers is a refusal** (README, "The app"). Drift's step 3 is "the receiver reviews the files and accepts".
**Relay:** a code receive starts downloading as soon as the code is entered; the name appears at the end. Trusted devices' files simply arrive.
**Why it's better:** consent and context before bytes move, and a timeout that fails safe. Three of the four references (and LocalSend) agree, which makes it a convention people will expect.
**Where:** Relay app + Rust (the offer has to arrive before the blobs).
**Proposal (P2, raises the LocalSend audit's P3):** after the code is confirmed, show "From Reuben's iPhone: Trip, 3 items, 24 MB" with Receive and Cancel, with an unanswered offer expiring as a refusal. For trusted devices keep the pairing policy's meaning (they may send without asking) but add the same card when the person turns "Ask before receiving" on for that device.

### 2. Two devices that are never online together still converge (iroh-drop, Daak)

**Reference:** iroh-drop keeps a signed log of offers and, **whenever a neighbour appears, both sides pull what the other has that they missed** (anti-entropy on neighbour-up; `docs/protocol.md`, catch-up sync). Daak queues an item for an offline device and retries with a growing delay, and **resets the delay the moment discovery sees the device** (`daak-core/src/engine.rs`, "Reset the backoff of the pending rows for a peer that came online").
**Relay:** wait-until-seen is planned for messages (Chat merge step 3) but not designed for files, and nothing says when a retry happens.
**Why it's better:** a timer alone is either too slow or too chatty; "try now, because I just saw it" is what makes delivery feel instant when the laptop wakes.
**Where:** Relay Rust (the outbox), with presence from brief PR 5.
**Proposal (P1 for step 3):** one outbox for messages *and* files to trusted devices: growing backoff, reset and flushed when Bonjour or the resolver reports the device reachable, and on reconnect each side asks the other for anything sent while it was away. Its tests: laptop off, send three items, laptop on, all three arrive once and in order.

### 3. Say exactly what a broadcast exposes (iroh-drop)

**Reference:** iroh-drop's `--lan` is opt-in per share, lasts only as long as the process, and prints "anyone here can list and receive these files". Its roadmap explains why it added no PIN: "a PIN short enough to type is brute-forceable by anyone already on the network" (`docs/roadmap.md`, "How U11 was gated").
**Relay:** a code sender advertises `_forgerelay._udp` with the code's number and its endpoint id in TXT (`RelayNetwork.swift`, `findCodeSender`). That is safe, because SPAKE2 still needs the whole code, but nothing tells the person what nearby devices can see.
**Why it's better:** honest wording about what a nearby stranger learns builds the right trust, and it stops a later feature (LocalSend audit proposal 2, nearby code offers) from quietly turning into "anyone here can take it".
**Where:** Relay app (wording) and its docs.
**Proposal (P3):** a line in Relay's help sheet: "Nearby devices can see that you're sending, but not what. Only someone with the whole code can receive it." Keep the rule that nothing advertised on the network is itself a capability.

### 4. iPhones that aren't on the same Wi-Fi (RelayPony)

**Reference:** RelayPony's Bonjour transport sets `parameters.includePeerToPeer = true` (`BonjourTransport.swift`), which lets two Apple devices find and reach each other over Apple's peer-to-peer Wi-Fi (the link AirDrop uses) with no shared network.
**Relay:** `NWBrowser` and `NetService` advertise and browse on the normal network only, and the data goes through iroh's own sockets.
**Why it matters:** "both on the same Wi-Fi" is the most common reason a nearby transfer fails, and phone to phone in a park is exactly Relay's case.
**Where:** Relay Swift discovery; the data path is the open question.
**Proposal (P3, spike):** an hour's spike on two iPhones: turn on peer-to-peer browsing, and check whether iroh can reach an address on that interface. If it can't, discovery over peer-to-peer Wi-Fi still helps by saying "they're nearby, but not on your network" instead of nothing. Needs two iPhones, so it waits for one.

### 5. Golden bytes and fuzzing for the wire format (iroh-drop, Daak, RelayPony)

**Reference:** iroh-drop's normative examples are golden byte fixtures checked in tests ("they cannot drift from what the code actually speaks", `docs/protocol.md`); Daak fuzzes its frame and manifest decoders (`daak-core/fuzz/`); RelayPony tests its codec against fixtures computed by the Android app and rejects hostile filenames such as `../../secret`.
**Relay:** filename containment is already tested (`code.rs`, `inside_folder`). `relay/device/1` will carry CBOR messages between versions of Relay on different devices, with no fixtures yet.
**Why it's better:** a phone and a laptop on different builds must keep understanding each other; a fixture fails the PR that would break that.
**Where:** Relay Rust tests.
**Proposal (P2, with brief PR 3):** commit one encoded example of each `relay/device/1` message and decode it in a test; add a `cargo fuzz` target for the decoder when the format settles. Not a framework piece: one user.

### 6. Received photos go to Photos (Daak)

**Reference:** Daak puts received photos and videos in Photos (asking once for add-only permission) and other files in the folder chosen in Settings.
**Relay:** everything goes to On My iPhone, Relay (#268), so a received photo isn't in the camera roll.
**Why it's better:** people look for photos in Photos.
**Where:** Relay app; a second user would make an "add to Photos" helper a ForgeServices candidate (Sketchbook exporting a drawing is the likely one).
**Proposal (P2):** photos and videos from a transfer get **Save to Photos** on the received item, and a setting to do it automatically. Add-only Photos access works with the free team.

## What Relay already does as well or better

- **Codes are protected by a PAKE.** Drift's 6-character code is a lookup key on its rendezvous server for five minutes (`crates/server`, `DISCOVERY_TTL_SECONDS = 300`), rate-limited by IP: whoever guesses it first gets the ticket. Relay's word codes use SPAKE2, so a wrong guess burns the code. iroh-drop reached the same conclusion: its word codes are blocked because "the exchange needs a PAKE … and a rendezvous" (`docs/roadmap.md`, U12), the two things Relay's plan already has (PR 7 for the internet rendezvous).
- **Trust keyed on the key, never the name.** RelayPony pins the public key and lets a discovered device be one-tap only if the key it advertises is already pinned. Relay's trusted devices are endpoint ids, which is the same rule; keep showing a device as trusted only by id, whatever name it advertises.
- **No number to compare.** Daak and RelayPony fall back to comparing a short number on both screens. Relay's SPAKE2 confirmation already proves both sides typed the same code, so Relay doesn't need that step.
- **Resume.** Drift resumes a re-sent transfer from a record keyed by the collection hash; Relay has had resume through iroh-blobs since #267.
- **Device key in the Keychain**, device only (RelayPony): already brief PR 3.

## What not to import

- **A long-lived helper process** (iroh-drop's daemon over a Unix socket). It works on a computer because the daemon outlives the window; iOS has no such process. Relay's CLI on the laptop is the place for that idea, if anywhere.
- **Every recipient serving the files to others** (iroh-drop's replication). Good for group drops between computers; on a phone it spends battery and data on someone else's behalf.
- **A centralised code server that sees tickets** (Drift).
- **Daak's protocol code.** Its ideas are listed above; its code has had no review we can rely on.
- **Flutter and React Native shells** (Drift, Daak): flows only, not components.

## Where each proposal belongs

| # | Proposal | Priority | Where | Depends on |
|---|---|---|---|---|
| 1 | Offer card before a code download, unanswered means no; optional "ask before receiving" per trusted device | P2 | Relay app + Rust | |
| 2 | One outbox for messages and files: backoff reset on presence, catch-up on reconnect | P1 | Relay Rust | Chat merge step 3, brief PR 5 presence |
| 3 | Help-sheet line on what nearby devices can see | P3 | Relay app | LocalSend audit proposal 5 (help sheet) |
| 4 | Peer-to-peer Wi-Fi discovery spike | P3 | Relay Swift | Two iPhones |
| 5 | Golden fixtures (then fuzzing) for `relay/device/1` | P2 | Relay Rust tests | Brief PR 3 |
| 6 | Save to Photos for received photos and videos | P2 | Relay app | |

## Suggested next step

Hand proposal 2 to the Relay thread as a design input for Chat merge step 3, and proposals 1, 5 and 6 to the Relay transfer-journey work. Nothing here changes Relay's protocol decisions.
