# Soundscape architecture: one library across phone, server and sources

Reuben's direction (2026-11-18): Soundscape becomes an independent, personal music platform. It is one
library and one listening experience across music on the iPhone, music on your own server (streamed or
downloaded), recordings from JewGrass and other creative tools, and later external catalogues and other
devices. The player never cares where a song comes from.

This document checks that proposal against the code as it is on main (2026-11-18). For each part it says
whether to keep, extend or replace it, then gives the work in order. The exact rules for the earlier pieces are
in [library.md](library.md), [collection.md](collection.md), [offline.md](offline.md) and
[sources.md](sources.md).

## Where we are

| Area | Today | Phone |
|---|---|---|
| Player (M1) | One `TakePlayer` (AVPlayer) owned by the Listen screen. It has a queue, shuffle, repeat, Play Next, Add to Queue, a mini-player, Now Playing, lock-screen controls and interruptions | Passed 2026-11-17 |
| Library (M2) | `library.rs`: Track, Recording and AudioSource kept apart. `library.json` holds the server snapshot, your edits, your artwork and imported files | Passed 2026-11-17 |
| Collection (M3) | `collection.rs`: favourites, playlists and history in `collection.json`. `lyrics.rs` handles plain lyrics and LRC | Passed 2026-11-18; #328 fixes to recheck |
| Offline (M4) | `offline.rs`: songs chosen for the phone, downloads, `offline.json`. Import Music copies files in with their tags | Passed 2026-11-18 |
| Takes (M5) | #334: rating and notes on the song, sort order, To Pick, stars in Now Playing. Blind A/B already existed | Not built yet |
| Server | JewGrass `tools/player/serve.py`: `/api/library`, `/api/song`, `/api/catalog`, `/api/pick`, and `/audio/…` with byte ranges (206). Picks, stars and notes are in `~/band/picks.json` | In use |
| Studio | The Projects tab (`project.rs`, `dsp.rs`, LoopEngine, ProjectView, TrackStrip, about 2,300 lines) was the first tab | Removed 2026-11-18 |

The proposal's P0, finishing the player that's already written and proving it on the phone, is nearly done. The
#328 and #334 checks are queued for the next combined Device Lab build.

## Keep, extend, replace

| Part | Decision | Why |
|---|---|---|
| Rust domain core (`soundscape_core`) | **Keep** | It already owns the library rules, queue rules, collection, offline plan and lyrics, all tested on Linux |
| SwiftUI + AVPlayer + AVAudioSession | **Keep** | AVPlayer plays both local files and HTTP streams. No custom audio engine until a measured limit needs one |
| `TakePlayer` as the one playback coordinator | **Keep, extend** | There is already one player for the whole Listen stack. Add: a persisted queue and position, the source and buffering state, and an error that says why a track can't play |
| Track / Recording / AudioSource | **Keep, extend** | It's the right model. Recording ids need to become stable Soundscape ids, and a recording can be at more than one source (below) |
| `library.json` snapshot + edits | **Keep for now** | It's fine at today's size, about 130 songs. Move to SQLite (`forge-storage` already has the `sqlite` feature) when the library passes a few thousand recordings or search gets slow. The document becomes the import format, not the store |
| Track ids by prefix (`take-server:<slug>`, `file:<hash>`) | **Extend** | The prefix picks the owning store: that's the resolver in [sources.md](sources.md). The slug isn't an identity, though: a renamed song would come back as a new track. Add a stable id the server gives (below) |
| `offline.rs` downloads | **Keep, extend** | It already downloads by track and keeps the preferred recording, replacing it after a new pick. Add: checksum verification (the server sends `sha256`), downloads that carry on in the background (forge-services `transfers`), and a storage limit |
| Imported files | **Keep, extend** | They're already deduplicated by content hash. Reconciliation (below) links an imported file to the same recording on the server |
| Collection (`collection.json`) | **Keep** | It's personal and on the device, as the proposal says. Shared playlists come with sync, later |
| Ratings, notes, picks | **Keep on the server** | The proposal puts personal ratings on the device first. Here they already live in the server's `picks.json`, which JewGrass's tools read. Keep that: it's the record his song tools use |
| Take server (`serve.py`) | **Extend, don't replace** | Keep its endpoints and add `/v1` next to them (below). It already does byte ranges and refuses paths it didn't list |
| Studio (Projects tab) | **Replace (remove)** | It's PR 6 of the earlier plan. The proposal's five-destination navigation has no place for it |
| Navigation | **Replace** | Today: two tabs, Projects and Library. Proposed: Listen, Library, Search, Collections, Sources, with the mini-player throughout |

## The model, extended

What exists stays. These are the additions, each with where it lives.

- **Recording identity.** Every recording gets a stable Soundscape id, which never changes when a file moves or a
  slug is renamed:
  - **Server recordings:** the id the server gives in `/v1` (a UUID kept beside the take, in its sidecar).
  - **Files with no server id:** `sha256:<hash of the audio>`, which is already how imports are deduplicated.

  Source-specific ids (slug, take id, Jamendo id) move into a mapping table, `RecordingSource`.
- **`Source`:** a configured location: this phone, your server, Jamendo, Internet Archive. Each has a status
  (reachable, signed in, removed) and what it supports: streaming, download, seeking, offline. Rust, in a new
  `sources.rs`.
- **`RecordingSource`:** that recording at that source, with its URL or file, codec, size and checksum. It
  also keeps the source's rights as separate facts: may stream, may download, attribution, may adapt (from
  [sources.md](sources.md)).
- **`Download`:** today's `offline.json` entry, plus whether its checksum was verified.
- **`PlaybackEvent`:** today's history entry, plus skips and completions. It stays on the device and its events
  are merged later.
- **`LibraryDevice`:** only when sync arrives (P3).

**Choosing what plays.** One Rust function, `resolve(recording, sources, preference)`, decides what plays:

1. A verified local copy, unless you chose another source.
2. Otherwise your server.
3. Otherwise another source that allows streaming.
4. Otherwise "unavailable", with the reason ("server unreachable", "not allowed offline", "file missing").

It never silently skips or substitutes. A setting "prefer server quality" reverses steps 1 and 2. The function
is pure, so it's tested on Linux with every combination.

**Reconciliation.** When an imported file's hash matches a server recording's `sha256`, both become one recording
at two sources, not two songs. When they don't match but the title, artist and length do, Soundscape offers to
link them and never merges by itself.

### Reconciliation, as built

Built before stable ids and the resolver (step 5 and 6), on today's track ids, so it needs no schema bump:

- **Offered, never automatic.** `merge::duplicates` groups songs linked to the same MusicBrainz recording, then songs
  with the same title and artist (letters and digits only, case ignored) whose lengths are within 3 seconds or
  unknown (a song of unknown length joins only when the known lengths agree, so it never ties a 200 s and a
  400 s version together). The take server's song comes first, then a catalogue's, then your file; the first is the
  song kept. **Keep Apart** remembers the group (`library.json` `distinct`) so it isn't offered again.
- **Merging** (`merge_songs`) records each absorbed track id with the song it joined (`library.json` `merged`);
  merges of merges collapse. The absorbed songs leave the lists, and their recordings show under the kept song
  with `merged_from` naming where each came from. Nothing is deleted.
- **What plays:** a recording the person chose from one of the merged songs (`library.json` `prefer`, set by
  Choose on a take that came with a merge), else your own file when the merged song has one (resolver step 1),
  else the kept song's own preferred recording.
- **What follows the song:** favourites, playlist entries, history and songs wanted offline move to the kept song
  at the merge. The library is saved first; if a later save fails, opening the app finishes the move
  (`merge::repair_collection`, `repair_offline`). Downloads, picks, stars and notes stay with the song that owns the recording (the core finds it
  through `merged_from`), so picking a take merged into a Jamendo song still goes to the take server.
- **Separate** (`separate_song`) removes the entry and the song is back as it was; moved favourites and playlist
  entries stay with the kept song, and so do songs that had been merged into it before it was merged (merges
  collapse, so they belong to the kept song).
- Not yet: matching by file hash, which waits for the server's `sha256` (step 7).

## The server

Grow `serve.py` rather than start a new server:

| Endpoint | Today | Add |
|---|---|---|
| `GET /api/catalog` | The whole catalogue | Keep, for old builds |
| `GET /v1/library?since=` | | Paged; recordings with stable ids, `sha256`, size, codec |
| `GET /v1/recordings/{id}/stream` | `/audio/<file>` with ranges | Same file by id; ranges kept |
| `POST /v1/imports` | | Upload a file from the phone (checksum checked); it lands in an `imports/` folder, never next to takes |
| `POST /api/pick` | Picks, stars, notes | Keep |
| `GET/PUT /v1/collections` | | With sync (P3) |
| Device auth | None (Tailscale only) | A per-device token, once the server is reachable beyond Tailscale |

One open question for Reuben: does "your own server" stay the JewGrass take server, or does it become its own
small service (its own repo) that JewGrass is one source of? The plan below works either way up to P2. The
endpoints would be the same.

## Work in order

Each step is one PR with its tests and a short phone check, as before.

**Reordered 2026-11-18.** Reuben: "I don't care about the music gen right now." Real music from free
catalogues comes first, so Soundscape can be tested with full-length tracks. Jamendo is built
([sources.md](sources.md#jamendo-as-built)); the generation side (takes, picks, `/v1` on the JewGrass server,
timed lyrics) waits, and the question of where "your own server" lives waits with it.

1. **Jamendo**, **Internet Archive** and **MusicBrainz** matching (all built; phone checks next), then a fixed test
   catalogue (built: [sources.md](sources.md#test-catalogue-as-built)).
2. **P0, finish what's written.** Recheck #328 and #334 on the phone. The other P0 items are already done.
3. **Remove Studio** (old PR 6). Done: `project.rs`, `dsp.rs`, the Studio screens and the microphone permission are
   gone, and Soundscape opens straight on the library (one stack, no tab bar: Discover and Your Music are rows at
   its top). Old Studio files stay on the phone untouched. A tab bar (Library, Discover, Search) can come later,
   once the mini-player is moved above it.
4. **Persist the queue.** Done: `rust/src/playback.rs` keeps the queue, the song and how far into it in
   `playback.json`, saved as it changes, on pause, and every 5 seconds while playing. The
   app opens paused where it was left. Songs come back as the library has them now (edits, a download since); a
   file on the phone that is gone is left out and playback moves to the next song.
5. **Stable recording ids and `RecordingSource`.** The model, a schema version bump for `library.json` that
   migrates today's ids, and tests for the five resolver cases in [sources.md](sources.md).
6. **The resolver.** `resolve` in Rust; the player asks it what to play and shows why when nothing can. It
   includes the proposal's test: play from the server, lock, lose the network, reconnect, download, restart; the
   same track, recording and queue remain.
7. **Server `/v1` (JewGrass), when generation is back in focus.** Stable ids, `sha256`, `/v1/library` and `/v1/recordings/{id}/stream`, next to the
   old endpoints.
8. **Reconciliation.** Imported files link to server recordings by hash. Downloads verify their checksum.
9. **Upload from the phone** (`POST /v1/imports`).
10. **Background downloads** (`transfers`) and a storage limit.
11. **Playback quality:** sleep timer (done), gapless and crossfade (done), then loudness
    levelling, which first needs the phone to measure songs from catalogues and files; each proven on the phone.
12. **Sync between devices:** versioned records and an operation log, not a distributed database.

Not planned, as the proposal says: replacing Spotify, Apple Music as the model, a custom audio engine,
recommendations before there is enough listening history, automatic upload or download of everything, and a
large media server.
