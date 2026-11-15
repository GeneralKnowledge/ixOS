# Relay stage 0 spike: iroh and word codes

> **Not production code.** A throwaway Linux experiment. M0 is now Linux to
> Linux (Reuben, 2026-11-14); the iOS build and the iPhone test move to the
> start of M1. The review's limitations below are fixed in the spike
> ([M0 on Linux](#m0-on-linux-2026-11-14)) but not yet proven by a model
> (see Verification in the platform plan).

2026-11-14. Answers the stage 0 questions in
[relay-transfer-plan.md](../relay-transfer-plan.md) on Linux. The iPhone half
waits for the laptop, where builds are paused.

Code: [examples/relay/spike](../../examples/relay/spike/README.md), about 450
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
| Session rules | The sender's code session is a pure state machine (`src/session.rs`) that mirrors `verification/relay/CodeSession.lean`: every admitted attempt ends in exactly one `finish` (proved, wrong code or dropped); busy and gone answers change nothing; a spent or expired code is refused until it is replaced; collision and handshake events carry the code's generation and events about a replaced code are ignored; a collision only replaces a code that is still unused, and expires it under the same lock so no receiver slips in before the new code (a collision queued before a receiver was admitted no longer cuts that handshake off). 15 unit tests, one of them 2,000 random sequences of 60 steps checking the model's invariant after every step |

Not covered yet: two physical machines on a real network (next), and the
iPhone (M1).

## Not answered yet (needs the laptop)

- The iOS static library builds with xtool's SDK, and its arm64 size.
  Command in the spike README; run through `forge queue run` once builds
  resume.
- iroh on the phone: first bind, local network permission prompt, a direct
  connection to the laptop on Wi-Fi from addresses Bonjour resolved, and
  throughput on a 1 GB file. That is the first stage 1 phone check.

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
