# Podcasts and live radio in Soundscape: proposal

Proposal, 28 November 2026. Revised the same day after Reuben's review (his six changes are folded in; see
[Review changes](#review-changes-28-november)). Nothing here is built yet, and podcasts are not to be implemented
until he says so. It refines Reuben's idea of the
same day ("a global audio discovery platform": podcasts plus live radio from around the world, one player)
against what Soundscape already is and what has already been decided about it.

## In short

- **Yes to both, in one app and one player.** Live radio and podcasts fit Soundscape's real identity, which is
  *discovering* things you didn't know (24 November), not playing mainstream catalogues. "Listen to the world"
  is a good extension of that, not a new product.
- **Live radio first, and small.** It was already planned as step 2 of Radio in [design.md](design.md); it needs no
  subscriptions and no downloads. Three PRs: a small lock-screen change in the shared Now Playing helper, then
  Rust, then screens. It is a new *source*, which your 18 November rule ("player features before more sources")
  puts after player work; I recommend it anyway, because it was already planned and asks almost nothing of the
  player, but that is your call (decision 1).
- **Podcasts second, on the plan already approved on 20 November** ([android-ports.md](../../../docs/research/android-ports.md#3-antennapod--soundscape),
  P0 to P4), with three changes below. P0 (speed, skip amounts, saved position) comes first because it improves music too.
- **Not before the owed checks.** Soundscape has a backlog of merged work not yet checked on the phone. The one
  authoritative list is step 0 of Soundscape in [app-plan.md](../../../docs/app-plan.md#soundscape); this page
  doesn't keep its own. Live radio starts only once that list has passed, and is never a reason to defer it.
- **Three things in the original idea I'd change:** the Dart and Flutter packages don't fit a Rust and Swift app;
  AntennaPod's source must not be studied, and RadioDroid's and MusicPod's not before their licences are checked
  (clean-room rule); and five source tabs
  would undo the redesign. Details follow.

## What already exists or is already decided

| Piece | Where | State |
|---|---|---|
| Generated "Radio" stations (nonstop Jamendo music from a genre, song or artist) | `rust/src/station.rs`, `Radio.swift` | Built (R1) |
| Live radio through Radio Browser, shown apart from generated stations | [design.md](design.md) "Live radio (step 2)" | Planned |
| Podcasts as their own section, shared player and queue, P0 to P4 | [android-ports.md](../../../docs/research/android-ports.md#3-antennapod--soundscape) | Approved 20 Nov |
| Clean room for GPL apps: don't read AntennaPod's source | same, "Rules" | Approved 20 Nov |
| Feed parsing shared between Commonplace and Soundscape | [app-plan.md](../../../docs/app-plan.md) framework table | Extract only after both have passed on the phone (below) |
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
| Parse feeds (RSS, Atom, JSON Feed, iTunes and Podcasting 2.0 tags) | Soundscape depends on `feed-rs` (MIT/Apache) itself, the same crate Commonplace uses. Podcast tags it doesn't map (chapters, transcript links) are read with `quick-xml`. |
| OPML import and export | Soundscape's own, with `quick-xml`, checked against Commonplace's OPML files for interoperability. |
| Search radio stations | Radio Browser's JSON API through `forge-net`. No key. |
| Play streams and episodes | The existing AVPlayer path. No new decoder. |

No new heavy dependency, and no framework work up front. Soundscape builds its podcast feed handling itself,
using Commonplace's as the reference for behaviour (both are our own code, and apps may not depend on each other,
so the reference is read, not imported). Only once podcasts have passed on the iPhone, and Commonplace's feed
work has too, is what turns out to be genuinely shared extracted into the framework, in its own PR. That is the
[extraction rule](../../../docs/architecture.md#framework-principles) as written: first app, real second use,
both proven, then extract.

### 2. Study behaviour, not source

AntennaPod is GPL-3.0, and the rule you approved on 20 November is a strict clean room: we learn from the
running app, its documentation and public specs (RSS, OPML, gPodder API), never its code. I believe RadioDroid
and MusicPod are GPL-3.0 too but couldn't confirm it from this session, so check each licence before anyone
opens their code, and treat them the same way if they are. Their running apps, store pages and docs are fair
game either way. Radio Browser's data is
public domain and its API is free to use, so that one is fine to integrate directly.

### 3. Keep four tabs; give live radio and podcasts a place inside them

Five tabs by source (Home, Music, Radio, Podcasts, Library) turns Soundscape into three apps sharing a
mini-player, and throws away the Listen / Library / Discover / Search shape the redesign just built. Apple Music
itself puts live radio inside its existing tabs rather than in a tab per kind of audio; Soundscape's advantage is
that one place holds all three.
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
  Starting a live station also stops a generated station's top-up (`Radio.swift` owns it today), so the two
  never fight over the player.
- **Episode:** a seekable file with a saved position and playback speed. Episodes *do* go in the queue, rather
  than a second "Up Next". The cheapest way: a queue entry already carries a `Recording` with an `AudioSource`,
  so an episode becomes a new `AudioSource::Podcast { feed }` (and `RecordingKind::Episode`) and the queue itself
  is unchanged. Saved queues (`playback.json`) from before keep loading, because the new cases are additions.
  Downloads build on `offline.rs`'s store and Downloaded screen, but with podcast rules of their own (below),
  not the music rules unchanged.

Rust owns the directories, subscriptions, episode state and station favourites; SwiftUI draws them with ForgeUI;
AVPlayer plays them. Storage needs care: a feed can list over a thousand episodes, so the episode list is a cache
refetched from the feed, and only what the person did (subscribed, played, position, downloaded) is kept in a new
`podcasts.json`, never in `collection.json`. If that grows too large, `forge-storage` already offers SQLite.

**One framework change is needed early.** The shared lock-screen helper
(`crates/forge-services/swift/helpers/ForgeNowPlaying.swift`) always reports a playback rate of 1 and has no
"live" flag. Live radio needs the live flag (no scrubber on the lock screen) and P0 needs the real rate. That is a
small, additive change in its own PR, copied into every app by binding generation; no other app changes
behaviour.

## Live radio, practically

Things that decide whether this feels solid or flaky:

- **Identity.** A saved station is stored by Radio Browser's stable station UUID, with the stream address kept
  only as a cache. When a play fails, or the cached address is older than a day, the station is looked up again
  by UUID and its current address used, so favourites survive a station moving its stream.
- **Dead streams.** A directory listing doesn't mean a station is on air. Ask Radio Browser for checked stations
  only (`hidebroken`, `lastcheckok`), try the resolved stream address then the original, give up after a short
  timeout with a clear "This station isn't broadcasting right now", and remember failures on the phone so a
  station that fails twice sinks in lists. Real multi-stream failover only exists where a station lists several
  streams, which Radio Browser mostly doesn't; I won't promise more than that.
- **HTTPS and HTTP.** Prefer an `https` address when the station has one. Plain `http` streams still play
  (Soundscape already allows them), and the station's details say the stream isn't encrypted. If App Store
  rules later need it, the app-wide exception narrows to `NSAllowsArbitraryLoadsForMedia`.
- **Codecs.** AVPlayer plays MP3, AAC/HE-AAC and HLS, which is most stations. Soundscape checks the codec Radio
  Browser reports *and* the stream's actual content type when it connects, and hides or refuses what AVPlayer
  can't play (Ogg Vorbis, Opus, FLAC streams), rather than adding a decoder.
- **Coverage, said honestly.** Radio Browser is a large community directory, not a complete list of the world's
  stations. Screens say "stations listed in Radio Browser" and show counts per country; an empty country says
  nobody has listed one yet, never that none exist.
- **Being a good citizen of Radio Browser.** It's volunteer-run: pick a server from its published server list
  (and move to another when one fails) rather than hard-coding one, and send a descriptive User-Agent with the
  app's name and version only.
- **Reconnection.** After a network drop or an interruption (a call), rejoin live rather than resume a stale
  buffer. The existing interruption handling is the place for it.
- **Now-playing titles.** Many Icecast and Shoutcast stations send the current song (ICY metadata). AVPlayer
  exposes it on some streams and not others, so it's shown when present and never relied on.
- **No recording.** Soundscape plays broadcasts; it never records, stores or rebroadcasts them.
- **Privacy (decision 5).** Searching and browsing send Radio Browser the search terms and the phone's IP
  address, as any web request does. Separately, Radio Browser offers a *click* endpoint: calling it with a
  station's UUID adds one to that station's popularity count, and Radio Browser sees that request come from the
  phone's IP address at that time. That is listening activity leaving the phone. So:
  - Click reporting is a setting, **Help rank stations (tells Radio Browser which stations you play)**, and
    playing never depends on it: the stream address comes from search results, not from the click call.
  - When on, it sends the station UUID only, once per play (not per reconnect), with the plain User-Agent and
    no account, device id, history or location.
  - My suggestion is off by default, with a one-line explanation in Discover's Live Radio section the first
    time, matching the roadmap's private-by-default principle. Reuben may prefer on, to support the directory.
  - Favourites, recents and history stay on the phone.

## Podcasts, practically

- **Directories.** Apple's search API needs no key and has the widest coverage; the Podcast Index is the open,
  independent one and carries Podcasting 2.0 data. Apple search and subscribing by feed address always work
  with nothing set up. The Podcast Index is optional and used only when a key is set.
- **Podcast Index credentials.** Unlike Jamendo's client ID, the Podcast Index *secret* is a credential (it signs
  every request). Key and secret are stored in the Keychain through the existing `forge-services` Keychain
  service, never in `podcasts.json`, a backup export or a log; requests log only that a directory was asked,
  never the signed headers. Removing the key turns the directory off and nothing else.
- **Downloads.** Streaming and keeping offline are separate states, shown separately on every episode.
  - In P1 every download is one the person starts (an episode, or "Download" on a show). Automatic downloads
    are P2 and per show, off by default.
  - Not every enclosure is a plain public file. A feed can be private or paid (a token in its address), an
    episode can be marked blocked or locked (`itunes:block`, `podcast:locked`), and a server can refuse or
    expire a download. Soundscape keeps a feed's address private (never logged, and exported only in an OPML
    file the person asks for), respects a refused download with a clear message, and keeps the reason with
    the episode.
  - Episode downloads have their own logic (per-show storage, delete after playing in P2), sharing only the
    store and the Downloaded screen with music. Music downloads keep their `may_download` rights check unchanged.
- **Refresh.** On opening and pull-to-refresh in P1; background refresh and automatic downloads in P2 (iOS
  background time is limited, so "new episodes are there when I open the app" is the honest promise).
- **Transcripts** reuse the lyrics view; **chapters** come in P4 as planned.
- **Portability:** OPML in and out from P1; gPodder sync with a Nextcloud server stays P3.
- **Benchmark.** android-ports.md asks each port for a benchmark folder (AntennaPod's features with Expected and
  Seen columns, walked on the Android emulator). That starts with P1, at `examples/soundscape/docs/benchmark/`.
- **Extraction comes last.** Because Soundscape builds its own feed handling, P1 doesn't wait on Commonplace.
  Extracting shared feed code waits for both: podcasts passed on the iPhone, and Commonplace's deferred feed
  check (#345).

## Milestones

| Order | Milestone | What it delivers | Phone check |
|---|---|---|---|
| Gate | Owed Soundscape checks | Step 0 of Soundscape in [app-plan.md](../../../docs/app-plan.md#soundscape), the one list | Already written |
| 1 | **L1 Live Radio** | Now Playing helper gets a live flag (framework PR). Radio Browser client in Rust (search; browse by country, language, genre; top stations; codec filter; station UUIDs), live-station play state, favourites and recents. Rust PR, then a screens PR (Discover section, Search results, Library "Stations", Live state in mini-player and Now Playing) | One MP3, one AAC and one HLS station, each played on the iPhone; a station from Japan and one from the UK; save one and replay it after a restart; lock screen shows it as live |
| 2 | **L2 Live Radio, finished** | Re-resolve by UUID, failure memory, dead-stream message, codec check on connect, now-playing titles where sent, "Around the World" shelf, sleep timer on live, rejoin after interruptions, click-reporting setting | A phone call mid-stream; Wi-Fi to mobile data and back; a station that has stopped broadcasting; an unsupported-codec station refused clearly |
| 3 | **P0 player features** | Speed (and the real rate on the lock screen), skip amounts, saved position per item (music benefits too) | Short listening check |
| 4 | **P1 Podcasts** (only when Reuben asks) | Soundscape's own feed handling (feed-rs), Apple search and feed addresses, optional Podcast Index (Keychain), subscriptions, episodes, stream and user-started download, played state, OPML. Tab review here | Subscribe to a fixture feed from the laptop and one real show; download, play offline, resume; a refused download shows its reason |
| 5 | P2 to P4 | Download rules, gPodder sync, chapters and transcripts, as approved | As in android-ports.md |
| Later | Feed extraction | Shared feed code into the framework, once podcasts and Commonplace's feeds have both passed on the phone | Both apps' feed checks again |

Each Rust PR is tested on Linux with fixture responses (no live network in tests); screens follow in their own PR,
as the design programme asks.

## My honest recommendation on sequencing

1. Run the owed Soundscape phone checks first (app-plan.md step 0). Radio never jumps ahead of them. Live radio adds a new way for playback to fail; it's much easier
   to judge once the music player is known to be good.
2. Then **L1 + L2 Live Radio**. It's the cheapest way to make "listen to the world" real, and it tests the new
   Live player state before podcasts add more to the player.
3. Then **P0**, then **P1 Podcasts** when you ask for it. Podcasts are the larger build (subscriptions, refresh,
   downloads), so they come once the player work they depend on is done.
4. The M12 to M18 roadmap stays as it is; I'd slot L1/L2 between the Jamendo work and M12, and podcasts after M13
   (portability) so OPML lands beside M3U and backups rather than on its own.

## Decisions for Reuben

1. **Order:** live radio after the owed checks, then P0, then podcasts (recommended, though live radio is a new
   source ahead of player features); or P0 first, then live radio, then podcasts (strictly "player before sources").
2. **Names:** "Stations" for generated mixes and "Live Radio" for broadcasts (recommended), or something else.
3. **Podcast Index key:** register for a free one (recommended, for independent shows), or Apple search only.
4. **Tabs:** keep four and review at P1 (recommended), or add a Podcasts tab from the start.
5. **Radio Browser click reporting:** off by default with a setting (recommended), or on by default.

## Review changes (28 November)

Reuben reviewed the first version and asked for six changes, all made above:

1. Feed handling is built in Soundscape first; extraction into the framework only after both apps pass on the phone.
2. No blanket "episodes are always downloadable": user-started downloads, streaming and offline kept apart,
   publisher restrictions respected, podcast download logic of its own.
3. The Podcast Index secret lives in the Keychain, never in app JSON or logs; the directory is optional.
4. Radio Browser click reporting is explained, minimal, a setting, and never needed for listening.
5. Live radio stores station UUIDs and re-resolves streams, prefers HTTPS, checks codec and content type, and its
   phone checks name MP3, AAC, HLS, interruptions, network changes and dead stations. Coverage is described honestly.
6. One gate: step 0 in app-plan.md, which this page points to instead of keeping its own list.

## Sources

- [Radio Browser API documentation](https://api.radio-browser.info), [RadioDroid](https://www.github.com/segler-alex/RadioDroid), [MusicPod releases](https://www.github.com/ubuntu-flutter-community/musicpod/releases)
- [Podcast Index API documentation](https://github.com/Podcastindex-org/example-code) and [API docs mirror](https://github.com/jamescridland/docs-api)
- [Apple developer forums: AVPlayer and ICY metadata](https://developer.apple.com/forums/thread/102200), [Shoutcast metadata in Swift](https://developer.apple.com/forums/thread/650170)
- Earlier plans in this repository: [android-ports.md](../../../docs/research/android-ports.md), [design.md](design.md), [roadmap.md](roadmap.md), [app-plan.md](../../../docs/app-plan.md)
