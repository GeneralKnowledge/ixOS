# Your collection: favourites, playlists and history

PR 3 of turning Soundscape into a personal music player (plan in [library.md](library.md)). The library is
what the sources have. The collection is what you did with it: what you love, the lists you made, and what you
played. It never goes to the take server.

## What is kept

`collection.json` (a `Document`, next to `library.json`), apart from the library so a library that has to start
again never takes your playlists with it:

- **Favourites:** library ids (`take-server:<slug>`, `album:...`, `artist:...`) with when you added them. A
  song, an album or an artist can be a favourite.
- **Playlists:** a name and a list of track ids, in your order. The same song can be in a playlist more than
  once. Up to 500 playlists of up to 5,000 songs.
- **History:** each song you played, with the recording and the time, newest last, the last 2,000 kept. A song
  counts as played after 30 seconds, or half of it when it is shorter (an hour of skipping isn't history).

Everything is by id, so it follows a track through edits and refreshes. A track that leaves the library stays in
the file but isn't shown (a playlist says "2 songs not in your library"), and comes back if the track does.

## What the app shows

The Library screen gets a **Your Music** row above Songs, Albums and Artists:

- **Favourites:** your favourite songs, then albums, then artists. A heart on every song, album and artist
  screen, and in the Now Playing sheet, adds or removes it.
- **Playlists:** new, rename, delete (a swipe in the list, or Delete Playlist in its More menu); add a song from
  its screen, its row's menu or Now Playing; add a whole album; reorder by editing; remove a song by editing, a
  swipe or its menu. Play and Shuffle like an album.
- **Recently Played:** the last 50 different songs, newest first, with when; clear history from there.

The Library's home also shows the last few songs you played, so getting back to them is one tap.

The rules (dedupe, limits, what counts as a play, building the lists) are Rust, in `rust/src/collection.rs`,
tested on Linux.

## Lyrics

Reuben asked for lyrics on the songs that have them (55 of 128 on the take server). The takes carry lyrics in
their sidecars, with section tags like `[verse]` and no timings, and takes of one song can differ, so each
recording carries its own. Now Playing gets a **Lyrics** button for the playing take, and a song's screen shows
its preferred recording's. `lyric_lines` (Rust, `rust/src/lyrics.rs`) turns tags into headings and blank lines
into one gap.

**Following the song.** Reuben's song generator will add timings (2026-11-17). They come as LRC: a `.lrc` file
next to the take, or `"lrc"` in its sidecar, which the take server sends as `lrc` (JewGrass PR #30), and the
phone prefers it over the plain lyrics. Line stamps (`[01:23.40] One more night`) light up the line being sung
and keep it in view; enhanced LRC word stamps (`[00:12.30] <00:12.30>One <00:12.61>more <00:12.90>night`) light
up each word as it is sung. Tapping a line plays from it. Without timings, the lyrics read as before.

## Not in this PR

Smart playlists (now built: [roadmap.md](roadmap.md#m10-as-built)), play counts on rows, sharing playlists, and import
from other apps.
