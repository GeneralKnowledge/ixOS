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
| F2a | The app: trips list, Record, live map and totals, text notes, trip screen with Share as GPX or GeoJSON; the core follows the location service | Written; Swift checked by the laptop build and macOS CI only |
| F2b | Hosted in Device Lab (Apps, FieldKit) | Done; not run on an iPhone yet |
| F2c | Photo and voice notes, with Chat's photo picker moved into the framework | Next |
| F3 | GPX and GeoJSON export (Rust done early), charts, background recording | Export written and tested on Linux; the rest not started |

## The app

- **Trips**: every trip, newest first, with the one being recorded on top.
- **Record**: name the trip and start. FieldKit asks for location the first
  time. The recording screen shows the track on Apple Maps, the distance,
  time and climb, and has Add Note and Stop.
- **A trip**: its map with notes as pins, its totals and notes, and Share as
  GPX or GeoJSON, Rename and Delete.

Recording happens while FieldKit is on screen; background recording is F3.

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
- **Export** to GPX 1.1 and GeoJSON, with placed notes as waypoints.
- **Follows location itself** while a trip records, through the location
  native service (`native_services!(location)`), and says why when it
  can't (permission denied, no service).
- Swift owns the camera and the microphone and passes their files in.

```bash
cargo test --manifest-path examples/fieldkit/rust/Cargo.toml
./scripts/check-app-crates.sh examples/fieldkit
```

What building it taught us about the framework is in
[docs/research/app-friction-log.md](../../docs/research/app-friction-log.md).
