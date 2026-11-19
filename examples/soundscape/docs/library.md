# Soundscape's music library

PR 2 of turning Soundscape into a personal music player (Reuben, 2026-11-16). This page is the design. The code
follows it in the same pull request.

## Today

Listen is a browser for one take server. The server is JewGrass's `tools/player/serve.py`, and its answers are
listed in `rust/src/listen.rs`. A "song" is the server's slug. Its takes are audio files, and the server's pick
says which one is preferred. The act (AFTER HOURS, LOWBRIDGE, ...) arrives as a section name, and the title
sometimes carries the act in front of it. Nothing is kept on the phone except the server's address, so the list
is empty until the server answers.

## Three things kept apart

| Thing | What it is | Where it comes from |
|---|---|---|
| **Track** | The musical work: title, artist, album, track number, genre, duration, artwork | The source's catalogue, plus your own edits on the phone |
| **Recording** | One audio version of a track: a generated take, a mix, a master, an imported file | The source, one track can have many |
| **Audio source** | Where a recording's audio lives | `TakeServer` or `LocalFile` (an imported file); a downloaded take keeps its source and plays from its copy ([offline.md](offline.md)) |

One track, many recordings. A song generated three times is one library entry with three recordings, and the
preferred recording is what plays. For a take-server track the preferred recording is the server's pick, so
choosing another one picks it on the server, and JewGrass's `picks.json` stays the single record. With no
pick, the best rated recording plays, else the newest (`preferred_take`, already in PR 1).

Artists and albums are their own entries, with their own artwork. A JewGrass act is an **artist** (each act is a
band with its own `artist-dna.yaml`). Its songs make up one **album**, named after the act unless the catalogue
names one, numbered in the catalogue's order. A song with no act gets no artist ("Unknown Artist") and no album.

## Where the library is kept

The phone keeps the library in `library.json` (a `Document`, next to the projects):

- **Snapshots:** the last catalogue each source sent, saved as it came. The library browses and searches
  without the server; a song plays without it once downloaded ([offline.md](offline.md)).
- **Edits:** your changes to a track's title, artist, album, track number or genre. They apply over the
  snapshot, so a refresh never undoes them.
- **Artwork:** pictures you chose for an artist, album or track, in Soundscape's file store. They win over the
  server's artwork. With neither, the screens draw the stand-in (initial on a colour from the title).

The browsable library (artists, albums, tracks, recordings) is built from these three each time. That build
is one pure Rust function, `build_library`, tested on Linux. Ids are stable:

- track `take-server:<slug>`;
- artist `artist:<name, lower case>`;
- album `album:<artist>/<title, lower case>`.

Because of that, an edit that moves a track to another album or artist simply creates or joins that entry.

## The take server's catalogue request

Asking for 128 songs one by one is slow, so the server gets one more request, `GET /api/catalog`. It answers
with everything in one go:

```json
{"version": 1,
 "artists": [{"id": "after-hours", "name": "AFTER HOURS", "artwork": "/art/after-hours/artist.jpg"}],
 "albums":  [{"id": "after-hours", "title": "AFTER HOURS", "artist": "after-hours", "artwork": null}],
 "tracks":  [{"slug": "ah-one-more-night", "title": "One More Night", "artist": "after-hours",
              "album": "after-hours", "number": 1, "artwork": null, "pick": "<take id>", "updated": 1763300000,
              "takes": [{"id": "...", "file": "...", "label": "seed 7202", "seconds": 225, "bpm": 112, "key": "A major",
                         "lufs": -12.1, "stars": 4, "note": "", "mtime": 1763300000, "...": "as in /api/song"}]}]}
```

- **Artwork** lives under `~/band/artwork/`:
  - `<act>/artist.jpg` for the artist;
  - `<act>/album.jpg` for the album;
  - `<act>/<song slug>.jpg` for one track;
  - `<song slug>.jpg` for a track with no act.

  PNG works too. The server serves only files it found there, at `/art/...`.
- An older server without `/api/catalog` still works. The phone falls back to `/api/library` plus one
  `/api/song` per song, and builds artists and albums from the acts.

## What the app shows

The Listen tab becomes **Library**:

- Artists, Albums and Songs, with search across all three.
- An artist shows its albums.
- An album shows its artwork, its tracks in order, and Play and Shuffle.
- A track shows its recordings, with the preferred one marked and Use This Recording on the others. Edit
  changes its details and its artwork (from Photos), and Takes and Compare opens the existing A/B screen.
- Everything plays through PR 1's queue. Each track plays its preferred recording, and Now Playing shows the
  artist, album and artwork.

## Ordinary music files (decided: yes)

The library will hold your own audio files as well as generated music. The model has room for them (an
`Imported` recording whose source is a `LocalFile`, with tags from the file). Importing comes with offline
storage in PR 4, because both need files on the phone: see [offline.md](offline.md).

## Not in this PR

- Favourites, playlists and history (PR 3).
- Downloads and importing files (PR 4).
- Deeper take and version tools (PR 5, below).
- Removing Studio (PR 6).
- Streaming services, cloud sync, tagging and recommendations: not planned.

## Takes and versions

PR 5 makes a generated song's takes part of the library rather than a separate Listen screen:

- **On the song's screen** each take shows its stars (tap to rate, tap the same star to clear) and its note, and
  the takes can be listed **best** first (the one that plays, then most stars, then newest), **newest** or
  **oldest** first. Ratings, notes and picks go to the take server, so JewGrass's `picks.json` stays the record.
- **Compare Takes** on the song opens the A/B comparison (matched loudness, Blind hides which is which).
- **Now Playing** shows the playing take's stars, to rate it while listening.
- **To Pick**, in Your Music, lists songs with two or more takes and no pick yet, newest first.

The rules (`to_pick`, `sort_recordings`, `mark_recording`) are Rust, tested on Linux. Imported files have one
recording and no ratings.
