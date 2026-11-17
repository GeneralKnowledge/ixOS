# Music sources: free catalogues next to your own music (plan)

Reuben, 2026-11-17. This is the first expansion after the six music-player PRs (player, library, collection,
offline, creation, removing Studio). It is a plan only, and no code exists for it yet.

## The idea

Soundscape stays your own self-hosted player. Free catalogues are optional **sources** next to it. They are not
dependencies, and they don't replace the take server or a home music server. Your generated and personal music
stays in your own library.

| Section | What |
|---|---|
| My Music | Your take server and your own files, as now |
| Discover | Search and browse the free catalogues |
| Playlists | Mix your tracks and catalogue tracks |
| Offline | Download, only where the track's licence allows it |

Everything plays through the same player and queue.

## Sources, in order

1. **Jamendo**
   - Independent music with an official API: search, artists, albums, playlists.
   - The API needs a `client_id`. Reuben registers it at developer.jamendo.com. It is free for non-commercial apps, up to 35,000 requests a month.
   - Each track has its own Creative Commons licence and says whether it may be downloaded.
2. **Internet Archive**
   - A huge and odd collection: live recordings, historical audio, net labels.
   - Its search and metadata APIs need no key.
   - Rights vary item by item, so every recording shows its licence.
3. **Later, after a reliability check:** ccMixter, whose API is old.

**Not Free Music Archive.** Its API was shut down, and streaming from its servers is not allowed.

## How it fits what exists

- **The library already separates the three things.** A track, its recordings, and each recording's audio
  source are kept apart (`library.md`). Catalogues become new `AudioSource` variants (`Jamendo { id }`,
  `InternetArchive { item, file }`) next to `TakeServer` and `LocalFile`.
- **Each catalogue track keeps its rights.** It records its source, original URL, artist, artwork, licence,
  attribution text, and whether it may be downloaded. Soundscape never treats a non-commercial or
  no-derivatives track as unrestricted. The track's screen shows the licence and credit.
- **Catalogue metadata is kept separately.** Saved and played catalogue tracks are kept on the phone in a
  catalogue store, separate from `library.json`. Search results stay in a cache. Audio is never copied into your
  home library by itself: you choose to stream, or to download where the licence allows it (this uses PR 4's
  offline store).
- **Playlists and favourites already work by id** (`collection.md`), so catalogue tracks join them with ids like
  `jamendo:<id>` and `archive:<item>/<file>`.
- **Requests go through the app's Rust networking**, like the take server's. The rules (what the licence allows,
  building the credit line) are Rust and are tested on Linux.

## Limits

These catalogues are not Spotify or Apple Music, and they have no mainstream new releases. This is not meant
to grow into a full streaming service.
