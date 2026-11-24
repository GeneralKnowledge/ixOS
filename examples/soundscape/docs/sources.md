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
asking Jamendo. Not done yet: a Jamendo track that has since been taken down is not marked unavailable in the library.

**Checked live** from the laptop with Reuben's client ID (2026-11-18):

- **Empty answers.** Jamendo sometimes answers "success" with no results to a search that finds tracks a
  moment later (2, 0, 2, 2, 0, 2 results for the same request). Soundscape asks up to three times before it
  shows "no results".
- **Listed tracks with no file.** Some tracks it lists have no file: the stream answers 404, even for this week's
  most popular track. The player then says "Jamendo has no playable file for this track right now", not a
  network error. Tracks that do stream answer byte ranges (206, `audio/mpeg`), so seeking works.
- **A wrong client ID** is HTTP 200 with `status: failed`, code 5; Soundscape reads the status, not the HTTP code.
- **Licences.** Most tracks are CC BY-NC-ND (non-commercial, no changes), and downloading is allowed per track.
- **Audio URLs** carry a `from=` token. Saved tracks keep the URL they were added with; if those turn out to
  expire, the next step is fetching a fresh URL by id (`/tracks/?id=`) when a saved track plays.

## Internet Archive, as built

- **Discover** has a switch: Jamendo or Internet Archive. The Archive is searched for audio items (albums,
  concerts, recordings), most downloaded first. Until you type, it shows this week's most downloaded net-label
  releases. No key is needed.
- **An item opens to its tracks**: Play, Shuffle, Add All to Library, and each track with play, Play Next, Add
  to Queue and Add to Library, as in Jamendo's list.
- **One track per piece of audio, in its best MP3.** An item often holds the same song as FLAC, Ogg and several
  MP3s. Soundscape keeps one per original, preferring VBR MP3, then MP3, then the smaller MP3s; Ogg and FLAC
  are left out because AVPlayer streams MP3 reliably. Tracks are ordered by their track number, then by file
  name. A title missing from the tags comes from the file name, without its leading number.
- **Ids** are `archive:<identifier>/<file>`, and streams are `archive.org/download/<identifier>/<file>`.
- **Rights.** Each track carries the item's licence, or "Licence not stated" when the item has none, which is
  common. Downloading is allowed unless the item is access-restricted (lending items). A dark or missing item
  gives no tracks.
- **In `rust/src/archive.rs`**, tested in `rust/tests/archive.rs` against canned answers in the Archive's shapes.
  The live API hasn't been checked yet: this cloud session can't reach archive.org, so that check is for the
  laptop.

## MusicBrainz, as built

MusicBrainz recognises songs; it serves no audio. On a song's page, **Identify Song** looks the song up by its
title and artist (`/ws/2/recording`, JSON, no key) and lists what MusicBrainz knows, best first.

- **Ranking.** MusicBrainz's own score counts for less than what Soundscape knows about the song: the same
  title (or the same title without a bracketed note such as "(final)" or "(2011 Remaster)"), the same artist,
  and a length within 5 seconds. A length far off counts against, and so does a live recording (said so in its
  note, or released only on live albums) unless the song's own title says live. Matches from 80 out of 100 with
  an agreeing length are marked **Likely**. Each match shows its note ("live, 1979-01-24: Berlin").
- **The release.** Each match comes with the release it is best known from: official before anything else, an
  album before a single or compilation, then the earliest. Its cover is the release group's front cover at the
  Cover Art Archive.
- **Nothing changes on a lookup.** The person picks a match, then **Link and Use Its Details** (title, artist,
  album and track number, saved as the person's own changes so Edit can undo them) or **Link Only**. The cover
  shows only when the song has no picture of its own.
- **The same recording elsewhere.** A linked song keeps its MusicBrainz recording id (`library.json`,
  `musicbrainz`). Songs from different sources linked to the same id list each other under **Also in Your
  Library**. Merging them into one song with several sources comes with reconciliation
  ([architecture.md](architecture.md)); linking never merges by itself.
- **Manners.** Requests say who is asking in their User-Agent, as MusicBrainz requires, and go one at a time,
  only when the person asks. Up to 25 recordings come back per lookup.
- **Checked live (2026-11-19, from the laptop).**
  - MusicBrainz is often busy for everyone: three 503s in a row, each with `Retry-After: 1`, then an answer. A
    lookup now tries up to 4 times, waiting what it is asked (at most 3 seconds).
  - Without a User-Agent it refuses with 403.
  - `score` is a number. `length` can be missing. A release may have no `status`, a release group no
    `primary-type`, and track numbers are text.
  - The top three answers for a famous studio song, all scored 100, were concerts. That is why live recordings
    count against a match.
  - Cover Art Archive covers redirect twice (to archive.org), which URLSession follows.
- **In `rust/src/musicbrainz.rs`**, tested in `rust/tests/musicbrainz.rs` against canned answers and the real
  answer from that check (`rust/tests/data/`).

## One search for everything, as built

The library's search box searches everywhere. Your own music answers at once, on the phone. Half a second after
typing pauses, Jamendo and the Internet Archive are asked together (`search_catalogues`, each on its own thread),
and their results appear below under **From Jamendo** and **From the Internet Archive**, at most 8 each, with
**More in Discover** for the rest.

- **Library first.** A Jamendo track already in the library is left out of Jamendo's section: the library's row
  has the person's edits and any download.
- **Each source answers for itself.** A source that fails or isn't set up (no Jamendo client ID) says why in its
  own section, and never hides the other's results.
- **Typing doesn't flood the catalogues.** Each change of the search cancels the previous one, and nothing is
  asked for an empty search (the catalogues' popular lists belong to Discover).
- **In `rust/src/search.rs`**, tested in `rust/tests/search.rs`.

## More by the artist and by genre, as built

Now Playing on a Jamendo song (from the library or straight from Discover)
has an **On Jamendo** section: **More by <artist>** and **More in This
Genre**. Rust (`jamendo::more`) first asks Jamendo for the song itself
(`tracks/?id=`), so it works for songs added before Soundscape kept an
artist's id, then asks for that artist's tracks (`artist_id=`, most played
first) or the song's first genre (`tags=`, this week's most popular),
leaving out the song playing. Discover's Jamendo list starts with
**Browse by Genre**: twenty of Jamendo's genre tags (`jamendo::GENRES`),
each opening that genre's popular tracks. `tags=jazz` was checked live on
2026-11-18; `artist_id=` and the other tags were not yet. The library's own
genre names are unchanged (`newage` stays "Newage" there), so songs already
added don't move genre.

## Test catalogue, as built

`rust/src/test_catalogue.rs` holds eight tracks that Soundscape writes itself the first time Discover's
**Test Catalogue** is opened (menu at the top right), into `test-catalogue/` in the app's data folder (about
18 MB). The same files come out on every phone and in every test, and nothing needs the network or a client ID.

| Track | Checks |
|---|---|
| Short Chime (4 s, WAV) | The next song starts on its own; the time never runs past the end |
| Long Drone (10 min, WAV at 8 kHz) | Scrubbing, locking the phone, the sleep timer |
| No Artwork (45 s, AIFF) | The stand-in artwork everywhere, the lock screen too |
| Untagged (30 s, CAF) | No artist, album or genre: Unknown Artist, and lists still sort |
| Theme in C, slow (60 s, WAV) and fast (40 s, AIFF) | Two recordings of one work stay two songs (lengths differ) |
| Theme in C, copy (60 s, CAF) | The slow recording again: Your Music offers to merge the two |
| Stream Only (20 s, WAV) | Download isn't offered; Downloads never lists it |

The audio is tones written as 16-bit mono PCM: WAV, AIFF and CAF are the formats iOS plays that need no
encoder, so MP3 and AAC are not covered here (Jamendo and the Archive already serve MP3). Artwork is a PNG drawn
by the same module. Ids are `test:<slug>`, the provider is "Test Catalogue" and the licence CC0. A test track
saved in the library keeps a `file://` URL; the app's data folder moves when it is reinstalled, so building the
library follows the file name into the folder as it is now (`test_catalogue::relocate`, only for the
catalogue's own names). Downloading one copies the file, and only a test track's recording may be copied from
the phone this way.

## Order of work

1. ~~The track resolver~~ (the library is the lookup; above) and Jamendo: built.
2. Internet Archive, as a second and independent source: built (above). Its file checksums (`sha1`, `md5`)
   are not used yet; download verification comes with reconciliation.
3. MusicBrainz, to recognise the same recording from different sources: built (above). Merging linked songs
   into one comes with reconciliation ([architecture.md](architecture.md)).
4. A small fixed test catalogue: built ([below](#test-catalogue-as-built)).
5. Mixed playlists, and playback across all sources, tested together.

Not now: more providers, recommendations, or extra infrastructure.

## Limits

These catalogues are not Spotify or Apple Music, and they have no mainstream new releases. This is not meant
to grow into a full streaming service.

## Exploring Jamendo, as built (Rust)

Reuben (2026-11-24): Soundscape focuses on Jamendo for now, and is a *discover* app: nobody knows Jamendo's
artists beforehand, so exploring has to offer much more than one week's most popular songs. This part is the
Rust; the screens come in their own PR (`docs/design/`: data before visuals).

- **Shelves** (`jamendo_shelves`, `jamendo_shelf`): Jamendo's Picks (`featured=1`), New Releases
  (`order=releasedate_desc`), Rising (`order=buzzrate`, songs gaining listeners fastest), Popular This Month,
  Instrumental (`vocalinstrumental=instrumental`) and All-Time Favourites, each with singles and album tracks.
- **Artists to discover** (`jamendo_artists`): this week's most popular artists, or the newest; ones with a
  picture first.
- **Because You Like** (`jamendo_because`): the Jamendo song in your library you play most (a favourite counts
  as three plays) starts it; songs in its genre by artists not in your library.
- **Artist pages** (`jamendo_artist`, `jamendo_artist_of_track`): the artist and their releases, newest first,
  as **Albums, EPs and Singles** (one song is a single; up to six songs under 30 minutes an EP; more an album).

### Functional albums

Many Jamendo songs are not in a proper album: artists upload one single at a time. Reuben's rule: *singles with
the same album art go together*. So on an artist's page:

1. Jamendo's own albums with two or more of the artist's songs stay as they are, in Jamendo's track order.
2. Every other song is grouped by its cover. Jamendo gives each single its own artwork address even when the
   picture is the same, so the cover files themselves are compared: each is fetched once (at most 80 per page,
   2 MB each) and compared by SHA-256, and kept for the rest of the run so a page opened again fetches nothing.
3. Songs sharing a cover make one album (marked `grouped`), in the order they came out, named by the album name
   they all share or else after the earliest song. A song left alone is a single. A cover that can't be fetched
   leaves its song a single.

Not checked yet: the live API (the cloud can't reach Jamendo). In particular, whether singles with the same
picture always have byte-identical cover files; if Jamendo re-encodes them, a perceptual comparison (a small
hash of the scaled-down picture) is the next step. The order and filter names (`releasedate_desc`, `buzzrate`,
`featured`, `vocalinstrumental`, `joindate_desc`) are from Jamendo's API documentation.
