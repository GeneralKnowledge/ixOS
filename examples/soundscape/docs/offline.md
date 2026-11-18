# Offline: downloads and your own music files

PR 4 of turning Soundscape into a personal music player (plan in [library.md](library.md)). Until now a song
needed the take server to play. This PR keeps songs on the phone, so they play on a train or out of reach of
the laptop, and lets you bring in ordinary music files of your own.

## Downloads

You choose **what** to keep, not which file: a song, an album or a playlist (its songs at the time). The phone
then keeps the recording that plays for each of those songs, the preferred one. When the preferred recording
changes (you pick another take), the new one is downloaded on the next pass and the old file is removed.
Choosing several songs at once (an album) downloads them in their order.

`offline.json` (a `Document`, next to `library.json`) keeps:

- **Wanted:** the song ids you chose, with when.
- **Files:** each downloaded recording, by song and recording: its file name in the `downloads` store, its size, when it
  arrived and which song it was for.

Downloading runs one file at a time through the app's Rust networking (URLSession on the phone), streamed
straight to disk, and the file is kept only once all of it has arrived. A file that fails is noted and tried
again on the next pass; it never stops the others. A song's row shows a small arrow once it is on the phone.

A downloaded recording plays from the phone: its `audio_url` becomes the file, wherever it is played from
(the library, a playlist, the queue). Where the audio came from (`source`) stays as it was.

The rules are Rust, in `rust/src/offline.rs`, tested on Linux: what to fetch and what to remove (`plan`), what
each song's state is (not downloaded, waiting, downloading, on the phone, failed), and swapping in the local
file.

**Only where the source allows it.** Only take-server recordings are downloaded today. When catalogue sources
come ([sources.md](sources.md)), each recording carries whether it may be downloaded, and `plan` leaves out any
that may not.

## Your own files

**Import Music** in the Library opens Files. Pick any audio files (MP3, AAC/M4A, ALAC, WAV, AIFF, FLAC, CAF);
each is copied into Soundscape, so it stays even if the original is moved. The phone reads the file's tags
(title, artist, album, track number, genre, length, cover picture) with AVFoundation and Rust keeps them in
`library.json` under `imported`. A file without a title is named after the file.

An imported file is a track like any other: it joins its album and artist, can be edited, given artwork, made
a favourite and put in playlists. Its id is `file:<first 16 characters of its SHA-256>`, so importing the same
file twice adds it once. Its only recording is `Imported`, with a `LocalFile` source. **Remove from Library** on
its screen deletes the copy.

## What the app shows

- A song's menu and screen: **Download** or **Remove Download**. A playlist's More menu: **Download** for all
  of its songs.
- An album: a **Download Album** button under Play and Shuffle (showing "Downloading 3 of 12…" while it
  comes), **Download** in its More menu, and **Download** when you long-press it in a list (Reuben asked for a
  whole-album download on 2026-11-18; the menu item alone was too hidden).
- **Your Music** gets **Downloaded**: what is on the phone and what is still coming, the space it takes, and
  **Remove All Downloads**.
- The Library toolbar gets **Import Music**.

## Not in this PR

Downloading in the background after the app has closed (the `transfers` service could do this later), a
Wi-Fi-only setting, keeping server artwork offline, and syncing your files back to the laptop.
