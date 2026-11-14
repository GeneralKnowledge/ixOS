# Relay stage 0 spike: iroh and word codes

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
