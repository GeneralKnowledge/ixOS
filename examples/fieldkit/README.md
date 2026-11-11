# FieldKit

A field notebook: record a walk, ride or site visit as a GPS track, note
things along the way (text, photos, voice memos), see it on a map, and
export it. Demo three of the [capability ledger](../../docs/capability-ledger.md)'s
sequence, built as a hostable app so it can also run inside the Device Lab
showcase.

## Status

| Milestone | What | State |
|---|---|---|
| F1 | Rust core: trips, track filtering and totals, notes with photo and voice files, SQLite | This folder's `rust/`; tested on Linux |
| F2 | The app: SwiftUI screens, the location native service, MapKit, camera, voice notes; hosted in Device Lab | Waits for the showcase's hostable-app fixes (#167) |
| F3 | GPX and GeoJSON export, background recording | Not started |

## The Rust core

`rust/` is FieldKit's own crate (`fieldkit_core`). It owns what a trip is:

- **Track rules** (`src/track.rs`): a fix is kept only if it is a real
  place, accurate to 50 m, later than the last kept fix, at least 3 m from
  it and not an impossible jump (over 80 m/s). Distance and climb (ignoring
  altitude wobble under 3 m) are kept as running totals.
- **Recording survives the process.** A trip has no end until it is
  stopped, so if iOS ends the app mid-walk, the next launch finds it still
  recording.
- **Notes** are placed at the last kept fix while recording. Photos and
  voice memos are copied into a content-addressed store and deleted with
  the last note that uses them.
- Swift owns Core Location, the camera and the microphone, and passes the
  results in.

```bash
cargo test --manifest-path examples/fieldkit/rust/Cargo.toml
./scripts/check-app-crates.sh examples/fieldkit
```

What building it taught us about the framework is in
[docs/research/app-friction-log.md](../../docs/research/app-friction-log.md).
