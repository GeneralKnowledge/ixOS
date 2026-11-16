# Relay stage 0 spike: iroh and word codes

> **Not production code.** A throwaway Linux experiment. M0 is now Linux to
> Linux (Reuben, 2026-11-14); the iOS build and the iPhone test move to the
> start of M1. The review's limitations below are fixed in the spike
> ([M0 on Linux](#m0-on-linux-2026-11-14)) but not yet proven by a model
> (see Verification in the platform plan).

2026-11-14. Answers the stage 0 questions in
[relay-transfer-plan.md](relay-transfer-plan.md) on Linux. The iPhone half
waits for the laptop, where builds are paused.

Code: [examples/relay/spike](../spike/README.md), about 450
lines, its own workspace, not part of the app.

## What was built

`relay-spike send <file>` and `relay-spike receive <code>`:

1. Both sides start an iroh 1.4.1 endpoint with relays turned off, so
   nothing leaves the local network.
2. The sender makes a code `<1-99>-<word>-<word>` from two random bytes
   (PGP word list, even then odd, so swapped words are caught as a typo) and
   advertises `_forgerelay._udp` over mDNS with the code's number and its
   endpoint id in the TXT record. These are DNS-SD records, so Bonjour on
   iPhone reads the same thing.
3. The receiver browses for that number, connects on the code protocol
   (`forge-relay/code/0`), and both run symmetric SPAKE2 (`spake2` 0.4,
   identity `forge-relay/v1/<number>`) with the words as the password.
4. Each side proves the key with an HMAC over its role and both endpoint
   ids. QUIC has already authenticated those ids, so this ties "knows the
   code" to "is this endpoint".
5. Only then does the sender name the file (BLAKE3 hash, size, name) and
   let that endpoint id fetch it with iroh-blobs (a connection gate refuses
   every other endpoint). iroh-blobs checks every chunk against the hash as
   it streams.

## Results

| Question | Result |
|---|---|
| Transfer works offline on one network | Yes: 1 GB of random data, identical after transfer (`cmp`), two processes, relays off |
| Speed | 316 to 334 MB/s over loopback, including the receiver's in-memory store. Wi-Fi will be the limit on real devices; measure on the phone |
| Discovery by code number | Yes: mDNS found the sender within the first browse, even inside the cloud container |
| Wrong code | The receiver sees "wrong code"; the sender sees "someone tried a wrong code; it no longer works" and stops. A second try with the right code finds nothing, as intended (one guess per code) |
| Stranger who knows the hash but not the code | Refused: "Unable to download"; the right receiver then gets the file |
| Hashing cost on the sender | 1 GB in 2.1 s before the code is shown |
| Size (Linux x86-64, stripped, LTO) | 11.1 MB for the spike, against 3.8 MB for today's `relay` command: about +7 MB. iroh default features off except `tls-ring` |
| Dependencies | 402 crates (today's `relay`: 137). Mostly iroh's QUIC stack and iroh-blobs' redb store |
| `ring` for iOS | Already built for iOS in ixOS (rustls via forge-net), so no new C toolchain question |

## Known limitations (from review, 2026-11-14)

| Limitation | Risk | Fix |
|---|---|---|
| **Code number collisions.** The number is random from 1 to 99 and the receiver takes the first sender advertising it | Two transfers at once on one network can send the receiver to the wrong sender, which also spends that sender's code | Senders pick a number nobody nearby is using and re-pick if one appears; a receiver that finds two senders with its number stops and asks for a new code rather than guessing (a guess would burn an unrelated code) |
| **The code is spent before the proof is checked** | Intended (one guess per code), but anyone on the network can burn a code and interrupt a transfer | A time limit on the handshake, one handshake at a time (others are told "busy" without spending the code), the sender is told, and a new code is issued at once |
| **16 bits in the code** (two PGP words) | SPAKE2 stops offline guessing, but an active attacker's single online guess is 1 in 65,536 | Three words from the EFF short list (about 31 bits) with completion; the QR code stays the easiest way |
| **Memory stores** on both sides | A large file is held in RAM; nothing resumes | iroh-blobs' file store, bounded streaming, resume after an interruption |

## M0 on Linux (2026-11-14)

The review's fixes, tested between processes on one Linux machine (two
real machines next):

| Check | Result |
|---|---|
| Code | A number and three EFF short words (`37-grasp-gag-drum`), about 31 bits; words checked against the list when typed |
| Numbers in use | A second sender picked a different number (37, then 97) after listening for 1.5 s |
| Forced collision (test switch `RELAY_SPIKE_NAMEPLATE`) | The sender saw another sender take its number and issued a new code; a receiver that saw two senders with one number stopped with "two senders nearby are using number 37; ask the sender for a new code" and spent nobody's code |
| Wrong guess | The receiver is told; the sender says "Someone tried a wrong code, so it no longer works. New code: …"; the new code works and the file is identical |
| Busy | A second receiver during a transfer is told "the sender is busy" and the code is not spent |
| Resume | 4 GB transfer, receiver killed after about 0.3 to 0.4 GB, twice: the sender noticed within 8 s (receivers tick every 2 s; the window is now 30 s after the slow tailnet run below) and issued a new code; the receiver resumed with "332791808 of 4000000000 bytes already here" and the file was identical (`cmp`) |
| Memory | Peak resident memory during a 4 GB transfer: receiver 24 MB, sender 21 MB (iroh-blobs file store; the sender hashes the file in place) |
| Speed | 290 to 364 MB/s over loopback with the file store |
| Stranger | Still refused with the right hash |
| Malformed first message (test command `garble`) | Counts as a wrong code: the sender issues a new code at once and that code works (file identical). Before the fix a malformed message left the code marked busy forever |
| Session rules | The sender's code session is a pure state machine (`src/session.rs`) that mirrors `verification/relay/CodeSession.lean`: every admitted attempt ends in exactly one `finish` (proved, wrong code or dropped); busy and gone answers change nothing; a spent or expired code is refused until it is replaced; collision and handshake events carry the code's generation and events about a replaced code are ignored; a collision only replaces a code that is still unused, and expires it under the same lock so no receiver slips in before the new code (a collision queued before a receiver was admitted no longer cuts that handshake off). 18 unit tests, one of them 2,000 random sequences of 60 steps checking the model's invariant after every step |

## Two machines (2026-11-15)

The laptop (home line) and a Hetzner server, over Tailscale (direct over
IPv6, not through a DERP relay; ping 49 to 103 ms, 55 ms average), both
running the same binary (0ab5e7e, then 2ef86c3 for the rerun), the receiver using the `--addr`/`--id` form because mDNS
does not cross the tailnet. The home line sets the speed.

| Check | Result |
|---|---|
| 1 GB Hetzner to laptop | Identical (sha256), 4.4 MB/s, 228 s |
| Wrong code | Refused; the old code was then refused too; a new code within 2 s |
| Malformed first message (`garble`) | A new code within 5 s; that code then received a 20 MB file, identical |
| 4 GB resume | Receiver killed at 502 MB; the sender issued a new code 32 s later (the 30 s silence window); the receiver resumed from "524091392 of 4000000000 bytes already here" |
| 1 GB laptop to Hetzner, and finishing the 4 GB resume | **Failed at 0ab5e7e, fixed in 2ef86c3:** both were cut off at exactly 600 s. The sender counted the code's ten-minute lifetime even after a receiver had redeemed it, so a transfer longer than ten minutes was ended mid-file. Now the lifetime only limits how long an unused code can be redeemed (`Session::lapse`); a running transfer ends with done, or after 30 s of silence |
| Rerun at 2ef86c3: 1 GB laptop to Hetzner | Identical (sha256), 1057 s at 0.9 MB/s (the home upload), well past the old cut-off |
| Rerun at 2ef86c3: 4 GB Hetzner to laptop with an interruption | Receiver killed at 501 MB; a new code 32 s later; resumed from "522551296 of 4000000000 bytes already here", fetched the remaining 3.48 GB in 730 s (5 MB/s); identical (sha256) |
| Plain mDNS between the two | Not possible here: they are not on one network |

An earlier attempt (before the silence window was raised from 8 s to 30 s,
while another build was loading the laptop and ping rose to about 1.1 s)
stalled after 6.4 MB; the next code resumed from the 6,586,368 bytes
already there.

## iPhone (start of M1, 2026-11-15)

Relay's own receiver (`relay_core::code`, with Swift's "Receive with a Code"
sheet) inside Device Lab, installed by cable at 7f1c94a, on an iPhone 15 Pro
Max (iOS 26.3.1). The sender was `relay-spike send` on the laptop, on the
same home Wi-Fi.

| Check | Result |
|---|---|
| iOS build of the iroh stack | Relay, Device Lab and minimal build with `check-ios-xtool.sh` (needs libc held at 0.2.189, the stub linker for dependencies' cdylibs, and SystemConfiguration). The spike staticlib is 16.2 MB, 11.8 MB stripped |
| Bonjour finds the Rust sender | Yes: `_forgerelay._udp` from the code's number, no address typed |
| Laptop to iPhone, 200 MB | Received at the first try with code `88-gem-oil-union`; the sender prints "Sent." only after the receiver has verified every chunk (BLAKE3) and saved the file |
| Wi-Fi speed | 15.9 MB/s shown on the phone (200 MB in about 12.6 s) |
| Laptop to iPhone, 1 GB | 105.2 s, 9.5 MB/s, intact, first code (OTA Device Lab build after #232) |
| iPhone to Linux | Passed: "Send with a Code" (#214, fixes #232 and #235) sent a 9,711-byte file; `relay-spike receive` got code `22-flyer-step-dig` at the first try, 2.75 s for the whole flow, BLAKE3-verified, sha256 matched |
| Phone memory during a transfer | The sheets show a `phys_footprint` readout; a reading from the phone is not recorded yet |

Then (#214, #232, #235): the sender moved into relay-core
(`relay_core::code_send`, with the session state machine as
`code_session`), "Send with a Code" in the app (Swift advertises the code
with Bonjour and reports other senders, so Rust avoids their numbers), a
peak-memory readout on both sheets, and `relay send-code FILE` on Linux.
"yo-yo" is dropped from the word list on both sides, since a dash separates
a code's words. A loopback test covers a wrong code, the new code, and the
file arriving byte for byte.

Two fixes came from the phone. The sheet now shows "Getting ready" as soon
as a file is picked, since Rust marks the send as preparing before it starts
(#232). iOS suspends an app's Bonjour registration while it is in the
background, so the app publishes the code again when it becomes active, and
the sheet asks the sender to keep Relay open (#235).

## What changes for stage 1

- Use iroh-blobs' file store (`FsStore`), not memory: the spike's receiver
  held 1 GB in RAM, which the phone can't. The file store also gives resume.
- Collections for several files and folders (iroh-blobs `HashSeq`).
- Bonjour type: iroh runs over UDP, so the service becomes `_forgerelay._udp`
  in `forge.toml`'s `bonjour_services`.
- Code words: the PGP list works but has long words ("handiwork",
  "headwaters"). Try the EFF short list with completion before stage 1 ships.
- Keep the gate: blobs only to endpoints that proved the code or are
  trusted devices.
- Sizes are acceptable for an app (Device Lab already hosts several), but
  watch them; drop iroh-blobs' `rpc` feature if it isn't needed.
