# Soundscape design: from functional screens to a music app

Reuben's direction (2026-11-22): Weather is the benchmark. Soundscape should feel like a carefully designed
iPhone music app, close to a Jamendo front end, with the navigation, artwork treatment, Now Playing and motion
of the best native music apps as inspiration, and its own personality. No copied assets or exact layouts:
Soundscape takes the native patterns (tab bar, bottom accessory, sheets, zoom transitions), not another app's
look.

This page is the visual specification (S1), the audit of today's screens, and the plan for S2 to S6. The
boundaries from [architecture.md](architecture.md) are unchanged:

- SwiftUI owns presentation. No visual decision moves into Rust.
- Rust keeps the music domain: library, collection, history, queue, saved playback, offline.
- `TakePlayer` and AVPlayer stay the playback engine: queue persistence, gapless and crossfade, sleep timer,
  lock-screen controls through `ForgeNowPlaying`, interruptions.
- ForgeUI supplies the shared parts (`ForgeThumbnail`, `ForgeRating`, `ForgeRow`, `ForgeStateView`,
  `forgeBar`); Soundscape adds only what is its own identity, in its own files.

## Screen audit (main at 161781b, 2026-11-22)

| Screen | Today | Problem | Goes to |
|---|---|---|---|
| Root | One `NavigationStack` titled Library; the mini-player floats over it through `forgeBar` | No navigation structure: Discover, Favourites, Playlists, Recently Played and Downloaded are rows inside the library | S2: four tabs |
| Library home | `YourMusicSection` rows, Played Lately, a segmented Songs / Albums / Artists picker over one long list | Reads as a database browser; artwork is 40 pt and secondary | S2: Library tab (categories, then artwork grid); S4: shelves and detail pages |
| Search | `.searchable` on the library home; Jamendo and the Archive below your music | Good behaviour, hidden: search only exists on the library screen | S2: its own tab, same behaviour |
| Discover | Segmented Jamendo / Archive switch, Browse by Genre row, a list of tracks | Two API result lists | S2: its own tab; S5: editorial shelves |
| Mini-player | Full-width bar with a hairline progress bar, `.bar` material | Fine functionally; not attached to anything, and it covers content when there are tabs | S2: the tab bar's bottom accessory (iOS 26), `forgeBar` before iOS 26 |
| Now Playing | A sheet holding a `List`: 260 pt artwork, title, slider, buttons, stars, a Lyrics button, Up Next; nine toolbar buttons | The most important screen looks like a settings page; actions are spread over the toolbar | S3: immersive full-screen player |
| Lyrics | A separate sheet over Now Playing | Breaks the player apart | S3: shown in place inside Now Playing |
| Album, artist, playlist, track | `List` screens with a header row | Usable; plain | S4 |
| Empty, loading, failed | `ForgeStateView` everywhere | Good; keep | S6 checks each new screen |

## Visual specification (S1)

### Character

Soundscape is a personal record collection that also opens onto free catalogues. Its personality comes from
**the music's own artwork and colour**, a purple accent, rounded type for its own headings, and quiet motion.
Chrome stays native: system backgrounds, the tab bar, sheets, menus, materials.

### Colour

- **Accent:** ForgeUI's `purple` (`.forgeTheme(ForgeTheme(accent: .purple))`). Purple is free among the apps
  (Pocket Library orange, Good Dog green, Commonplace indigo, FieldKit teal, Sketchbook pink) and reads as
  music without imitating anyone's red. Used for tints, the selected tab, Play and Shuffle text, sung lyrics in
  light screens.
- **Artwork palette** (`ArtworkPalette.swift`, Foundation only, tested on Linux): from a 24-pixel thumbnail of
  the playing artwork, the average colour and the most saturated colour, with brightness clamped so white text
  keeps at least 4.5:1 contrast over the darkest stop. A song without artwork uses the hue of its stand-in (the
  coloured initial `Artwork` already draws), so every song has a colour. Changes animate over 0.6 s.
- **Over artwork colour** (Now Playing, lyrics): always dark appearance, white primary text, white at 60% for
  secondary, as Weather does over its skies.

### Type

| Role | Style |
|---|---|
| Screen titles | System large titles (native) |
| Soundscape's own headings (Listen greeting, shelf titles) | `.title2.bold()` / `.title3.bold()` in the rounded design |
| Now Playing title / artist | `.title3.bold()` / `.title3` at 60% white, one line each, marquee not used |
| Track rows | `.body` title, `.subheadline` secondary |
| Times | `.caption.monospacedDigit()` |

All text styles are Dynamic Type styles; nothing is a fixed point size except symbol glyph sizes in the
transport, which scale with `@ScaledMetric`.

### Artwork

- Every picture goes through `Artwork`, which frames it in ForgeUI's `ForgeThumbnail` and keeps Soundscape's
  coloured-initial stand-in (#413).
- Sizes: 44 pt in rows, 56 pt in the mini-player's expanded state (40 inline), 150 pt on shelves, grid cells fill
  their column, Now Playing fills the width less 32 pt each side.
- Corners come from `ForgeThumbnail` (a tenth of the width up to the theme's card radius); artists are circles.
- Large artwork has a soft shadow (`radius 24, y 12`, black at 35%) only on Now Playing and album headers.

### Surfaces

- Lists and grids sit on the system background; shelves are plain scroll views, not cards.
- Floating controls (mini-player, queue sheet) use the system's own materials: the iOS 26 tab bar accessory is
  Liquid Glass by itself; before iOS 26 the mini-player uses `.regularMaterial`.
- Now Playing: artwork palette gradient, the artwork itself blurred very large over it at 50%, and a dark scrim
  at the bottom third so controls always read.

### Spacing

A 4-point scale: 4, 8, 12, 16, 20, 24, 32. Screen edges 16 (20 on shelves to line up with large titles), shelf
item gap 12, section gap 24, Now Playing edges 28.

### Motion (`SoundscapeMotion`)

| Moment | Motion | Reduce Motion |
|---|---|---|
| Mini-player opens Now Playing | Zoom transition from the mini-player's artwork (iOS 18+); a cover slide before | Plain cover |
| Track changes | Artwork cross-fades; palette animates 0.6 s | Instant swap |
| Pause | Artwork eases to 86% size with a spring (`response 0.45, damping 0.8`); play brings it back | No scaling |
| Scrubbing | The bar thickens from 6 to 12 pt while dragging; times brighten | Same, no animation |
| Transport taps | SF Symbol bounce on next / previous | No bounce |
| Lyrics | Line being sung brightens and scrolls to the centre | Same, no scale |

Haptics stay ForgeAppKit's small vocabulary (success / failure); no new kinds.

## Navigation (S2)

Four tabs, each its own `NavigationStack` with the same destinations (albums, artists, playlists, songs):

| Tab | Root | What it holds |
|---|---|---|
| Listen | `ListenHome` | A greeting, Continue Listening (the saved queue), Hear Nonstop Music (radio), Recently Played shelf, your playlists, recently added albums, Popular on Jamendo when a client ID is set |
| Library | `LibraryTab` | Category rows (Playlists, Artists, Albums, Songs, Favourites, Downloaded, To Pick, Recently Played), then recently added albums as an artwork grid; import and take server in the toolbar |
| Discover | `DiscoverView` | Jamendo and the Internet Archive as today (S5 makes it editorial) |
| Search | `SearchTab` | One search for your music, Jamendo and the Archive (as today), with genres to browse before you type |

The Search tab uses the search role, so iOS 26 shows it as the separate search button. The mini-player is the
tab bar's bottom accessory on iOS 26 (it shrinks inline when the tab bar minimises on scroll), and a
`forgeBar` above the tab bar before iOS 26. With nothing playing it says "Not Playing", like the system's own
music apps, so the accessory never jumps in and out.

## Now Playing (S3)

Full screen, dark, over the artwork palette:

1. A grabber and a chevron to close (swipe down closes too).
2. The artwork, as large as the width allows; or, with lyrics on, a small artwork row and the lyrics filling
   the space, the sung line bright.
3. Title and artist on the left; favourite and a More menu on the right (Add to Playlist, Go to Album, Go to
   Artist, More by Artist and More in This Genre for Jamendo songs, Rate This Take for generated songs, Stop).
4. The scrubber: a thin bar, elapsed time left, time remaining right.
5. Previous, play / pause, next: large glyphs without circles.
6. The volume slider (the system's, so it also follows the hardware buttons).
7. Lyrics, AirPlay and Queue along the bottom; sleep timer and Between Songs in the More menu, with the sleep
   note under the scrubber while it runs.

The queue is a sheet with shuffle and repeat at the top and Up Next below (tap to jump, swipe to remove). A/B
comparison keeps its own A / B switch in place of the transport.

## Radio (Reuben, 2026-11-22 20:28 and 20:36)

Radio is nonstop music: a station is an endless playlist generated from a seed (a genre, a song or an artist),
queued a batch at a time, so there are many stations for free. It's a smarter shuffle, so the mix lives in Rust
(`rust/src/station.rs`, tested on Linux), and Swift only starts it, shows it and keeps the queue topped up
(`Radio.swift`).

**Built (step 1):**

- **Seeds:** a genre (your library's genres and Jamendo's), a song (Start Radio in Now Playing's More menu), or
  an artist (your most played).
- **The mix:** songs from your library in the genre (or by the artist), weighted up by Like (+3), favourite
  (+2) and plays (+0.5 each, up to +3), and two in five new to you: unplayed songs, and Jamendo's in the
  station's genre when there is a client ID. A Jamendo song that isn't in the library seeds from Jamendo's
  "more in this genre".
- **Rules:** a disliked song never plays; the same artist doesn't come back within four songs (across
  batches too); a station never repeats a song until it has played everything; the mix is repeatable from a
  seed number.
- **Like and Dislike:** two buttons in Now Playing while the radio plays. Dislike also skips the song. Both
  are kept in `collection.json` (`feelings`), for any track id, library or catalogue.
- **Hear Nonstop Music:** a shelf of station tiles on Listen: your top genres, your most played artists,
  then Jamendo's genres, eight in all.
- **Continuous play:** the radio asks Rust for the next 10 songs while fewer than four are still to come, so
  there is always a batch in hand, and AVPlayer already loads the next song before the current one ends
  (gapless). Playing anything else ends the radio.
- **When a top-up fails:** the station stays on and tries again after 2, 5, 10, 20 and 40 seconds, saying so
  in Now Playing and on Listen. When the retries run out it offers Try Again and Stop Radio. A station with
  nothing that fits (`NotInLibrary`) says so instead of retrying; a Jamendo failure with nothing else to play
  is Jamendo's error, so it is retried, not mistaken for an empty station.
  The radio owns the top-up and its retries, not the screen that asked for it, so the queue moving on
  or a view going away never cuts a wait short or restarts the count; only Stop or a new station ends it.

**Next:**

- **A smarter mix, grown in `station.rs`:** related genres and artists, tempo and energy where the
  metadata has them, a gentler path from familiar to new, and Like / Dislike / skips steering the very next
  batch. No separate recommendation service.
- **Cache ahead for weak connections:** download the next few radio songs in the background when their rights
  allow it (`may_download`, as Discover's downloads already check), and drop them once played unless kept.
  Songs whose licence allows downloading also get Keep on Phone from the radio.
- **Live radio (step 2):** open internet radio through the Radio Browser directory (free, community-run,
  no key): browse by genre, country and language, search stations, play their streams through the same
  player, with categories on a Radio screen. Shown apart from generated stations, because a live stream has
  no queue, no skip and no Like.

## Podcasts stay on the roadmap

Podcasts are planned as a first-class listening experience, not a corner of the music tabs: the AntennaPod
counterpart in `docs/research/android-ports.md` (P0 to P4: subscriptions, episodes, downloads, play position,
queue). The four music tabs are not the permanent shape of the app. When podcasts arrive, the tab set is
reviewed then: Listen can hold both (continue an episode, new episodes beside radio), and podcasts get their
own place rather than being pushed into Library or Discover.

## Milestones

| Step | What | State |
|---|---|---|
| S1 | This spec; accent; `ArtworkPalette`; `SoundscapeMotion`; artwork sizes | In this PR |
| S2 | Four tabs; Listen home; Library tab with Songs, Albums (grid) and Artists screens; Search tab; mini-player accessory | In this PR |
| S3 | Immersive Now Playing; queue sheet; lyrics in place; scrubber; volume and AirPlay | In this PR |
| S4 | Library: album and artist pages with large artwork, Play / Shuffle, more shelves | Next |
| S5 | Discover: genre tiles, Popular This Week shelf, artists, Archive collections, recently discovered | Next |
| S6 | Native finish: Dynamic Type at the largest sizes, VoiceOver order, loading and empty states, interruptions | Next |
| R1 | Radio (above): station mixer in Rust, Like and Dislike, Hear Nonstop Music on Listen, continuous queue | In this PR |
| R2 | Radio: cache songs ahead (downloadable rights only); live open radio (Radio Browser) | Next |

## Phone checks (next combined Device Lab sitting, about 10 minutes)

1. Open Soundscape: four tabs, Listen first; the mini-player above the tab bar says Not Playing.
2. Play an album from Library > Albums; the mini-player shows it; scroll a long list and the tab bar and
   mini-player shrink together.
3. Tap the mini-player: Now Playing zooms open from it; the background takes the artwork's colours; pause
   shrinks the artwork.
4. Scrub, skip, open Queue, jump to a song, swipe one away; open Lyrics on a song that has them.
5. Radio: on Listen, tap a Hear Nonstop Music tile; songs keep coming past ten; Dislike skips; Start Radio
   from Now Playing's More menu.
6. Lock the phone: lock-screen controls still work; the queue survives closing the app (unchanged behaviour,
   check it wasn't broken).
7. Light and dark mode, and the largest Dynamic Type size on Listen and Now Playing.
