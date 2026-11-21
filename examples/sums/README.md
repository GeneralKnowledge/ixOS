# Sums

A calculator rebuilt from the outside. Sums is the recreation in the Calculator expedition ([plan](../../docs/research/app-as-specification.md), [expedition folder](../../docs/research/expeditions/calculator/)): we watched the iPhone's own calculator through its accessibility tree, recorded its answers to 39 sums, and rebuilt the behaviour from that evidence alone. The look is Sums' own; only the behaviour follows the reference.

## Layout

| Path | What |
|---|---|
| `rust/src/engine.rs` | Every rule: keys in, display and expression line out. Where the evidence ran out, a rule says `GUESS:` |
| `rust/src/lib.rs` | The UniFFI API (`SumsCore`): each key press is a command on the forge-app runtime, and the keys since the last All Clear are saved, so a sum survives a relaunch |
| `rust/tests/reference.rs` | The reference's 39 recorded answers as a test, on Linux |
| `rust/examples/predict.rs` | The engine's answers to a sequence file, written before the reference is asked |
| `Sources/AppHost/` | The SwiftUI screen (`ContentView.swift`) and its view model (`SumsModel.swift`) |

## Checking it

```bash
cd rust && cargo test     # includes "39 of 39 sequences match the reference"
```

On the phone, Sums runs inside Device Lab (Apps, Sums). Every key has the identifier `sums.key.<name>`, where the name is the reference's key id in lower case (`Seven` gives `sums.key.seven`), and the display is `sums.display`, so one sequence table drives both calculators (`run_sequences.py` in the expedition folder).
