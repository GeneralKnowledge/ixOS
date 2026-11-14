# Relay as a file transfer app: plan

Status: proposal, 2026-11-14. No app code until it is approved.

Relay today sends notes and files between devices that paired once by QR
code, on the same network ([examples/relay](../examples/relay/README.md)).
This plan grows it into a product of its own: send any file to any of your
devices, first on the local network and later over the internet, with the
"send, get a short code, type it on the other device" flow that
[croc](https://github.com/schollz/croc) and fsend made popular.

## What the person does

```text
Laptop                                   iPhone
$ relay send holiday.zip                 Relay > Receive
Code: 7-orbit-velvet                     types 7-orbit-velvet (or scans the QR)
Sending holiday.zip (1.2 GB) ...         Receiving holiday.zip ... saved to Files
```

- **Send:** pick files, photos or a folder (or `relay send <paths>`). Relay
  shows a code and a QR code and waits.
- **Receive:** type the code, or scan the QR code from the sender's screen.
  The receiver sees the file names and total size and accepts.
- **Trusted devices:** after a transfer, "remember this device" (on by
  default for your own devices) lets later sends skip the code: pick
  "MacBook" from a list, as Relay's paired devices work now.
- Works with no internet on the same Wi-Fi (stage 1). Works across networks
  later (stage 2): phone on 5G, laptop at home.

Devices: iPhone app (in Device Lab, and on its own), the `relay` command on
Linux (and macOS, same code), Android on the emulator once stage 1 works.
Reuben's own set: the Ubuntu laptop, the iMac, Hetzner and Oracle servers,
the iPhone, all on Tailscale. The goal is a universal, account-free file
transfer utility: no sign-up, no Tailscale needed by the other person.

## What exists to reuse

| Option | What it is | Fit |
|---|---|---|
| **croc** | Go, MIT. Word code, PAKE, public relays, direct path when NAT traversal succeeds | The flow we want. Go, so not embeddable in a Rust core on iOS. Borrow the UX, not the code |
| **fsend** | Go CLI with the same send/receive-by-code flow | Borrow the UX only |
| **magic-wormhole** | A published protocol (SPAKE2 code phrases `7-guitar-revenge`, a "mailbox" rendezvous server, a "transit" relay), many clients (Python CLI, Warp, Wormhole William on Android). Rust crate `magic-wormhole` 0.8.1 (May 2026), **EUPL-1.2** licence | Proven code-phrase design. But the rendezvous server is always on the internet, even for two devices on one Wi-Fi, so it fails our "local first, offline" start. Transfer v2 is unfinished in the Rust crate, and EUPL is a copyleft licence beside MIT ForgeKit. Borrow the design (SPAKE2, numbered word codes), don't adopt the protocol now |
| **iroh** | Rust, MIT/Apache-2.0. **1.0 stable since June 2026** (wire compatible within v1). QUIC connections dialled by public key: direct on a LAN, hole punching across NATs, relay fallback (n0's public relays, rate limited, or our own: the relay is open source). Officially supports Swift/iOS embedding. `iroh-blobs` (BLAKE3 verified, resumable transfers) is what `sendme` is built on | The transport and file layer for both stages: one stack for LAN and internet, written in Rust. No short codes: tickets are long strings. We add the code phrase on top |
| **LocalSend** | Open LAN protocol v2: UDP multicast discovery, HTTP or HTTPS (self-signed certs), optional PIN | LAN only, PIN or trust-on-first-use instead of a PAKE. Its multicast discovery needs Apple's multicast entitlement (see below). A possible later "send to LocalSend apps" bridge, not the base |
| **Network.framework / Bonjour** | Apple's discovery and sockets | Already how Relay finds devices on iOS. Keeps doing discovery on Apple; not cross-platform crypto |
| **Multipeer Connectivity** | Apple peer to peer | Apple only, so no Linux or Android. Not used |
| **Floe** | Go + TypeScript, MIT. Word code (`olive-tiger-castle`), WebRTC with a signalling server and optional TURN, self-hostable. One wire protocol across browser, desktop and CLI | Its idea worth taking: a **browser receiver**, so a device without the app can still get a file. WebRTC in a Rust core on iOS is heavy; see stage 3 |
| **wush** (Coder) | Go, CC0. `wush serve` prints an auth key (address, DERP region, public keys); WireGuard via Tailscale's `tsnet` and Tailscale's public DERP relays, no Tailscale account. Files and shells | Shows how far a borrowed relay network goes. Its key is long (pasted, not typed); and it rides Tailscale's free relays, which exist for Tailscale users |
| **derphole** | CLI (run with `npx`) for one-shot transfers, tunnels and terminal sharing over DERP relays, which never get payload keys; not affiliated with Tailscale | Same lesson as wush. Interesting, young, not something to build on |
| **Taildrop** | Tailscale's own file sending between devices on one tailnet | Works today on Reuben's devices, but needs everyone on the tailnet. Its useful idea: when both ends are on a tailnet, use the tailnet address directly |
| **ptransfer** | Short-code transfers through a self-hosted relay | Couldn't find a canonical project under this name; the pattern (self-hosted relay + code) is what stage 2 does with iroh |
| **Relay today** | TCP + Noise `XXpsk3` (from `forge-devproto`), CBOR frames, QR pairing with a 32-byte one-time secret, Bonjour `_forgerelay._tcp` | Keep the product shape (identity, trusted devices, history, inbox, CLI, AI asks). Replace the hand-built wire with iroh, which also clears friction A16 (Noise living in a frozen tooling crate) |

Other building blocks, in one line each:

- **QUIC:** yes, through iroh (fast, encrypted, many streams on one
  connection).
- **STUN/TURN-style NAT traversal and relay fallback:** yes, through iroh's
  hole punching and relays (the same job, without running coturn).
- **WebRTC:** only if a browser receiver is wanted (stage 3); iroh's
  browser build (relay only) is the lighter alternative to compare.
- **libp2p:** does what iroh does with far more moving parts and a weaker
  iOS story; not needed.
- **BitTorrent/WebTorrent, IPFS:** made for one file to many strangers; a
  two-device transfer gains nothing. iroh-blobs already gives content
  addressing (BLAKE3) and resume.
- **Syncthing:** continuous folder sync is a different product. A later
  "keep this folder in sync with my laptop" mode could borrow its ideas;
  not in scope.

### Three kinds of network dependency

The real difference between these tools is whose servers they lean on when
two devices can't reach each other directly.

| Kind | Examples | For Relay |
|---|---|---|
| **Someone else's public infrastructure** | croc's and magic-wormhole's public servers, n0's iroh relays, Tailscale's DERP (wush, derphole) | Zero setup, but rate limits, someone else's goodwill, and their uptime. Fine for development, not a foundation |
| **Infrastructure you already have** | Tailscale (Taildrop, tailnet addresses) | Free win for Reuben's own devices: when both ends are on the tailnet, connect to the tailnet address directly, no relay at all. Not universal (the other person needs the tailnet) |
| **A server you run** | ptransfer-style self-hosted relay, croc relay, `iroh-relay`, a rendezvous service | The universal answer, and cheap here: Reuben already has Hetzner and Oracle servers. Account-free for users, nothing borrowed |

**Plan: all three, in order of preference.** Direct on the LAN first; then
the tailnet address if both ends have one; then our own relay and
rendezvous on Hetzner (with Oracle as a second region later); n0's public
relays only during development.

## Recommendation

**Own product flow, borrowed protocols: iroh for transport and files,
magic-wormhole's code-phrase design on top, Bonjour for discovery on
iPhone.**

It takes the best idea from each: croc's and fsend's send-and-type-a-code
flow, magic-wormhole's PAKE word codes, iroh's transport (the same job as
DERP in wush and derphole, with relays we run), Taildrop's direct tailnet
path, and later Floe's browser receiver.

1. **Transport: iroh 1.0.** Each device has an iroh endpoint (an Ed25519 key)
   as its identity. On a LAN, connections go direct with no internet. Across
   networks iroh does the hole punching and falls back to a relay that only
   sees ciphertext. One stack for both stages, so stage 2 adds servers, not
   a second transport.
2. **Files: iroh-blobs.** Content-addressed (BLAKE3), verified while
   streaming, resumable after a dropped connection, folders as collections.
   This replaces Relay's own Offer/Chunk/Done and SHA-256 check. (iroh-blobs
   is not yet 1.0; pin the version, and keep a plain QUIC stream fallback in
   mind if it moves too much.)
3. **Codes: SPAKE2, as magic-wormhole does it,** with the `spake2` crate
   (RustCrypto, MIT/Apache). The code is `<number>-<word>-<word>`. The
   number (the "nameplate") finds the sender; the words are the password.
   Both sides run SPAKE2 over the first connection, so the code is never
   sent and an eavesdropper can't test guesses offline. The agreed key
   then confirms the two iroh endpoint keys, so the rest of the transfer
   is iroh's normal encrypted QUIC.
4. **Discovery:** on a LAN, the sender advertises Bonjour
   `_forgerelay._tcp` with the nameplate number and its endpoint key in the
   TXT record; the receiver looks for that number (Swift on iOS, as now; a
   Rust mDNS crate in the CLI). Over the internet (stage 2), a small
   rendezvous service maps the nameplate to the sender's endpoint key for a
   few minutes, then iroh connects.
5. **Not now:** magic-wormhole wire compatibility. It would let Relay talk to
   `wormhole send` on any machine, but needs the internet even on a LAN and
   brings EUPL code into the app. It can come later as a `relay wormhole`
   CLI bridge if wanted.

What we write ourselves: the code layer (a few hundred lines around
`spake2`), the rendezvous service in stage 2 (a small Rust server with a
key-value map and expiry), the Bonjour glue, and the app. Everything that is
hard (crypto, NAT traversal, relays, verified resumable transfer) is
borrowed.

## Code phrases: words or letters?

| Style | Example | For | Against |
|---|---|---|---|
| Number + 2 words (magic-wormhole) | `7-orbit-velvet` | Easy to say aloud and type on a phone, autocorrect-friendly with completion | English words |
| Number + 3 words (croc style) | `4821-orbit-velvet-canyon` | More margin | Longer to type |
| Letters (fsend style) | `abc-defg-jkm` | No language | Hard to read aloud (b/d/p), awkward on a phone keyboard |

**Recommend number + 2 words** from a 256-word list (magic-wormhole uses the
PGP word list; the EFF short list is an alternative), with word completion
in the app and the CLI, plus a QR code for devices in the same room.
Because of the PAKE, 2 words is enough: an attacker gets one live guess per
code (about 1 in 65,000), a wrong guess ends the code, and the sender sees
"someone tried a wrong code". Codes expire after 10 minutes or one use.

## Security

- **Threats covered:** someone on the same Wi-Fi or a relay operator reading
  or changing files; someone guessing a code (one online guess); a lost
  code reused later (single use, expiry).
- **Keys:** SPAKE2 per transfer, then iroh's QUIC (TLS 1.3 with the
  endpoints' raw public keys). Forward secrecy from QUIC's ephemeral keys.
  Relays and the rendezvous service see only endpoint keys, sizes and
  timing, never names or content.
- **Trusted devices** are stored endpoint keys. A trusted send needs no code
  and is refused if the key doesn't match. Secrets and the device key live
  in the Keychain on iOS (friction A17 needs fixing first, so the core can
  reach it) and in a 0600 file on Linux.
- **Receiver consent:** every incoming transfer from a code shows names and
  size before anything is written; trusted devices can be set to accept
  automatically. Received files keep their names but are written only
  inside Relay's inbox (no path tricks).
- **Optional verify words:** both screens can show two words derived from
  the session key, as magic-wormhole's `--verify` does, for the cautious.
- **Logging** through `forge_runtime::logging`: sizes, times, outcomes.
  Never file names, notes, codes or keys.
- **Review:** the code layer and rendezvous get a focused security review
  before stage 2 ships to anyone but Reuben.

## iPhone limits on the free Apple team

| Limit | Effect | Plan |
|---|---|---|
| Local network permission | iOS asks once; needs `NSLocalNetworkUsageDescription` and every Bonjour type in `NSBonjourServices` | Already in `forge.toml` |
| Multicast entitlement (`com.apple.developer.networking.multicast`) | Needs a request to Apple and a paid team. Without it the app can't send raw mDNS or UDP multicast (iroh's own mDNS discovery, LocalSend discovery) | Discovery on iOS stays on Bonjour (Network.framework), which needs no entitlement, and hands addresses to iroh |
| No background sockets | A transfer stops a few seconds after the app leaves the screen (about 30 s with a background task) | Keep the screen awake during a transfer, resume with iroh-blobs when the app returns. Background `URLSession` only helps for HTTP to a server, so it fits a later "upload to a store" mode, not peer to peer |
| No push notifications | The phone can't be woken for an incoming file | Receiving happens with Relay open. Fine for a code flow |
| Share extension | Costs an App ID (10 new per week) and has a small memory budget; under Device Lab it belongs to the host | Stage 3, after Reuben decides on Device Lab vs its own app |
| 7-day signing | Re-sign weekly, as every app now | Nothing new |

## Stages

| Stage | What | Proves | Size |
|---|---|---|---|
| **0. Spikes** | iroh 1.0 + iroh-blobs + spake2 in `relay_core`: Linux to Linux offline; iOS build size and first launch; a direct LAN connection from addresses Bonjour found; Wi-Fi throughput on a 1 GB file. Builds go through the laptop's build queue once builds resume | iroh works inside Forge's runtime on iOS and keeps the app reasonably small. If it doesn't: keep today's TCP + Noise for LAN, use magic-wormhole for stage 2 | 2 to 3 days |
| **1. Local network** | `relay send <paths>` / `relay receive <code>`; app Send (files, photos, folder) and Receive (type or scan); progress, cancel, resume; trusted devices; history; received files in the Files app; AI asks (protocol 2) moved onto an iroh protocol; old TCP wire removed. Hosted in Device Lab. Android emulator screen if time | The croc flow on one Wi-Fi with no internet: phone to laptop, laptop to phone, phone to phone | 1.5 to 2 weeks |
| **2. Over the internet** | Rendezvous service (Rust, small) and our own `iroh-relay`, both on Hetzner (n0's public relays until then); tailnet addresses tried before any relay; trusted devices reachable anywhere by key; clear "direct", "via tailnet" or "via relay" in the UI; `relay send` on the servers too | Phone on 5G to laptop at home; laptop at a café to phone at home | 1 to 1.5 weeks + server setup |
| **3. Product polish** | Share extension ("Send with Relay" from Photos), a browser receiver page for devices without the app (Floe's idea; iroh in the browser or WebRTC, decided by a spike), drag and drop, clipboard text, Android app beyond the emulator when the phone is back, optional `relay wormhole` and LocalSend bridges | Everyday use | as chosen |

Each stage has one short phone check (about 10 minutes, batched with
others), built and published over the air before the phone is needed.
Capability changes update the ledger (`docs/ledger/`) in the same PR.

## Where it lives

Relay stays `examples/relay` in ixOS, its own module hosted in Device Lab,
so it can move to its own repository later without changes (its crate is
already its own workspace). Splitting ixOS into repositories is parked, so
the plan doesn't assume it. If it becomes a real product, its own repo,
name and App ID make sense at stage 2 or 3.

## Decisions for Reuben

1. **Approach:** own flow on iroh with magic-wormhole-style codes
   (recommended), or full magic-wormhole compatibility (talks to existing
   `wormhole` clients, but needs the internet even on a LAN).
2. **Code style:** number + 2 words (recommended), number + 3 words, or
   letters.
3. **Repository:** stay in ixOS as `examples/relay` (recommended for now),
   or a separate repository today.
4. **Name:** keep "Relay", or a new product name (Relay also means "relay
   server" in this design, which can confuse).
5. **Stage 2 servers:** run the rendezvous service and relay on Hetzner
   (recommended; Oracle as a second region later), and whether n0's public
   relays are fine until then.
6. **Browser receiver:** wanted in stage 3 (Floe's idea), or CLI and apps
   only.
7. **Old pairing:** replace Relay's QR pairing with trusted devices
   (recommended; it's a demo with no users), or keep both for a while.

## Sources

- croc: <https://github.com/schollz/croc>
- magic-wormhole protocol: <https://magic-wormhole.readthedocs.io/>; Rust
  crate: <https://docs.rs/crate/magic-wormhole/latest>
- iroh 1.0: <https://iroh.computer/blog/v1>; sendme:
  <https://github.com/n0-computer/sendme>
- LocalSend protocol: <https://github.com/localsend/protocol>
- Floe: <https://github.com/jannskiee/floe>; wush:
  <https://github.com/coder/wush>; derphole:
  <https://shayne.github.io/derphole/>; Taildrop:
  <https://tailscale.com/docs/features/taildrop>
- Relay today: [examples/relay/README.md](../examples/relay/README.md),
  friction A16 to A19 and A26 to A27 in
  [research/app-friction-log.md](research/app-friction-log.md)
