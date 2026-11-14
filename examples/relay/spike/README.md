# Relay stage 0 spike

Throwaway code that answers the stage 0 questions in
[docs/relay-transfer-plan.md](../../../docs/relay-transfer-plan.md): does a
croc-style word code work on iroh, offline, on a local network? Results are in
[docs/research/relay-stage0-spike.md](../../../docs/research/relay-stage0-spike.md).
Stage 1 rebuilds this inside `relay_core`; then this folder goes.

```bash
cargo build --release --manifest-path examples/relay/spike/Cargo.toml
S=examples/relay/spike/target/release/relay-spike
$S send holiday.zip          # prints Code: 7-orbit-velvet
$S receive 7-orbit-velvet    # on another machine on the same network; saves to ./inbox/
```

The receiver finds the sender by mDNS (`_forgerelay._udp`). Where multicast
doesn't reach, the sender also prints a `--addr … --id …` line to add.
`relay-spike steal <hash> --addr … --id …` is a test: a stranger who knows
the file's hash but not the code, who must be refused.

The library target (`src/lib.rs`) exists only to build the same stack as an
iOS static library on the laptop:
`cargo build --release --lib --target aarch64-apple-ios` (through
`forge queue run`), which needs the iOS SDK that xtool installs.
