# Relay stage 0 spike

**Not production code.** Throwaway code that answers the stage 0 questions in
[examples/relay/docs/relay-transfer-plan.md](../docs/relay-transfer-plan.md): does a
croc-style word code work on iroh, offline, on a local network? Results are in
[examples/relay/docs/relay-stage0-spike.md](../docs/relay-stage0-spike.md).
The review's limitations (code collisions, codes burnt before the proof, 16-bit
codes, memory stores) are fixed here and tested in the results. M1 rebuilds this inside
`relay_core`; then this folder goes.

```bash
cargo build --release --manifest-path examples/relay/spike/Cargo.toml
S=examples/relay/spike/target/release/relay-spike
$S send holiday.zip            # prints Code: 7-acid-tidy-rug
$S receive 7-acid-tidy-rug     # on another machine on the same network; saves to ./inbox/
```

The receiver finds the sender by mDNS (`_forgerelay._udp`). Where multicast
doesn't reach, the sender also prints a `--addr … --id …` line to add.
`relay-spike steal <hash> --addr … --id …` is a test: a stranger who knows
the file's hash but not the code, who must be refused.

The library target (`src/lib.rs`) exists only to build the same stack as an
iOS static library on the laptop:
`cargo build --release --lib --target aarch64-apple-ios` (through
`forge queue run`), which needs the iOS SDK that xtool installs.

Both sides keep blobs in `--store` (default `./.relay-spike-store`). If a
transfer stops, the sender prints a new code; receiving with it from the
same folder fetches only what is missing. The code words are the EFF short
word list 1 (`src/eff_short_wordlist_1.txt`, CC BY 3.0 US, EFF).
