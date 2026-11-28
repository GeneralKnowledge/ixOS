# Podcasts and live radio in Soundscape: proposal

Proposal, 28 November 2026, for Reuben's review. Nothing here is built yet. It refines Reuben's idea of the
same day ("a global audio discovery platform": podcasts plus live radio from around the world, one player)
against what Soundscape already is and what has already been decided about it.

## In short

- **Yes to both, in one app and one player.** Live radio and podcasts fit Soundscape's real identity, which is
  *discovering* things you didn't know (24 November), not playing mainstream catalogues. "Listen to the world"
  is a good extension of that, not a new product.
- **Live radio first, and small.** It was already planned as step 2 of Radio in [design.md](design.md); it needs no
  subscriptions, no downloads and no new player features. Two PRs (Rust, then screens).
- **Podcasts second, on the plan already approved on 20 November** ([android-ports.md](../../../docs/research/android-ports.md#3-antennapod--soundscape),
  P0 to P4), with three changes below. P0 (speed, skip amounts, saved position) comes first because it improves music too.
- **Not before the owed checks.** Soundscape has Jamendo pages (#444, #445) and seamless switching (#447)
  merged but not yet checked on the phone, and you narrowed its scope on 22 and 24 November. Live radio starts
  once those checks pass; it should not overtake them.
- **Three things in the original idea I'd change:** the Dart and Flutter packages don't fit a Rust and Swift app;
  AntennaPod's, RadioDroid's and MusicPod's source must not be studied (clean-room rule); and five source tabs
  would undo the redesign. Details follow.

## What already exists or is already decided

| Piece | Where | State |
|---|---|---|
| Generated "Radio" stations (nonstop Jamendo music from a genre, song or artist) | `rust/src/station.rs`, `Radio.swift` | Built (R1) |
| Live radio through Radio Browser, shown apart from generated stations | [design.md](design.md) "Live radio (step 2)" | Planned |
| Podcasts as their own section, shared player and queue, P0 to P4 | [android-ports.md](../../../docs/research/android-ports.md#3-antennapod--soundscape) | Approved 20 Nov |
| Clean room for GPL apps: don't read AntennaPod's source | same, "Rules" | Approved 20 Nov |
| Feed parsing moves out of Commonplace into the framework on its second use (podcasts) | [app-plan.md](../../../docs/app-plan.md) framework table | Planned |
| Tab set reviewed when podcasts arrive | [design.md](design.md) "Podcasts stay on the roadmap" | Planned |
| Downloads, Downloaded screen, lock-screen controls, sleep timer, saved queue | `offline.rs`, shared Now Playing helper | Built |
| Plain `http` streams allowed (`NSAllowsArbitraryLoads`) | `forge.toml`, `Info.plist` | Built |

So most of the plumbing is in place. What's new is two directories, two kinds of thing to play, and the screens.

## Where I'd change the idea

### 1. Use Rust crates and plain HTTP, not Dart packages

`podcast_search`, `universal_feed` and MusicPod are Dart and Flutter. Soundscape's logic is Rust and its
screens are SwiftUI, so they can't be used. What they do is small in our stack:

| Job | In Soundscape |
|---|---|
| Search podcasts | Two plain JSON APIs through `forge-net`: Apple's iTunes Search API (no key) and the Podcast Index (free key and secret, a signed header). About 150 lines with fixture tests. |
| Parse feeds (RSS, Atom, JSON Feed, iTunes and Podcasting 2.0 tags) | `feed-rs` (MIT/Apache), which Commonplace already uses, extracted into the framework as planned. Podcast tags it doesn't map (chapters, transcript links) are read with `quick-xml`, also already in use. |
| OPML import and export | Commonplace's `opml.rs`, extracted with the parser. |
| Search radio stations | Radio Browser's JSON API through `forge-net`. No key. |
| Play streams and episodes | The existing AVPlayer path. No new decoder. |

No new heavy dependency, and the framework gains feed parsing on a real second use, which is the extraction
rule working as intended.

### 2. Study behaviour, not source

AntennaPod is GPL-3.0, and the rule you approved on 20 November is a strict clean room: we learn from the
running app, its documentation and public specs (RSS, OPML, gPodder API), never its code. RadioDroid and
MusicPod are GPL-3.0 too, as far as I can tell, so the same rule applies to them. Radio Browser's data is
public domain and its API is free to use, so that one is fine to integrate directly.

### 3. Keep four tabs; give live radio and podcasts a place inside them

Five tabs by source (Home, Music, Radio, Podcasts, Library) turns Soundscape into three apps sharing a
mini-player, and throws away the Listen / Library / Discover / Search shape the redesign just built. Apple Music
and Apple Podcasts are separate apps for a reason; Soundscape's advantage is that one place holds all three.
Organising by *what you're doing* rather than *where it comes from* keeps that:

| Tab | Gains |
|---|---|
| **Listen** | Continue the episode you were in; new episodes from your shows; your favourite stations; recently played across all three |
| **Discover** | A **Live Radio** section (by country, language, genre, "Around the World" shelf) and later a **Podcasts** section (categories, charts, independent shows) beside music |
| **Search** | One search; results grouped Songs / Stations / Podcasts, as #364 already does for Jamendo and Archive |
| **Library** | **Stations** (saved live stations) and **Podcasts** (your shows, downloaded episodes) rows next to Playlists and Albums |

If podcasts become something you open daily, a fifth **Podcasts** tab is the natural review point at P1
(iOS allows five without "More"). Live radio does not need its own tab.

### 4. Name the two kinds of radio apart

"Radio" already means generated Jamendo stations ("Hear Nonstop Music", "Start Radio"). Live broadcasts are
different: no queue, no skip, no Like. I suggest **Stations** for the generated mixes (they're already called
"… Station") and **Live Radio** for broadcasts. Your call.

## How it fits the existing player

Three kinds of thing to play, as you said, and kept apart in the data:

- **Song** (today's `Track` and `Recording`): unchanged.
- **Live station:** a name, country, language, tags, artwork, and one or two stream addresses. It is *not* put in
  the queue. Playing one sets the music queue aside (saved, as today), and the player shows a **Live** state:
  no scrubber, no skip, no Like, no repeat; pause means stop-and-rejoin-live. Stopping returns to the queue
  where it was. Lock screen marks it as live.
- **Episode:** a seekable file with a saved position and playback speed. Episodes *do* go in the queue, so the
  queue gains an item kind (song or episode) rather than a second "Up Next". Downloads reuse `offline.rs`;
  episodes are always downloadable (publishers put them in feeds to be downloaded), unlike Jamendo tracks whose
  rights decide.

Rust owns the directories, subscriptions, episode state and station favourites (new JSON documents beside
`collection.json`, as the rest of Soundscape does); SwiftUI draws them with ForgeUI; AVPlayer plays them.

## Live radio, practically

Things that decide whether this feels solid or flaky:

- **Dead streams.** A directory listing doesn't mean a station is on air. Ask Radio Browser for checked stations
  only (`hidebroken`, `lastcheckok`), try the resolved stream address then the original, give up after a short
  timeout with a clear "This station isn't broadcasting right now", and remember failures on the phone so a
  station that fails twice sinks in lists. Real multi-stream failover only exists where a station lists several
  streams, which Radio Browser mostly doesn't; I won't promise more than that.
- **Codecs.** AVPlayer plays MP3, AAC/HE-AAC and HLS, which is most stations. Ogg Vorbis and Opus streams it
  doesn't play reliably, so those are filtered out rather than adding a decoder. To confirm on the phone.
- **Being a good citizen of Radio Browser.** It's volunteer-run: pick a server from its list rather than
  hard-coding one, send a descriptive User-Agent, and report a play through its click endpoint (that's what
  drives its popularity ranking).
- **Reconnection.** After a network drop or an interruption (a call), rejoin live rather than resume a stale
  buffer. The existing interruption handling is the place for it.
- **Now-playing titles.** Many Icecast and Shoutcast stations send the current song (ICY metadata). AVPlayer
  exposes it on some streams and not others, so it's shown when present and never relied on.
- **No recording.** Soundscape plays broadcasts; it never records, stores or rebroadcasts them.

## Podcasts, practically

- **Directories.** Apple's search API needs no key and has the widest coverage; the Podcast Index is the open,
  independent one and carries Podcasting 2.0 data. I'd use the Podcast Index when a key is set (entered on the
  phone like the Jamendo client ID) and Apple's otherwise, and always allow subscribing by feed address.
- **Refresh.** On opening and pull-to-refresh in P1; background refresh and automatic downloads in P2 (iOS
  background time is limited, so "new episodes are there when I open the app" is the honest promise).
- **Transcripts** reuse the lyrics view; **chapters** come in P4 as planned.
- **Portability:** OPML in and out from P1; gPodder sync with a Nextcloud server stays P3.

## Milestones

| Order | Milestone | What it delivers | Phone check |
|---|---|---|---|
| Gate | Owed Soundscape checks | Jamendo pages and functional albums (#444/#445), seamless switching (#447) | Already written |
| 1 | **L1 Live Radio** | Radio Browser client in Rust (search; browse by country, language, genre; top stations), live-station play state, favourites and recents. Rust PR, then a screens PR (Discover section, Search results, Library "Stations", Live state in mini-player and Now Playing) | Find and play a station from Japan, one from the UK, one by genre; save one; lock screen; walk out of Wi-Fi and back |
| 2 | **L2 Live Radio, finished** | Failure memory, dead-stream message, now-playing titles where sent, "Around the World" shelf, sleep timer on live, rejoin after a call | A dead station, a call during playback |
| 3 | **P0 player features** | Speed, skip amounts, saved position per item (music benefits too) | Short listening check |
| 4 | **P1 Podcasts** | First the feed-parser extraction PR (framework), then subscriptions, episodes, stream and download, played state, OPML. Tab review here | Subscribe to a fixture feed from the laptop and one real show; download, play offline, resume |
| 5 | P2 to P4 | Download rules, gPodder sync, chapters and transcripts, as approved | As in android-ports.md |

Each Rust PR is tested on Linux with fixture responses (no live network in tests); screens follow in their own PR,
as the design programme asks.

## My honest recommendation on sequencing

1. Run the owed Soundscape phone checks first. Live radio adds a new way for playback to fail; it's much easier
   to judge once the music player is known to be good.
2. Then **L1 + L2 Live Radio**. It's cheap, immediately fun, and the "listen to the world" idea shows up in a
   week rather than a month.
3. Then **P0**, then **P1 Podcasts**. Podcasts are the larger build (subscriptions, refresh, downloads, the parser
   extraction), so they come once the player work they depend on is done.
4. The M12 to M18 roadmap stays as it is; I'd slot L1/L2 between the Jamendo work and M12, and podcasts after M13
   (portability) so OPML lands beside M3U and backups rather than on its own.

## Decisions for Reuben

1. **Order:** live radio after the owed checks, then podcasts (recommended), or podcasts first.
2. **Names:** "Stations" for generated mixes and "Live Radio" for broadcasts (recommended), or something else.
3. **Podcast Index key:** register for a free one (recommended, for independent shows), or Apple search only.
4. **Tabs:** keep four and review at P1 (recommended), or add a Podcasts tab from the start.

## Sources

- [Radio Browser API documentation](https://api.radio-browser.info)
- [Podcast Index API documentation](https://github.com/Podcastindex-org/example-code) and [API docs mirror](https://github.com/jamescridland/docs-api)
- [Apple developer forums: AVPlayer and ICY metadata](https://developer.apple.com/forums/thread/102200), [Shoutcast metadata in Swift](https://developer.apple.com/forums/thread/650170)
- Earlier plans in this repository: [android-ports.md](../../../docs/research/android-ports.md), [design.md](design.md), [roadmap.md](roadmap.md), [app-plan.md](../../../docs/app-plan.md)
