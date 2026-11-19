# Relay

**Your devices. One network.** Private connectivity for the things you own
and the people you trust. Relay starts by moving files between your devices
and grows into a personal network; the roadmap is
[docs/relay-platform-plan.md](docs/relay-platform-plan.md), and the next
work is [docs/relay-m1-m2-brief.md](docs/relay-m1-m2-brief.md).

Today it sends files, photos or a folder to anyone on the same Wi-Fi with a
word code (typed, or scanned from its QR code), and, once the two sides trust
each other after a code transfer, notes and files straight to a trusted
device. Everything goes over iroh.
Chat merged into it on 2026-11-17 ([relay-chat-merge.md](docs/relay-chat-merge.md)):
the Chats, Assistant and Settings tabs are Chat's messaging through a chat
server you run, with an on-device assistant ([messaging.md](docs/messaging.md));
Devices is Relay's own screen. It
runs inside Device Lab (Apps, then Relay) and on its own. The second device
can be a Linux machine running the `relay` command, so one iPhone is enough.

## Where it is

The current milestone is [Relay M1: Reliable Trusted-Device Delivery](docs/relay-reliable-delivery.md). Ideas beyond it are in [relay-future-directions.md](docs/relay-future-directions.md). A review of the product and architecture (2026-11-18): [relay-review-2026-11.md](docs/relay-review-2026-11.md).

| Step | What | State |
|---|---|---|
| R1 | Rust core (`rust/`): identity, history; the `relay` command. Its first transport (QR pairing and a Noise channel over TCP) was removed in step 3 of the [review](docs/relay-review-2026-11.md) (2026-11-19) | Done, tested on Linux over localhost |
| R2 | SwiftUI app: Bonjour (NetService advertises, NWBrowser finds devices), notes and files from the file picker, received files in Quick Look, hosted in Device Lab | Built with xtool and installed on an iPhone in Device Lab (2026-11-15) |
| M1 start | Word codes on iroh (`code.rs`, `code_send.rs`): "Receive with a Code" and "Send with a Code" in the app, `relay send-code` on Linux, `relay-spike` from [spike/](spike/) | Passed on an iPhone 15 Pro Max on 2026-11-15: 1 GB Linux to iPhone at 9.5 MB/s and iPhone to Linux, each at the first code and verified ([results](docs/relay-stage0-spike.md#iphone-start-of-m1-2026-11-15)). Not yet a production transfer: no folders, no history, separate from paired transfers |
| M1 PR 1 | The transfer core: one iroh endpoint per device with a key kept in `device.key` (0600), one blob store in `codes/store` for sending and receiving, folders as one transfer (an iroh-blobs collection), resume after a relaunch, both directions in the history, stopping the sender stops the receiver ([brief](docs/relay-m1-m2-brief.md#1-production-transfer-core)) | Done on Linux (tests: a folder both ways, a transfer stopped halfway resumes after a relaunch and verifies); not yet on a phone |
| M1 PR 2 | Send and Receive in the app's bottom bar (several files, photos or a folder with one word code; a progress bar and Stop; a folder opens in the Files app), received files in Documents/Relay where the Files app shows them (On My iPhone), and `relay send PATH...` / `relay receive <code>` finding each other with mDNS ([brief](docs/relay-m1-m2-brief.md#2-native-transfer-screens-and-cli)) | Rust and the command done and tested on Linux (a folder both ways between two `relay` commands); the Swift is not yet compiled or tried on a phone |
| M2 PR 3a | Trusted devices: after a code transfer each side is asked "Trust this device?" (both learn the other's name); a trusted device gets notes, files and folders with no code over `relay/device/1` on iroh, found by Bonjour (`_forgerelay._udp`, TXT `d=1`); revoke (refused for good, codes never restore it, only the person trusts it again), forget, "Can Send to This Device", and the pairing policy (Ask to Trust, Trust Again Automatically, Codes Only); `relay devices`, `trust`, `revoke`, `forget`, `policy`, `send <device>` ([brief](docs/relay-m1-m2-brief.md#3-trusted-devices)) | Rust, the command and the Swift model tested on Linux (two cores over localhost); the screens not yet compiled or tried on a phone. AI asks moved to iroh and TCP removed in review step 3 (QR pairings are not carried over: trust those devices again with a code); the device key in the Keychain is review P4 |
| M2 PR 5 | Device discovery and status: each trusted device shows as nearby (Bonjour sees it), reachable (a trusted connection worked either way in the last five minutes) or offline, with when it was last reached, kept across launches (`presence.rs`); `relay/device/1` gains `StatusAsk` / `Status` (name, platform, what it offers; read-only, the device's own claim, grants nothing); "About This Device" on a device's screen with Ask Again; `relay devices` shows presence, `relay status <device>` asks ([brief](docs/relay-m1-m2-brief.md#5-device-discovery-and-status)) | Rust and the command tested on Linux (two cores over localhost); the screens compiled for iOS, not yet tried on a phone. Not yet: the Nearby / Connections / Activity screens, which follow |
| R3 | Resume, background transfers, a share extension (costs an App ID) | Later, within M1 and the plan's milestones |
| Review step 4 | Unified delivery ([plan](docs/relay-unified-delivery.md)): text to a trusted device is a message in its conversation in Chats (notes are gone; one from an older build arrives as a message), files sent to it are an attachment there (a folder opens in Files), and Relay's outbox is the one queue for both, so they keep their order. Notes already sent or received move into their device's conversation once, on the first open. The device's screen has Message and Send Files; History keeps word-code transfers and file deliveries | Rust and the command tested on Linux (order, relaunch, a lost answer, revocation, the migration from a saved record); the Swift model tested on Linux, the screens not yet compiled or tried on a phone |
| AI A1 | Ask a trusted device's language model (over `relay/device/1` since review step 3): `relay serve-ai` answers with an OpenAI-compatible server, `relay ask` asks from a terminal ([forge-ai-plan.md](../../docs/forge-ai-plan.md)) | Done in Rust and the command, tested on Linux; the app gets it with the AI Lab (A2) |

## How it works

- **Word codes** (`code.rs`, `code_send.rs`): the sender shows a code such
  as `37-grasp-gag-drum`, and its QR code; the receiver types or scans it,
  finds the sender by the code's number with Bonjour (`_forgerelay._udp`),
  and fetches over iroh with a PAKE keyed by the code.
- **Trusted devices** (`trust.rs`, `device.rs`): after a code transfer, both
  sides may trust each other by iroh endpoint id. Trusted devices talk over
  `relay/device/1` on iroh: messages, files and folders (iroh-blobs),
  status and AI asks, as CBOR messages.
- **One queue per device** (`outbox.rs`, `device_chat.rs`): messages and
  files for a trusted device wait in Relay's outbox, are sent oldest first
  and received once; both show in the device's conversation in Chats.
- **Files** are kept only if their hash matches what the sender announced.
- **Rust owns iroh; Swift owns Bonjour.** The app advertises Rust's iroh
  port and passes the addresses it browses to Rust.

## The `relay` command

```bash
cargo build --release --features cli --manifest-path examples/relay/rust/Cargo.toml
R=examples/relay/rust/target/release/relay
$R listen                 # takes what trusted devices send, then waits
$R history
$R send ~/Pictures/Trip notes.txt    # files and folders; prints a word code and its QR
$R receive <code>                     # finds the sender with mDNS on this network
$R receive-code <code> <host:port> <endpoint-id>   # without mDNS
$R devices                            # trusted and revoked devices
$R send <device> --text "Hi"          # a message to a trusted device, no code
$R messages <device>                  # its conversation
$R send <device> ~/Pictures/Trip      # files and folders, no code
$R revoke <device>                    # or forget / trust
$R policy automatic                   # codes-only | ask | automatic
```

After a code transfer in a terminal, `relay` asks whether to trust the other
device. `listen` also advertises this device for trusted devices. While it
runs, `relay send <device>`, `devices`, `revoke`, `forget`, `trust`, `policy`
and `history` from another terminal go through the listener (a socket in
the data folder), so a laptop can listen and send at the same time.

### Asking a model on another device

```bash
# On the laptop, with Ollama (or llama.cpp's llama-server, LM Studio) running:
$R serve-ai --upstream http://127.0.0.1:11434/v1 --model qwen3:8b
# From a trusted machine (a second RELAY_DATA works on the same one):
$R ask laptop --system "Be brief." "What is a night train?"
```

`serve-ai` listens like `listen`, and answers
trusted devices' asks with the server's model, one at a time; a second ask
meanwhile gets "busy". Without `--model` it takes the first model the server
lists. `$RELAY_AI_KEY`, if set, is sent as a bearer token. Only trusted devices
can ask, and the server's address is never shared, so keep the server on
localhost. The answer streams back in pieces (at most five times a second);
stopping the asker stops the model. The log records the model, the number of
pieces and the time, never the text.

On the wire (`relay/device/1`): `Ask { request }` with Forge AI's request
(OpenAI chat completions, text only), then `Delta { text }` pieces and
`Answered { model }` or `Failed { error }`; the asker may send `Stop` at any
time. A device that answers no asks refuses it.

Data lives in `~/.local/share/forge-relay` (or `$RELAY_DATA`); received
files are in its `inbox/`.

## Rust

```bash
cargo test --manifest-path examples/relay/rust/Cargo.toml
```

## Swift

| File | What |
|---|---|
| `Sources/AppHost/RelayModel.swift` | The screens' model: asks the core, trusts, sends, keeps alerts. Foundation only, tested with two cores over localhost |
| `Sources/AppHost/RelayNetwork.swift` | Bonjour (`_forgerelay._udp`): advertises code senders and this device's iroh port, finds code senders and trusted devices, resolves them to `host:port` |
| `Sources/AppHost/ContentView.swift` | Trusted devices and the history; advertises this device while on screen |
| `Sources/AppHost/TrustedDeviceViews.swift` | Trusted devices: the trust look, one device's screen (Message, which opens its conversation in Chats, Send Files, Can Send to This Device, revoke, forget), revoked devices with Trust Again, and the pairing policy |
| `Sources/AppHost/CodeQR.swift` | A word code as a QR code (Core Image) for Send, and the scanner (VisionKit) for Receive |

`forge.toml` lists `bonjour_services = ["_forgerelay._udp"]` (iOS refuses
any service type not in `NSBonjourServices`), the local network text and the
camera text for the scanner.

To try it with one iPhone, run `relay send <file>` on the laptop and scan its
QR code from the phone's Receive; trust each other when asked, then run
`relay listen` and send from either side without a code.
