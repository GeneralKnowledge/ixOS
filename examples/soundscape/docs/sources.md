# Music sources: free catalogues next to your own music (plan)

Reuben, 2026-11-17. Moved to the front on 2026-11-18: Reuben wants to test Soundscape with real, full-length
music, and the generation work (takes, picks, timed lyrics) waits. **Jamendo is built** (below); Internet
Archive and MusicBrainz come next.

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

## Jamendo, as built

- **Discover** (first row of Your Music) searches Jamendo, showing this week's popular tracks until you type.
  Each result shows its artwork, artist, album, length and licence. Tap plays the results from there through
  the same player and queue; Play Next and Add to Queue are on the long press.
- **The client ID is Reuben's own**, typed once in Discover and kept in `listen.json` next to the take server's
  address. Without one, Discover asks for it and the rest of Soundscape is unaffected.
- **Add to Library** (the plus) keeps the track in `library.json` under `catalogue`, by its id `jamendo:<id>`,
  with its rights as they were when added. From then on it is a track like any other: it joins its album and
  artist, can be edited, favourited, put in playlists, and downloaded where Jamendo allows it
  (`audiodownload_allowed`). Its screen shows the licence, the credit, whether offline and changes are
  allowed, and a link to its Jamendo page. **Remove from Library** takes it out again.
- **Rights**, in `rust/src/catalogue.rs`: the Creative Commons licence URL becomes a name (`CC BY-NC-SA 3.0`),
  may adapt (no ND), non-commercial (NC), and the credit line. An unknown licence is never adaptable.
- **The library no longer needs a take server.** It opens on your imported and Jamendo music; the take server
  is an optional source in the Library's menu.

**One change from the plan above:** saved catalogue tracks are kept in `library.json` (its `catalogue`
section) rather than a store of their own. Every screen already looks tracks up in the library, so playlists,
favourites, history, downloads and the queue work for Jamendo tracks with no separate resolver. Their rights
live with the saved track, so they outlast any cache. Search results that are not added stay in memory only.

Of the resolver tests above, these pass in `rust/tests/catalogue.rs`: adding to a playlist, restarting and
finding it again, leaving search results without losing the playlist entry, and keeping the licence without
asking Jamendo. Not done yet: a Jamendo track that has since been taken down plays as a stream error rather than
showing as unavailable with a reason.

## Order of work

1. ~~The track resolver~~ (the library is the lookup; above) and Jamendo: built.
2. Internet Archive, as a second and independent source: its metadata gives each file's format, size and
   checksum, which also tests imports and download checks.
3. MusicBrainz, to recognise the same recording from different sources (reconciliation in
   [architecture.md](architecture.md)).
4. A small fixed test catalogue: short and long tracks, with and without artwork or tags, two recordings of one
   work, several formats, and one track that may not be downloaded.
5. Mixed playlists, and playback across all sources, tested together.

Not now: more providers, recommendations, or extra infrastructure.

## Limits

These catalogues are not Spotify or Apple Music, and they have no mainstream new releases. This is not meant
to grow into a full streaming service.
