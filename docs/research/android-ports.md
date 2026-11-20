# Open-source Android apps, answered by our own apps

Plan, 19 November 2026; approved by Reuben on 20 November 2026 (his decisions are under [Decisions](#decisions-20-november-2026)). Reuben asked on 2026-11-19 to grow existing ixOS apps into iOS counterparts of established open-source Android apps, to show that ixOS can carry a real product, and perhaps later to offer a proper port to those projects' communities. Breezy Weather and our Weather app are planned in their own thread and are not covered here. This page plans the rest of his shortlist: OpenTracks, Feeder and AntennaPod in full, and Etar and Fossify Gallery briefly.

No code, builds or phone runs come from this page. Each phase below becomes its own PR when Reuben asks for it.

## The short version

| Android app | Our app | Verdict | Order |
|---|---|---|---|
| Feeder (RSS reader, GPL-3.0) | Commonplace | **Do it.** Smallest gap, and it is already Commonplace's roadmap (folders, OPML, full articles) | 1 |
| OpenTracks (GPS tracking, Apache-2.0) | FieldKit | **Do it.** Good fit, the friendliest licence, and it brings a new capability (Bluetooth sensors) | 2 |
| AntennaPod (podcasts, GPL-3.0) | Soundscape (not Pocket Library) | **Do it, after Soundscape's pending phone checks and player work.** Biggest audience, biggest gap | 3 |
| Etar (calendar, GPL-3.0) | Good Dog / Checklist | **Not as a port.** Take the EventKit capability the roadmap already gives Good Dog; don't build a calendar app | No |
| Fossify Gallery (photos, GPL-3.0) | none | **No.** Apple's Photos already does it, and we have no app to grow into it | No |

The rule behind the order: start where our app is closest to the reference and the evidence comes quickest, and never let a port become a new app ([roadmap: expand an existing app before adding a new one](../roadmap.md)).

## Rules for every port

### Licence

ixOS and its apps are dual-licensed MIT OR Apache-2.0. Code can move from a permissively licensed project into a GPL-3.0 one, but not the other way: copying GPL-3.0 code into Commonplace or Soundscape would make that app GPL-3.0. So:

- **GPL-3.0 projects (Feeder, AntennaPod, Etar, Fossify): clean room.** We work from what the app does as a user sees it (running it, its website, documentation, changelogs and store listings) and from public format specifications (RSS, Atom, JSON Feed, OPML, the gPodder API, iCalendar). We copy none of its source code, strings, translations, icons, artwork or test fixtures. The evidence is observable behaviour, documentation and public format specifications. **We don't read Feeder's or AntennaPod's source** while working on the matching feature: restating source-derived explanations in our own words does not make an implementation clean room. If a question can't be answered from the running app, its documentation or a file it exports, the answer is to test the app (for example, export with a setting on and off and compare the files), not to read its code. This is how Commonplace was built against NetNewsWire, and it is the same here.
- **Apache-2.0 (OpenTracks): reuse is allowed, with conditions** (approved 2026-11-20, subject to checking the exact licence at the source commit). We may translate an algorithm or take a test file, as long as the file carries the licence notice, we say what we changed, and any NOTICE text is kept. In practice little transfers, because OpenTracks is Android Java and we write Rust, but its recording rules and sample exports are useful. Each borrowed piece is listed in the app's README with its source commit.
- **Interoperability is fine.** Reading and writing another app's export files and talking to the same sync servers is not copying code. File formats are not covered by the app's licence.
- **Before any phase starts,** the licence is checked against the LICENSE file at the commit we look at, and recorded with the date and commit (as in the [external projects evaluation](external-projects/evaluation-framework.md)). The licences on this page come from Reuben's research and the projects' pages, not from reading each LICENSE file yet.

A note for later: a port the original project could adopt as its own iOS app would most likely have to be GPL-3.0, and the Free Software Foundation's view is that the App Store's terms conflict with the GPL (VLC was pulled from the App Store over this in 2011). That only matters if a maintainer wants an official port, and copyright holders can grant an exception. It is not a reason to hold back on our own apps.

### Names and branding

- Our apps keep their own names, icons and looks. No Android project's name, logo, icon, colours or screenshots go into an app, its icon, its home-screen name or a store listing.
- Plain factual statements are fine where they help a user: "imports OPML exported from Feeder", "reads OpenTracks GPX and KMZ files". Not "Feeder for iPhone".
- **Maintainers are not contacted for now** (Reuben, 2026-11-20 07:38): we build first, and contact them later only if an app gets close to something worth showing. When that happens it is selective and never blocks development. Reuben makes the contact; we introduce the work honestly: an independent native iOS app, no GPL source copied, their project credited as the product reference, compatible import and export, possible future collaboration. We don't ask for an endorsement. If a maintainer wants to adopt an iOS app officially, licensing and App Store distribution are discussed separately.

| Project | When | Purpose |
|---|---|---|
| Feeder | After the C3 prototype | Confirm OPML interoperability and per-feed settings |
| OpenTracks | Before borrowing any code | Attribution, compatibility, possible collaboration |
| AntennaPod | Before P3 | Confirm gPodder interoperability, discuss the iOS counterpart |

### How we prove it

Each port gets a benchmark folder like Commonplace's (`examples/<app>/docs/benchmark/`): a spec of the reference's features with Expected and Seen columns, a gap log of what ixOS was missing, and a walk a person can do side by side. The Android phone is under repair, so the reference is walked on the Android emulator (already set up for Pocket Library's Android stage 0) with the app installed from F-Droid, or from the project's documentation where the emulator can't do it (GPS, Bluetooth). A feature is "matched" only after it passes on the iPhone; Linux tests alone are not enough.

The value Reuben wants to show is that a real, mature product can be answered by an ixOS app with its logic in Rust, and that the framework pieces that fell out of it are reusable. So each benchmark also records what was extracted into the framework and which other app used it.

## 1. Feeder → Commonplace

### Where Commonplace is

Commonplace already reads RSS, Atom and JSON Feed, with HTTP caching, timelines (All Unread, Bookmarks, All Articles, each feed), read and unread, bookmarks, SQLite, the ForgeReader reader, full-text search over saved articles, settings (order, hide read, summaries, where links open, font), the in-app web view, background refresh about hourly, and new-article notifications with Mark All as Read. Folders and OPML are its planned slice 3.

### Gap against Feeder

Feeder's features as its project describes them, to confirm in the walk:

| Feeder | Commonplace today | Gap |
|---|---|---|
| OPML import and export, including per-feed and global settings | none (slice 3) | **Main gap** |
| Tags (feeds grouped) | none (folders in slice 3) | Folders |
| Full article fetched from the web page, per feed by default | summary or what the feed carries | Article extraction (also on the roadmap) |
| Per-feed settings: notify, full text by default, open articles with reader/browser | global settings only | Per-feed settings |
| Block list (hide articles matching a pattern) | none | Filters |
| Text to speech of an article | none | Read aloud, with Pocket Library's Read Aloud as the precedent |
| Background sync with conditions (Wi-Fi only, charging), old articles cleaned up | hourly background refresh, no conditions, no clean-up | Refresh conditions and retention |
| No accounts | no accounts | Same |

### Data compatibility

Feeder's OPML export (since 2.5.0) is standard OPML with its own namespace, `https://nononsenseapps.com/feeder`: per-feed attributes such as `notify`, `fullTextByDefault` and `openArticlesWith`, and a `settings` element with global settings and block-list patterns. Commonplace will:

- import any standard OPML (feeds, and outlines as folders), with a checklist of feeds to pick as Feeder and AntennaPod both do;
- read Feeder's per-feed attributes where Commonplace has the same setting, and say on the import summary what was not carried over;
- treat imported block patterns as text to match, never as code, since Feeder itself warns that a hand-edited pattern can crash it;
- export standard OPML that any reader imports, with Commonplace's own settings under a namespace of our own.

Saved and bookmarked articles are not in Feeder's OPML, so they don't move. That is said on the import screen, not discovered later.

### Phases

1. **C3: folders and OPML** (already Commonplace slice 3; **built 2026-11-20**, Rust tested on Linux, Swift and the phone check waiting for the next combined Device Lab build): a folder hierarchy; OPML import with a preview and feed selection; OPML export; Feeder-compatible per-feed attributes; duplicate detection; a clear list of settings that were not carried over. Requirements Reuben set:
   - OPML is an **interchange format, not the internal model**. Commonplace keeps feed addresses, folders and its own settings in its SQLite library; OPML is read and written at the edge, and nothing inside depends on Feeder's attribute names.
   - Importing is **non-destructive**: importing the same OPML twice adds no duplicate subscriptions (feeds match by normalised address), and an import never removes or changes a feed the person already has, beyond filling in its folder when it has none.
   - **Round trip**: export from Feeder, import into Commonplace, export again, and the folders and supported settings are kept. Tested on Linux with our own fixtures, and once on the phone.
   - Imported filters and patterns are **untrusted input**: validated, kept as plain text and never executed.
   Fixtures are OPML files we write ourselves, including the Feeder namespace, never files copied from Feeder's repository.
2. **C4: full articles** (**built 2026-11-20**): fetch and extract the article from its page, per feed (the roadmap's "full article extraction"), and Load Full Article for one article. The extractor is `dom_smoothie` (MIT), [assessed](external-projects/assessments/reading/dom-smoothie.md) before it went in. Article addresses come from the feed and are untrusted: a page is never fetched from this device or its networks (loopback, private, link-local, carrier-grade NAT and Tailscale, `.local` and similar names, checked as written and after the name is looked up), except on the feed's own host; the body is read as a stream that stops at 5 MB, and the whole fetch has a 20-second deadline. **Open gap:** redirects are not checked hop by hop, because URLSession and ureq follow them inside the transport and `forge_net` can't turn that off yet, so a public page that redirects to a local address is still fetched. Closing it needs a `forge_net` request option to not follow redirects (URLSession delegate, ureq config), as its own framework PR.
3. **C5: per-feed settings and filters**: notify, full text, how to open; block list; refresh only on Wi-Fi; keep only the last N articles per feed unless bookmarked.
4. **C6: read aloud**: a third user for the speech pieces Pocket Library and Urdu Naturally use (the ForgeSpeech extraction already named as a candidate).

Phone check for each phase: import a fixture OPML, see the folders and the per-feed settings, export it, re-import it into a fresh library.

## 2. OpenTracks → FieldKit

### Where FieldKit is

FieldKit records a trip as a GPS track with filtering rules in Rust, keeps running distance and climb, survives the app being killed mid-trip, records in the background, places text, photo and voice notes, shows the track on Apple Maps with altitude and pace charts, and exports GPX 1.1 and GeoJSON with notes as waypoints.

### Gap against OpenTracks

| OpenTracks | FieldKit today | Gap |
|---|---|---|
| Activity types (walk, run, cycle and more) and statistics per type | one kind of trip | Activity types, totals by type and over time |
| Bluetooth LE sensors: heart rate, cycling speed and cadence, power, running speed and cadence | none | **New capability: CoreBluetooth.** Ledger record needed |
| Barometer altitude | GPS altitude | Core Motion altimeter (`CMAltimeter`) |
| Voice announcements at intervals (distance, time, pace, heart rate) | none | Spoken announcements while recording |
| Markers with photos | notes with photos and voice | Same idea, already there |
| Export GPX 1.1, KML 2.3, KMZ 2.3 (with photos); automatic export after each recording | GPX and GeoJSON, by hand | KML/KMZ, automatic export to a folder |
| Import of its own KMZ | no import | **GPX and KMZ import** |
| Indoor recording with sensors and no GPS | needs GPS | Sensor-only trips |
| No account, no network | the same | Same |

### Data compatibility

This is the strongest proof of the three, because the formats are open and the licence lets us test against real exports.

- **Import GPX 1.1 and KMZ/KML 2.3**, so a person moving from Android can bring their history. The importer is **general-purpose and standards-based**, not one that only works with OpenTracks: plain GPX 1.1 and KML 2.3 come first, and OpenTracks' extras are read when present. Duplicates are found from three signals, never the exported id alone (some exports omit or change it): the source's id when present (OpenTracks writes `opentracks:trackid`, and Gadgetbridge sets the same), a fingerprint of the content (start time, point count and a hash of the rounded points), and the person's confirmation when a match is likely but not certain. FieldKit keeps the source id with the trip so its own export can write it back. GPX from OpenTracks has only the points with a GPS position, so indoor trips come through KMZ.
- **Export** keeps GPX and GeoJSON and adds KMZ with the notes' photos, and writes OpenTracks' track id, so a FieldKit trip imported into OpenTracks is not duplicated either.
- Sensor values (heart rate, cadence, power) are stored per fix and written into the exports in the way OpenTracks reads them. Which extension elements those are gets written into the benchmark spec from its export files.
- Our Rust zip handling already exists (Pocket Library reads EPUB, which is a zip), so KMZ needs no new dependency.

### Phases

1. **T1: import and activity types**: GPX and KMZ import with duplicate detection; activity types; totals by type. Fixtures: exports made from OpenTracks on the emulator (Apache-2.0, attributed), plus our own.
2. **T2: KML/KMZ export and automatic export** to a folder the person picks in Files.
3. **T3: spoken announcements** every N minutes or kilometres, in the background, ducking other audio.
4. **T4: Bluetooth LE heart rate**, the first sensor, not complete until tested with an actual compatible strap, as a native service following [native-services.md](../native-services.md): Swift owns CoreBluetooth, Rust owns the values and the rules. A ledger record ships in the same PR, and "iPhone: Yes" waits for a run with a real strap.
5. **T5: the other sensors** (cycling speed and cadence, power, running) and indoor trips, only once T4 passes on the phone.

Barometer altitude can ride along with T1 or T3; it is small.

Phone checks: T1 imports a file from Files and shows it on the map. T3 needs a short walk with the phone locked. T4 and T5 need a Bluetooth heart-rate strap (or other sensor), which Reuben would have to have; until then they are "Rust tested on Linux" only.

## 3. AntennaPod → Soundscape

### Why Soundscape, not Pocket Library

Soundscape already has most of what a podcast app needs underneath: a library with sources that the player does not care about, a queue that survives restarts, offline downloads with a Downloaded screen, a mini-player and Now Playing, lock-screen controls through the shared Now Playing helper, a sleep timer, and gapless playback. A podcast episode is one more kind of audio source. Pocket Library is about books; its only audio is Read Aloud, which is speech made on the phone, not files from the internet. Putting podcasts in Pocket Library would mean building a second player.

The cost: Soundscape is being made into Reuben's personal music platform, and podcasts widen what it is. Podcasts are a first-class section, never mixed into the music library (Reuben, 2026-11-20):

- **Music**: albums, artists, tracks and playlists, as now.
- **Podcasts**: subscriptions, shows and episodes.
- **Shared player**: the queue, playback controls, downloads and the audio session.
- **Shared history**, with separate music and podcast statistics. Reuben also said on 2026-11-18 that player features come before more sources, and Soundscape's latest work still waits for a phone sitting. So this port starts only after those checks pass, and its first phase, P0, must improve the music player on its own even if podcasts never ship. The music-player work stays the priority.

Reuben approved podcasts in Soundscape on 2026-11-20.

### Gap against AntennaPod

| AntennaPod | Soundscape today | Gap |
|---|---|---|
| Subscribe by feed address or search a directory | none (music catalogues only) | Podcast feeds and search (the Podcast Index or Apple's search API) |
| Episodes per podcast, new-episode inbox, played and unplayed | songs, albums, artists | An episode list and played state |
| Playback position per episode, resume | queue position restored | Position per episode |
| Playback speed, skip silence, volume boost | none | **Speed first**; the others later |
| Skip forward and back by set amounts, per-podcast skip intro/outro | back 10 s on the lock screen | Configurable skips |
| Automatic download of new episodes, automatic delete after playing | downloads by hand | Download rules, background downloads |
| Chapters (in the file and Podcasting 2.0), transcripts | lyrics with timings | Chapters; transcripts could reuse the lyrics view |
| Queue, sleep timer | yes | Same |
| Statistics | listening history | Time listened per podcast |
| OPML import and export; database export; gPodder sync | none | See below |

### Data compatibility

AntennaPod's own documentation lists three ways out:

- **OPML**: subscriptions only (no played state, queue or settings). Standard OPML, so Soundscape imports it with a pick list and exports its own. This is phase P1 and covers most people.
- **gPodder sync** (gpodder.net, or the gPodder Sync app on a Nextcloud server): subscriptions and episode actions (played, position). Its API is public, so Soundscape can talk to the same server, and a person can use AntennaPod on Android and Soundscape on an iPhone against one Nextcloud. This is the strongest proof of compatibility. AntennaPod's docs say the queue, favourites and settings don't sync, and users report that history syncs only as episodes are played, so we promise subscriptions and positions only.
- **Database export**: an SQLite file tied to the AntennaPod version that wrote it. Reading it would carry played state and favourites, but its layout changes between versions and we would be reverse-engineering it. Not planned unless sync proves not enough.

Episode feeds are RSS with enclosures, and Commonplace already parses RSS. Apps may not depend on each other, so this is the framework's second real use for feed parsing: when P1 is built, the parser moves out of Commonplace into a framework crate in its own PR, after Commonplace's feed work has passed on the phone (the [extraction rule](../architecture.md#framework-principles)). Until then Soundscape doesn't copy Commonplace's code.

### Phases

1. **P0: player features music needs too**: playback speed, configurable skip amounts, a saved position per item. Soundscape benefits even if podcasts never land.
2. **P1: podcasts**: subscribe by address or search, the episode list, played state, streaming and downloading an episode, OPML import and export. The feed parser extraction above.
3. **P2: download rules**: new episodes downloaded in the background (the background transfers work from the roadmap), deleted after playing, limits per podcast.
4. **P3: gPodder sync** with a Nextcloud server, tested against AntennaPod on the emulator using the same server.
5. **P4: chapters and transcripts**, then skip silence and statistics.

Phone checks: P0 is a short listening check. P1 subscribes to a fixture feed served from the laptop, as Commonplace's benchmark does. P3 needs a Nextcloud with gPodder Sync, which we would have to set up (the Hetzner box could host it), so it is the last to be proven.

## 4. Etar: not worth porting

Etar is a calendar app over Android's own calendar store. On iOS that store is Apple's Calendar, reached through EventKit, and Apple's Calendar app is already good and free. A port would be a new calendar app (against the expand-before-new rule) in the most crowded category on the iPhone, with nothing of Etar's code or data to carry over. Etar's ICS import and export are worth having, but as features of the apps that plan things:

- Good Dog puts training sessions and appointments in Apple's Calendar through EventKit, as the roadmap already says (first user of calendar integration; FieldKit the second, for planned trips).
- Checklist gets due dates, recurring tasks and an agenda, as the roadmap says; it has none of these today (just titled items), so this is new work, not a port.
- ICS export of a Good Dog schedule or a FieldKit trip plan is a small Rust job once EventKit is in.

So nothing from Etar's feature list is lost by not porting it, and there is no community to serve on iOS that Apple's app doesn't.

## 5. Fossify Gallery: no

Fossify Gallery's value on Android is a private, ad-free gallery over the phone's files. On iOS the Photos app already offers albums, editing, a Hidden album behind Face ID, Recently Deleted and metadata, and PhotoKit only lets other apps work over the same library within limits. We have no app that is a photo manager to grow, so it would be a new app with very high competition and little to show that ixOS adds. If Fossify's other apps (notes, calculator, file manager) come up later, they can be judged one at a time; the [calculator expedition](expeditions/calculator/) is already one example.

## Recommended order and what each proves

1. **Commonplace answers Feeder** (C3–C6). Mostly planned work already; the proof is a person moving from Feeder by OPML with their settings. It also gives the framework a second user for read aloud.
2. **FieldKit answers OpenTracks** (T1–T5). The proof is a person importing their whole OpenTracks history and recording a ride with a heart-rate strap. It adds Bluetooth LE and Core Motion to the ledger.
3. **Soundscape answers AntennaPod** (P0–P4), after Soundscape's pending phone checks and with Reuben's yes on podcasts in Soundscape. The proof is AntennaPod on Android and Soundscape on an iPhone sharing subscriptions and positions through one Nextcloud. It extracts feed parsing into the framework.

One phase at a time, each in its own PR, phone checks in the combined Device Lab sitting as usual.

## Decisions (20 November 2026)

Reuben approved the plan and the order, and made the three decisions:

1. **Podcasts belong in Soundscape**, in their own Podcasts section with the shared player and queue; music and podcast libraries stay distinct. P0 improves the music player whether or not podcasts ship.
2. **Don't contact the original maintainers yet.** Build first; if an app gets close to something worth showing, contact them selectively as in the table under [Names and branding](#names-and-branding), without blocking development.
3. **Start Commonplace C3 now.** It is the first implementation PR. The rest of the roadmap stays documented here, with no parallel implementation work until C3 is finished and tested.

Every phase is its own PR, with automated tests and a check on an actual iPhone wherever iOS behaviour matters. Linux tests alone don't advance a phase. Etar and Fossify Gallery stay excluded.

## Sources

Reuben's research of 19 November 2026 (in the project chat), and:

- [Feeder 2.5.0 OPML changes](https://news.nononsenseapps.com/posts/2.5.0_opml/) and [Feeder on F-Droid](https://f-droid.org/en/packages/com.nononsenseapps.feeder/)
- [OpenTracks on Codeberg](https://codeberg.org/OpenTracksApp/OpenTracks)
- [AntennaPod: backing up your episodes](https://antennapod.org/documentation/general/backup) and [a forum thread on Nextcloud gPodder Sync](https://forum.antennapod.org/t/syncing-with-nextcloud-and-gpodder-sync-app/6508)
