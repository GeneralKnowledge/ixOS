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
| Discover | Search and browse the free catalogues, with its own look (the wider world of music, not another folder of My Music) and a way to bring finds into your playlists |
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
- **Each catalogue track keeps its rights, as separate facts.** It records its source, original URL, artist,
  artwork, licence and attribution text. It also keeps four permissions apart:
  - may it be streamed;
  - may it be downloaded for offline listening;
  - what attribution is required;
  - whether it may be adapted (used in a mix or a generation).

  Downloadable never means reusable: no catalogue track goes into a generated mix or a redistribution unless
  its licence allows that. Soundscape never treats a non-commercial or no-derivatives track as unrestricted.
  The track's screen shows the licence and credit.
- **Catalogue metadata is kept separately.** Saved and played catalogue tracks are kept on the phone in a
  catalogue store, separate from `library.json`. Search results stay in a cache. Audio is never copied into your
  home library by itself: you choose to stream, or to download where the licence allows it (this uses PR 4's
  offline store).
- **A track resolver comes first (required before any catalogue code).** Today the collection only looks up ids
  in the personal library, and treats anything else as not in your library. Catalogue tracks live outside
  `library.json`, so playlists, favourites, search and playback must look a track up through one resolver. The
  id's prefix picks the store that owns it: `take-server:…` and local files go to the personal library,
  `jamendo:<id>` to Jamendo, `archive:<item>/<file>` to Internet Archive. The resolver answers with the track's
  details and a playable source, or says it is unavailable and why. The stores stay separate; only the lookup
  is shared. The resolver needs tests (Rust, Linux) for:
  - adding a catalogue track to a playlist;
  - restarting the app and resolving it again;
  - dropping it from search results without losing the playlist entry;
  - a catalogue recording that was deleted or is unavailable (shown as such, kept in the playlist, back if it
    returns);
  - keeping the licence and attribution after cached details expire. The rights are kept with the saved track,
    not in the expiring cache.
- **Each source can be removed or can fail on its own.** If Jamendo changes its API, disappears, or its client
  ID is missing, Soundscape still starts, plays your own music and keeps every playlist. That source's tracks
  show as unavailable until it works again. A source is a module behind the resolver, never a dependency of the
  library or the player.
- **Requests go through the app's Rust networking**, like the take server's. The rules (what the licence allows,
  building the credit line) are Rust and are tested on Linux.

## Order of work

1. Finish the six player PRs: player, library, collection, offline, creation, removing Studio.
2. The track resolver, over the personal library only at first, with the tests above.
3. Jamendo, with Reuben's free client ID.
4. Internet Archive, as a second and independent source.
5. Mixed playlists, and playback across all sources, tested together.

Not now: more providers, recommendations, or extra infrastructure.

## Limits

These catalogues are not Spotify or Apple Music, and they have no mainstream new releases. This is not meant
to grow into a full streaming service.
