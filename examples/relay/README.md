# Relay

Send notes and files between nearby devices, encrypted, after pairing once
by QR code. The last of the remaining demo apps
([friction log](../../docs/research/app-friction-log.md)); it runs inside
Device Lab (Apps, then Relay) and on its own. The second device can be a Linux
machine running the `relay` command, so one iPhone is enough.

## Where it is

| Step | What | State |
|---|---|---|
| R1 | Rust core (`rust/`): identity, QR pairing, the encrypted protocol, sending and receiving notes and files, history; the `relay` command | Done, tested on Linux over localhost |
| R2 | SwiftUI app: pair by scanning (VisionKit) or showing a QR code, Bonjour (NetService advertises Rust's port, NWBrowser finds devices), notes and files from the file picker, received files in Quick Look, hosted in Device Lab | Written; not yet built with xtool or run on a phone. Secrets stay in Rust's file (friction A17) |
| R3 | Resume, background transfers, a share extension (costs an App ID) | Later |
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
```

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
| `Sources/AppHost/PeerView.swift` | One device: write a note, pick a file, what went between you |
| `Sources/AppHost/PairingViews.swift` | Show my code (a Core Image QR code) and scan a code (VisionKit), or paste one |

`forge.toml` lists `bonjour_services = ["_forgerelay._tcp"]` (iOS refuses
any service type not in `NSBonjourServices`), the local network text and the
camera text for the scanner.

To try it with one iPhone, run `relay listen` on the laptop and scan its code
from the phone, or show the phone's code and paste it into `relay pair`.
