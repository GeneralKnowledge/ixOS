# Soundscape

A personal music player: one library for your own music files, free
catalogues (Jamendo, the Internet Archive) and songs on a take server on your
computer, with favourites, playlists, offline downloads, a queue that survives
restarts, and lock-screen playback. The player doesn't care where a song comes
from ([architecture](docs/architecture.md)). It runs inside Device Lab (Apps,
then Soundscape) and on its own.

Until 2026-11-18 Soundscape was also a loop recorder ("Studio": record loops,
layer them, mix down to WAV). Studio was removed then; a phone that used it
keeps its old `soundscape.json` and `recordings/` untouched, and the app no
longer asks for the microphone.

## Where it is

| Step | What | State |
|---|---|---|
| L1 | Listen tab: songs and takes from a take server, A/B at matched loudness, stars, notes and picks, lock-screen playback | Passed on the iPhone (Device Lab, 2026-11-16) |
| M2 | Music library ([design](docs/library.md)): tracks apart from their recordings and audio sources, artists, albums and songs with search, artwork (from the server or Photos), your own edits, a preferred recording per song, kept on the phone | Passed on the iPhone (Device Lab, 2026-11-17) |
| M3 | Your collection ([design](docs/collection.md)): favourite songs, albums and artists, playlists (new, rename, reorder, delete, add a song, an album or what's playing), listening history and Recently Played, and lyrics to read along (Now Playing and a song's screen, the playing take's own words; with LRC timings from the generator the line, and with word timings the word, being sung lights up) | Passed on the iPhone (Device Lab, 2026-11-18); playlist delete, remove and reorder fixes (#328) to recheck |
| M4 | Offline ([design](docs/offline.md)): download a song, album or playlist to play it without the take server (its preferred take, replaced when you pick another), a Downloaded screen with the space it takes, and Import Music from Files (your own MP3, M4A, WAV, AIFF, FLAC files, with their tags and cover) as tracks like any other | Passed on the iPhone (Device Lab, 2026-11-18): album and song downloads, Import Music |
| M1 | Music player: play all, an act or a song, shuffle and repeat, Play Next and Add to Queue, previous and next (also on the lock screen), a mini-player on every screen and a Now Playing screen with Up Next | Passed on the iPhone (Device Lab, 2026-11-17): plays on, next song while locked, lock-screen controls |
| M5 | Takes and versions ([design](docs/library.md#takes-and-versions)): rate and note a song's takes on its own screen, sort them best, newest or oldest first, rate the playing take in Now Playing, and a To Pick list of songs with several takes and no pick; compare A/B (blind if you like) from the song | Rust tested on Linux; Swift written, not yet built with xtool or run on a phone |
| N1 | Jamendo ([plan](docs/sources.md)): Discover searches Jamendo with your own client ID (this week's popular until you type) and plays full tracks through the same player; Add to Library keeps a track with your music (playlists, favourites, edits, downloads where Jamendo allows), with its licence, credit and what it allows on its screen. The library now works without a take server | Rust tested on Linux; Swift written, not yet built with xtool or run on a phone |
| N2 | Internet Archive ([plan](docs/sources.md#internet-archive-as-built)): a Jamendo / Internet Archive switch in Discover; Archive albums and concerts open to their tracks (best MP3 of each), to play, add one or all to the library, and download unless the item is restricted | Rust tested on Linux against canned answers; live API not yet checked; Swift written, not yet built with xtool or run on a phone |
| N3 | MusicBrainz: Identify Song looks a song up, ranks what MusicBrainz knows by title, artist and length, and links the one picked, with or without its details and cover; songs linked to the same recording list each other | Rust tested on Linux; Swift written, not yet built with xtool or run on a phone; live API checked from the laptop (2026-11-19) |
| N4 | A small fixed test catalogue | Planned |
| N5 | One search for everything: the library's search also asks Jamendo and the Internet Archive, together, and shows their results below your own music, each source with its own section or its own reason it couldn't answer | Rust tested on Linux; built for iPhone and looked at on the phone (2026-11-21) |
| N6 | More from Jamendo: Now Playing on a Jamendo song offers more by its artist and more in its genre; Discover browses Jamendo by genre | Rust tested on Linux; Swift written, not yet built with xtool or run on a phone |
| M7 | Studio removed: Soundscape opens straight on your library; no microphone permission | Rust tested on Linux; Swift written, not yet built with xtool or run on a phone |
| M6 | Carry on where you left off: the queue, the song and how far into it survive closing the app, and it opens paused there | Rust tested on Linux; Swift written, not yet built with xtool or run on a phone |
| M8 | Sleep timer: pause after 15 to 90 minutes or at the end of this song, fading out over the last 10 seconds | Swift written, not yet built with xtool or run on a phone |
| M9 | Between songs: no gap (the next song is loaded early and starts the moment one ends) or a 2 to 12 second crossfade, kept as a setting | Rust tested on Linux; Swift written, not yet built with xtool or run on a phone |
| S1-S3 | The redesign ([design](docs/design.md)): four tabs (Listen, Library, Discover, Search), the mini-player above the tab bar, a Listen home of artwork shelves, the Library by kind with an album grid, and a full-screen Now Playing in the artwork's colours with lyrics in place, a queue sheet, volume and AirPlay | Swift written, not yet built with xtool or run on a phone |
| R1 | Radio ([design](docs/design.md#radio-reuben-2026-11-22-2028-and-2036)): nonstop stations from a genre, a song or an artist, mixed in Rust (likes, favourites and plays weighted up, two in five songs new, no artist twice within four songs, disliked songs never), Like and Dislike in Now Playing, Hear Nonstop Music on Listen | Rust tested on Linux; Swift written, not yet built with xtool or run on a phone |

## How the work is split

- **Rust owns the state and the rules** (`rust/src`): the library (`library.rs`),
  favourites, playlists and history (`collection.rs`), what is kept on the phone
  (`offline.rs`), the queue (`queue.rs`), the radio's mix (`station.rs`) and what was playing (`playback.rs`),
  the catalogues (`jamendo.rs`, `archive.rs`, rights in `catalogue.rs`), the take
  server's client (`listen.rs`) and lyrics (`lyrics.rs`). All of it is tested on Linux.
- **AVPlayer plays** (`TakePlayer.swift`): the queue, the mini-player (`MiniPlayer.swift`) and Now
  Playing (`NowPlaying.swift`), the lock screen and interruptions, and blind A/B of two takes.

## Rust

```bash
cargo test --manifest-path examples/soundscape/rust/Cargo.toml
```

## Swift

| File | What |
|---|---|
| `Sources/AppHost/ContentView.swift` | The root: opens the core, then the library |
| `Sources/AppHost/ListenModel.swift` | The screens' model: asks the core, keeps alerts and feedback. Foundation only |
| `Sources/AppHost/ListenView.swift` | The library stack with the mini-player, Now Playing, a song's takes |
| `Sources/AppHost/LibraryView.swift` | Songs, albums and artists, search, a song's screen with its recordings and licence |
| `Sources/AppHost/CollectionView.swift` | Your Music: favourites, playlists, history, To Pick |
| `Sources/AppHost/DiscoverView.swift` | Jamendo and the Internet Archive |
| `Sources/AppHost/OfflineView.swift`, `MusicImporter.swift` | Downloads, and importing your own files |
| `Sources/AppHost/TakePlayer.swift` | AVPlayer: queue, lock screen, interruptions, saving what plays |
| `Sources/AppHost/SoundscapeErrors.swift` | Rust's errors in words |

## Listen

Listen is for generated songs that live on another computer, such as the
band laptop's renders (GeneralKnowledge/JewGrass). That computer runs a
**take server** (`python3 tools/player/serve.py` in JewGrass, standard
library only), which finds the takes, streams them, measures their loudness
and saves picks to `~/band/picks.json`.

- **Rust (`rust/src/listen.rs`)** keeps the server's address (in
  `listen.json`, apart from the projects), asks the server for songs and
  takes through the `http` native service (URLSession on iOS, so it follows
  Tailscale), sends picks, stars and notes, and works out the volumes for
  an A/B comparison at matched loudness (`loudness_match`: the louder take
  is turned down, never by more than 24 dB).
- **Swift** streams with AVPlayer (`TakePlayer.swift`). Comparing plays both
  takes in step and switching only changes which one you hear, so A and B
  are heard at the same moment; Blind hides which is which. Playback goes on
  with the screen locked, with lock-screen controls; "next track" on
  headphones switches A and B in a comparison and moves through the queue
  otherwise.
- **The library (`rust/src/library.rs`, [design](docs/library.md))** keeps
  tracks apart from their recordings and where their audio lives. The take
  server's catalogue (`/api/catalog`, one request) is kept in
  `library.json` with your edits and artwork, and `build_library` makes the
  artists, albums and tracks from those, so the library browses without
  the server and a refresh never undoes an edit. A track plays its
  preferred recording: the server's pick, else the best rated, else the
  newest; choosing another picks it on the server.
- **The collection (`rust/src/collection.rs`, [design](docs/collection.md))**
  keeps favourites, playlists and history by library id in
  `collection.json`, apart from the library, and shows them against the
  library as it is now: a song the library lost is hidden, not forgotten.
  A song counts as played after 30 seconds (or half a shorter one).
- **The queue (`rust/src/queue.rs`)** decides what plays next. Play,
  Shuffle, an act's play button or a song's Play plays songs one after
  another, each with its preferred take (`preferred_take`: the pick, else
  the best rated, else the newest; `listen_tracks` looks them up). Shuffle
  keeps the song playing and plays the others once each in random order;
  repeat goes Off, All, One. Play Next and Add to Queue add a song after
  the current one or at the end. The queue is a value Swift keeps and hands
  back to these functions.
- The song list is in sections by act when the server says which act a
  song belongs to (`group_songs` in Rust; the act comes off the title if the
  server put it in front), and **Unpicked Only** keeps the songs still
  waiting for a pick. Search matches songs and acts.
- The address must be numeric for plain http (iOS refuses plain http to a
  host name): use the computer's Tailscale address, which starts with 100.

| File | What |
|---|---|
| `Sources/AppHost/ListenModel.swift` | Songs, a song's takes, picks and ratings, the server address. Foundation only |
| `Sources/AppHost/ListenView.swift` | The Library tab's stack, the server form, a song's takes and the comparison panel, the mini-player and Now Playing with Up Next |
| `Sources/AppHost/LibraryView.swift` | Songs, albums and artists, search, an album, an artist, a track with its recordings, editing and artwork |
| `Sources/AppHost/TakePlayer.swift` | AVPlayer: the queue, or two takes in step for A/B; the lock screen |

Build and install on its own (it uses the free team's sandbox id):

```bash
forge run ios --project examples/soundscape
forge ui run examples/soundscape/uitests/smoke.toml   # phone plugged in
```
