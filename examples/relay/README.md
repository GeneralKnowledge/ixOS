# Relay

**Your devices. One network.** Private connectivity for the things you own
and the people you trust. Relay starts by moving files between your devices
and grows into a personal network; the roadmap is
[docs/relay-platform-plan.md](docs/relay-platform-plan.md), and the next
work is [docs/relay-m1-m2-brief.md](docs/relay-m1-m2-brief.md).

Today it sends notes and files between nearby devices, after pairing once by
QR code, and sends files, photos or a folder to anyone on the same Wi-Fi with a word code.
Chat merged into it on 2026-11-17 ([relay-chat-merge.md](docs/relay-chat-merge.md)):
the Chats, Assistant and Settings tabs are Chat's messaging through a chat
server you run, with an on-device assistant ([messaging.md](docs/messaging.md));
Devices is Relay's own screen. It
runs inside Device Lab (Apps, then Relay) and on its own. The second device
can be a Linux machine running the `relay` command, so one iPhone is enough.

## Where it is

| Step | What | State |
|---|---|---|
| R1 | Rust core (`rust/`): identity, QR pairing, the encrypted protocol, sending and receiving notes and files, history; the `relay` command | Done, tested on Linux over localhost |
| R2 | SwiftUI app: pair by scanning (VisionKit) or showing a QR code, Bonjour (NetService advertises Rust's port, NWBrowser finds devices), notes and files from the file picker, received files in Quick Look, hosted in Device Lab | Built with xtool and installed on an iPhone in Device Lab (2026-11-15); pairing, notes and Quick Look not yet tried on the phone. Secrets stay in Rust's file (friction A17) |
| M1 start | Word codes on iroh (`code.rs`, `code_send.rs`): "Receive with a Code" and "Send with a Code" in the app, `relay send-code` on Linux, `relay-spike` from [spike/](spike/) | Passed on an iPhone 15 Pro Max on 2026-11-15: 1 GB Linux to iPhone at 9.5 MB/s and iPhone to Linux, each at the first code and verified ([results](docs/relay-stage0-spike.md#iphone-start-of-m1-2026-11-15)). Not yet a production transfer: no folders, no history, separate from paired transfers |
| M1 PR 1 | The transfer core: one iroh endpoint per device with a key kept in `device.key` (0600), one blob store in `codes/store` for sending and receiving, folders as one transfer (an iroh-blobs collection), resume after a relaunch, both directions in the history, stopping the sender stops the receiver ([brief](docs/relay-m1-m2-brief.md#1-production-transfer-core)) | Done on Linux (tests: a folder both ways, a transfer stopped halfway resumes after a relaunch and verifies); not yet on a phone |
| M1 PR 2 | Send and Receive in the app's bottom bar (several files, photos or a folder with one word code; a progress bar and Stop; a folder opens in the Files app), received files in Documents/Relay where the Files app shows them (On My iPhone), and `relay send PATH...` / `relay receive <code>` finding each other with mDNS ([brief](docs/relay-m1-m2-brief.md#2-native-transfer-screens-and-cli)) | Rust and the command done and tested on Linux (a folder both ways between two `relay` commands); the Swift is not yet compiled or tried on a phone |
| M2 PR 3a | Trusted devices: after a code transfer each side is asked "Trust this device?" (both learn the other's name); a trusted device gets notes, files and folders with no code over `relay/device/1` on iroh, found by Bonjour (`_forgerelay._udp`, TXT `d=1`); revoke (refused for good, codes never restore it, only the person trusts it again), forget, "Can Send to This Device", and the pairing policy (Ask to Trust, Trust Again Automatically, Codes Only); `relay devices`, `trust`, `revoke`, `forget`, `policy`, `send <device>` ([brief](docs/relay-m1-m2-brief.md#3-trusted-devices)) | Rust, the command and the Swift model tested on Linux (two cores over localhost); the screens not yet compiled or tried on a phone. Next (3b): AI asks on iroh, QR pairings carried over, TCP removed, the device key in the Keychain |
| R3 | Resume, background transfers, a share extension (costs an App ID) | Later, within M1 and the plan's milestones |
| AI A1 | Ask a paired device's language model (protocol 2): `relay serve-ai` answers with an OpenAI-compatible server, `relay ask` asks from a terminal ([forge-ai-plan.md](../../docs/forge-ai-plan.md)) | Done in Rust and the command, tested on Linux; the app gets it with the AI Lab (A2) |

## How it works

- **Pairing.** The listening device shows a code
  (`forgerelay://pair?v=1&id=…&name=…&key=…&at=host:port`) as a QR code.
  The other scans it and calls `pair`, which connects and runs a Noise
  `XXpsk3` handshake keyed by the code's one-time secret. The secret is
  never sent; a wrong or used code fails the handshake. Both sides keep it
  as the pair's secret, and every later connection is keyed by it.
- **The wire.** Reused from the device tools (`forge-devproto`):
  length-prefixed frames, the Noise channel, CBOR messages. Relay adds a
  cleartext intro (who is calling, pairing or not), then Hello, Offer,
  Accept or Decline, Chunks of 32 KB, Done, Received; and, from protocol 2,
  asks for the other device's model ([below](#asking-a-model-on-another-device)).
- **Files** are kept only if their size and SHA-256 match what the sender
  announced, as `inbox/<sha256>/<name>`, so the name and extension survive.
- **Rust owns TCP; Swift owns Bonjour.** `start_listening` opens the port
  from Rust; the app advertises it as `_forgerelay._tcp` and passes the
  addresses it browses to `send_note` and `send_file`.

## The `relay` command

```bash
cargo build --release --features cli --manifest-path examples/relay/rust/Cargo.toml
R=examples/relay/rust/target/release/relay
$R listen                 # prints a QR code for the phone to scan, then waits
$R pair '<code>'          # or pair with a code the phone shows
$R peers
$R send <name> --note "Hello from the laptop"
$R send <name> ~/Pictures/harbour.jpg
$R history
$R send ~/Pictures/Trip notes.txt    # files and folders; prints a word code
$R receive <code>                     # finds the sender with mDNS on this network
$R receive-code <code> <host:port> <endpoint-id>   # without mDNS
$R devices                            # trusted and revoked devices
$R send <device> --note "Hi"          # to a trusted device, no code
$R send <device> ~/Pictures/Trip      # files and folders, no code
$R revoke <device>                    # or forget / trust
$R policy automatic                   # codes-only | ask | automatic
```

After a code transfer in a terminal, `relay` asks whether to trust the other
device. `listen` also advertises this device for trusted devices.

### Asking a model on another device

```bash
# On the laptop, with Ollama (or llama.cpp's llama-server, LM Studio) running:
$R serve-ai --upstream http://127.0.0.1:11434/v1 --model qwen3:8b
# From another paired machine (a second RELAY_DATA works on the same one):
$R ask laptop --system "Be brief." "What is a night train?"
```

`serve-ai` listens like `listen` (and prints a pairing code), and answers
paired devices' asks with the server's model, one at a time; a second ask
meanwhile gets "busy". Without `--model` it takes the first model the server
lists. `$RELAY_AI_KEY`, if set, is sent as a bearer token. Only paired devices
can ask, and the server's address is never shared, so keep the server on
localhost. The answer streams back in pieces (at most five times a second);
stopping the asker stops the model. The log records the model, the number of
pieces and the time, never the text.

On the wire (protocol 2): `Ask { request }` with Forge AI's request
(OpenAI chat completions, text only), then `Delta { text }` pieces and
`Answered { model }` or `Failed { error }`; the asker may send `Stop` at any
time. A device that answers no asks sends `Decline`.

Data lives in `~/.local/share/forge-relay` (or `$RELAY_DATA`); received
files are in its `inbox/`.

## Rust

```bash
cargo test --manifest-path examples/relay/rust/Cargo.toml
```

## Swift

| File | What |
|---|---|
| `Sources/AppHost/RelayModel.swift` | The screens' model: asks the core, pairs, sends, keeps alerts. Foundation only, tested with two cores over localhost |
| `Sources/AppHost/RelayNetwork.swift` | Bonjour: advertises Rust's port as `_forgerelay._tcp` named by the device id, browses for paired devices, resolves one to `host:port`; this device's local addresses for a pairing code |
| `Sources/AppHost/ContentView.swift` | Paired devices (nearby or not) and the history; listens while on screen |
| `Sources/AppHost/TrustedDeviceViews.swift` | Trusted devices: the trust look, one device's screen (send a note or files, Can Send to This Device, revoke, forget), revoked devices with Trust Again, and the pairing policy |
| `Sources/AppHost/PeerView.swift` | One device: write a note, pick a file, what went between you |
| `Sources/AppHost/PairingViews.swift` | Show my code (a Core Image QR code) and scan a code (VisionKit), or paste one |

`forge.toml` lists `bonjour_services = ["_forgerelay._tcp"]` (iOS refuses
any service type not in `NSBonjourServices`), the local network text and the
camera text for the scanner.

To try it with one iPhone, run `relay listen` on the laptop and scan its code
from the phone, or show the phone's code and paste it into `relay pair`.
