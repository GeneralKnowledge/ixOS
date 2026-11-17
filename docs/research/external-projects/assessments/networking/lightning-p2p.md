# Lightning P2P: what it does that Relay's delivery outbox doesn't

Status: 2026-11-17. Part of the [External Projects Atlas](../../README.md), P0 investigation 3. Research only: no change to Relay in this PR.

## Question

Lightning P2P is a transfer app on the same stack as Relay (iroh 1.0 and iroh-blobs). Against Relay's [delivery outbox](../../../../../examples/relay/docs/relay-delivery-outbox.md) (#299) and its current transfer core, what does it do for resume, retry, verification, storage and browser receiving that Relay should take, and what should Relay not take?

## Evidence

| Field | Value |
|---|---|
| Date | 2026-11-17 |
| Inspected | [Kerim-Sabic/lightning-p2p](https://github.com/Kerim-Sabic/lightning-p2p) at `ce2ee63` (2026-10-31); latest tags in the v0.9 series |
| Licence | Apache-2.0 (`LICENSE`, `NOTICE`) |
| Activity | 376 commits, one author; about 44 stars. Last commit 2026-10-31 |
| Read or run | Relay's side measured on Linux (below). Lightning's source and docs read: `src-tauri/src/node/` (retention, offers, request limits, relay wait), `src-tauri/src/storage/resumable_receives.rs`, `src-tauri/src/transfer/` (swarm, queue), `docs/ARCHITECTURE.md`, `docs/security-model.md`, `docs/browser-receiver-spike.md`, `docs/ios-testflight.md`. **Not built, not run** |
| Build evidence | Not built. Its Rust side uses `iroh` 1.0 and `iroh-blobs` 0.103 inside a Tauri 2 app (React front end); the browser receiver is a separate crate built for `wasm32-unknown-unknown` |
| Platform evidence | **No iOS app.** Its iOS document says "prepared, not shipped": it needs macOS, Xcode, CocoaPods and the multicast entitlement. Desktop, Android and browser receiving are its platforms |
| ixOS baseline | main at `98a8446`: Relay's Rust core (`examples/relay/rust/src/node.rs`, `code.rs`, `code_send.rs`; `iroh` 1, `iroh-blobs` =0.104.1) and the outbox decision in `relay-delivery-outbox.md` (#299, proposed, built in brief PR 4) |

## Where the two already agree

Both fetch with iroh-blobs, so both verify every chunk with BLAKE3 and resume from what the receiver already has. Both show an offer and wait for the person before bytes move (Lightning: `node/nearby_offer.rs`, 60 s decision timeout, unanswered is a refusal; Relay: the transfer journey's offer card, 2 minutes). Both acknowledge only after the file is saved (Lightning's "saved" receipt; the outbox's write-then-acknowledge). Nothing here reopens Relay's decisions.

## What Relay should take

### 1. Garbage-collect the blob store, with tags for what must stay (P1, before the outbox)

**Lightning:** `node/retention.rs` turns on iroh-blobs garbage collection and pins what must survive with named tags: `share/<root>` for everything this device offers, `receive/<transfer id>` for a receive while it downloads and for as long as its resume record exists. Collection waits until a burst of receives has settled, and its comment says "saved duplicates are freed within seconds of a receive".

**Relay today:** one persistent `FsStore` at `codes/store` (`node.rs`, `FsStore::load`), whose default options have garbage collection **off** (`iroh-blobs` 0.104.1, `store/fs/options.rs`: `gc: None`). Sends protect their blobs with temporary tags only. A received file is copied out of the store into Documents with `export` (`code.rs`, `save`), and nothing deletes the store's copy afterwards.

**Consequence, measured below:** every received file takes its space twice, and the store only grows.

**For the outbox:** temporary tags end with the process, but the outbox keeps items for up to 30 days across relaunches. Queued sends and paused receives need **persistent named tags** (as Lightning's), or a later collection would delete what the outbox is still waiting to send.

**Proposal:** in brief PR 4 (or earlier as a fix), open the store with a `GcConfig`, tag every queued delivery and every paused receive by its id, drop the tag when the item ends, and let collection reclaim the rest. Test: receive a file, wait for one collection, and check the store holds no copy; queue a send, relaunch, collect, and check it still sends.

### 2. Bound how much any one peer can ask for (P2)

**Lightning:** `node/blob_request_limiter.rs` caps concurrent blob requests at 128 in total and 16 per peer.

**Relay:** strangers are refused at connection time (`ConnectMode::Intercept`), so only code partners and trusted devices reach the store. With the outbox, one trusted device can hold several transfers open. A per-peer cap is cheap protection against a buggy or compromised device starving the others, and fits the outbox's fairness section.

### 3. Remember that relays are unreachable, instead of waiting each time (P2, with brief PR 6)

**Lightning:** `node/relay_wait.rs` bounds the wait for a relay route (6 s the first time); after a timeout it re-checks for only 0.5 s for the next two minutes, and any success restores the full wait.

**Relay:** relays are off today, so this doesn't apply yet. When the resolver and internet reach arrive (brief PRs 6 and 7), the same rule stops every transfer from stalling on a network without internet. Noted for those PRs.

### 4. A journal entry before publishing a received file (P2, in the outbox)

**Lightning:** `storage/resumable_receives.rs` writes the expected hash, size and file name **before** publishing a received file, and keeps a pending "saved" receipt until the sender has it, so a crash between saving and recording is recovered on the next launch.

**Relay's outbox:** files are verified, moved into `Documents/Relay`, then acknowledged, and the recipient remembers `(sender, delivery id, root hash)`. A crash after the move and before that record would make the sender retry, and the second copy is caught only by the "same root hash and earlier copy unchanged" rule. Recording the intent first closes that gap directly. A one-line addition to the outbox's acceptance tests: kill the recipient between the move and the acknowledgement, relaunch, and check there is one copy and the sender sees Delivered.

### 5. Parallel fetching for folders of many small files (watch)

**Lightning:** `transfer/swarm.rs` fetches a collection's children over parallel direct connections, because the standard sequential fetch pays one round trip per file; it is opt-in and falls back to the standard path.

**Relay:** folders arrive as collections fetched sequentially. Worth measuring once Relay has a folder-of-photos test on Wi-Fi; not worth the complexity before a measurement shows the dead air.

## What Relay should not take

- **Bearer tickets.** Lightning's `BlobTicket` is a capability: "anyone with a valid ticket can request the referenced content while the sender is online" (`docs/security-model.md`). Relay's word codes with SPAKE2 and its refusal of strangers are stronger, and stay.
- **Public n0 relays as the default.** Lightning uses n0's relays; Relay's plan runs its own (`relay-transfer-plan.md`).
- **The browser receiver now.** Lightning's spike proves an iroh 1.0 receive path builds for wasm and fetches over a relay (`docs/browser-receiver-spike.md`): useful evidence for Relay's stage 3 browser receiver (Floe's idea, on the [watchlist](../../decisions/watchlist.md)). It needs relays, which Relay doesn't run yet, and a ticket in a URL fragment, which is the bearer model above.

## Measurement: does Relay's store keep received files?

Run on Linux in this session (2026-11-17), with the `relay` command-line tool built from main at `98a8446` (`cargo build --release --features cli --bin relay`), two separate data directories on one machine, and a 50,000,000-byte random file:

| Step | Result |
|---|---|
| `relay send-code big.bin`, then `relay receive <code>` | Received and verified: "Received big.bin (50000000 bytes, 0 already here)" |
| Receiver's data directory after the receive | 96 MB: the exported `codes/received/big.bin` (50,000,000 bytes) **and** `codes/store/data/9abe0f7d….data` (50,000,000 bytes) |
| Receiver opened again (`relay history`) | The store still holds 48 MB: the second copy survives a relaunch |

So every received file is kept twice, and the store's copy is never reclaimed. The iPhone app uses the same Rust core (`RelayCore::new` with the app's data directory; received files go to `Documents/Relay`), so the same should happen there; that was not measured on the phone. A one-minute phone check: receive a 200 MB file, then compare iPhone Storage › Relay's size before and after. One side effect worth keeping: a repeated receive of the same content finds it already in the store ("already here"), which the outbox's "Already in Relay" rule could use, but on purpose, with a tag, not by never deleting.

## Scorecard

| Dimension | Rating | Reason |
|---|---|---|
| Capability | Adequate | Nothing Relay lacks in kind; specific techniques Relay hasn't built |
| Product value | Strong | The storage finding affects every person who receives files on the iPhone |
| Architecture | Strong | Same iroh and iroh-blobs stack, Rust owning the transfer; techniques port directly |
| Platform | Weak | No iOS app; nothing here is proven on an iPhone |
| Development | Unknown | Not built; not needed to borrow the techniques |
| Maintenance | Adequate | Active through October, one author |
| Licence | Strong | Apache-2.0; ideas are reimplemented, not copied |
| Security | Adequate | Its ticket model is weaker than Relay's; its request limiter is a good idea |
| Integration cost | Strong | Small changes in Relay's own code |
| Reusability | Adequate | Relay only, plus any later app on Relay's transport |

## Verdict

**Borrow**, with one finding to act on: Relay's blob store needs garbage collection and persistent tags (technique 1), which the outbox depends on anyway. Techniques 2 to 4 go into the outbox and resolver PRs as listed; technique 5 waits for a measurement. Not a dependency, and its iOS claims are not evidence.

Reopen when: Relay builds the browser receiver (stage 3), or Lightning ships an iOS app.
