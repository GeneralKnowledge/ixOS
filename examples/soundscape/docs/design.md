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
| Mini-player | Full-width bar with a hairline progress bar, `.bar` material | Fine functionally; not attached to anything, and it covers content when there are tabs | S2: a `forgeBar` above the tab bar on every screen |
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
- **Over artwork colour** (Now Playing, lyrics): always dark appearance, white primary text, white at 75% for
  secondary (`ArtworkPalette.secondaryTextOpacity`; the palette is darkened until that also reads 4.5:1),
  as Weather does over its skies.

### Type

| Role | Style |
|---|---|
| Screen titles | System large titles (native) |
| Soundscape's own headings (Listen greeting, shelf titles) | `.title2.bold()` / `.title3.bold()` in the rounded design |
| Now Playing title / artist | `.title3.bold()` / `.title3` at 75% white, one line each, marquee not used |
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
- Liquid Glass is for controls that float over content, never for the content itself (lists, artwork,
  shelves). On iOS 26 the mini-player is regular glass (`soundscapeGlass`), Now Playing's small round
  buttons are clear glass over the artwork colours (`nowPlayingCircle`), and the Undo banner is glass
  (ForgeAppKit). The tab bar, navigation bars, menus and sheets get glass from the system. On iOS 17 to 25
  each falls back to the look it had: `.regularMaterial` cards and a faint white disc.
- iOS shows glass only to an app whose binary records the iOS 26 SDK. Linux builds get that from our xtool
  fork (1.24.0-ixos.1); the laptop's upstream 1.24.0 recorded SDK 17.0, so every build before 2026-11-24
  ran in compatibility mode (the old opaque tab bar).
- Now Playing: artwork palette gradient, the artwork itself blurred very large over it at up to 45% (fainter
  over a bright cover, so secondary text keeps 4.5:1 over the cover's lightest region, not just its average),
  and a dark scrim at the bottom third so controls always read.

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
| Listen | `ListenHome` | A greeting (the subtitle under the Listen title on iOS 26), Continue Listening (the saved queue), Hear Nonstop Music (radio), Recently Played shelf, your playlists, recently added albums, Popular on Jamendo when a client ID is set |
| Library | `LibraryTab` | Category rows (Playlists, Artists, Albums, Songs, Favourites, Downloaded, To Pick, Recently Played), then recently added albums as an artwork grid; import and take server in the toolbar |
| Discover | `DiscoverView` | Jamendo and the Internet Archive as today (S5 makes it editorial) |
| Search | `SearchTab` | One search for your music, Jamendo and the Archive (as today), with genres to browse before you type |

The Search tab uses the search role, so iOS 26 shows it as the separate search button. The mini-player is a
`forgeBar` above the tab bar on every tab and every screen pushed in it. With nothing playing it says "Not
Playing", like the system's own music apps, so the bar never jumps in and out. (iOS 26's tab bar bottom
accessory was used first, but on the phone it went missing while browsing, 2026-11-24, leaving Listen's
Now Playing card as the only way back to the player.)

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

## Everyday Polish (S3.5, Reuben 2026-11-22 21:28)

Small touches that make Soundscape feel like a finished player, with no change to the playback engine:

- **Save as Playlist** in Up Next: every song in the queue, in play order, once each. Jamendo songs (a
  radio find) join the library, since a playlist shows library songs; takes that aren't in the library are
  left out.
- **One song menu everywhere** (`SongMenu`): Play Next, Add to Queue, Favourite, Add to Playlist, Start
  Radio and Go to Artist (More by This Artist for a Jamendo song outside the library), on library rows,
  search results, catalogue rows and Listen's tiles. A catalogue song joins the library before it becomes a
  favourite or goes in a playlist.
- **A heart on the mini-player** for the song playing (hidden when the tab bar minimises it). A radio find
  joins the library first, as in Now Playing.
- **Undo** on a banner (ForgeAppKit `ForgeBanner`) after taking a song out of Up Next, out of a playlist,
  deleting a playlist, or removing a song from the library. Rust keeps the last deleted playlist and the
  last removed song (with its file, edits and artwork) until the next one, so Undo also works from Rust's
  side, not just from what the screen remembered.
- **Saved stations**: Save Station (a pin) in Now Playing's radio row or a station tile's menu. Saved
  stations lead Hear Nonstop Music, newest first, and are never offered twice.
- **Recent searches**: the last ten, newest first, once each, shown before typing with Clear. A search is
  remembered when submitted, when a result plays, or when the person goes on from its results; half-typed
  text is not.
- **Download signs** on album tiles and rows and Listen's song tiles, as song rows already had: downloaded,
  on its way, or failed (an album shows the worst of its songs).

## Layered Navigation (Reuben, 2026-11-22 22:01)

A design principle for all of Soundscape, set before more queue UI: **navigation adds context; it doesn't
bury the person under screens.** Soundscape is connected, layered spaces, not a succession of unrelated
screens. It is built from SwiftUI's own stack, sheets and transitions, not a custom navigation framework.

**Three treatments.** Every destination declares one (`ListenRoute.style`):

| Treatment | Where | How it looks |
|---|---|---|
| Full screen | Areas and lists: the tabs, Albums, Songs, Playlists, Discover, settings | An ordinary push |
| Stacked | Things: an artist, an album, a playlist, an Archive item | Grows out of the tile or row it came from (zoom transition); pulling it back shows the surface underneath, which never moved |
| Overlay | Up Next, Add to Playlist; later filters and quick track details | Floats over the surface that opened it, translucent so that surface shows through; Up Next half raised leaves Now Playing visible and usable |
| Persistent | Now Playing and the mini-player | Always there, above every tab |

**The rules.** Each surface has an identity that doesn't depend on how it was reached
(`ListenRoute.surfaceID`: `artist:<id>`, `album:<id>`, `playlist:<id>` and so on), and every tab's stack
applies one rule to its path (`LayeredPath.settle`, tested on Linux):

1. Opening a surface that is already in the stack goes back to that one; nothing is copied. Artist, then
   one of its albums, then the artist again: the artist page comes back as it was left.
2. Opening a new surface stacks it on top.
3. Going back reveals the surface underneath with its scroll position and state (Show All, for example),
   because it was never closed.
4. Closing an overlay returns to the surface that opened it.
5. Opening the same thing again and again never makes a chain of copies.

The rule sits on the stack's path, so it holds however a push happens: a link, a song menu's Go to Artist,
or Now Playing's Go to Album. A screen that is new each time (a Discover search) has no identity and
always stacks.

**Depth, on purpose.** Translucency is a depth cue, used for overlays only, never the content itself.
Overlays share a corner radius (`SoundscapeSurface.overlayRadius`) and a thin material. With Reduce
Transparency they are solid; with Reduce Motion stacked surfaces use the ordinary push. Back buttons,
Done and Close stay for every gesture, so no gesture is the only way to navigate.

**Next.** Quick track details as an overlay from a playlist or album (today they push), and filters as a
small floating overlay when Library gets them.

## Album and artist pages (S4)

- **Album:** the artwork large, over a wash of its own colours that fades into the page; the artist (to go
  to), the album's genre; Play, Shuffle and the artist's station side by side; the songs by number; its
  length under them; Download; and More by the artist as a shelf. Play Next, Add to Queue and Add to
  Playlist are on its More menu.
- **Artist:** their picture and name over its colours; Play, Shuffle and their station; Top Songs (Rust
  ranks them: most played, then favourites and liked songs, never a disliked one) with Show All; their
  albums as a shelf; More on Jamendo when their songs came from there.

## Discover as an editorial page (S5)

Before a search, Jamendo's side of Discover is a page of shelves, not a list: Top This Week as a wide card
in its artwork's colours (it plays the week's list), Popular This Week, Start a Station (genre tiles that
start nonstop music), New in the person's three genres (Rust ranks Jamendo's genres by the library's
genres and plays, then Jamendo's own order), Recently Discovered (catalogue songs added lately), and
Browse by Genre. A search, and the Internet Archive, stay lists.

## Native finish, first pass (S6)

Done without the phone:

- VoiceOver: Magic Tap (two-finger double tap) plays or pauses anywhere; the two-finger scrub closes Now
  Playing; See All says what it shows ("See All Recently Played").
- Dynamic Type: at the accessibility sizes the Listen card and Top This Week put their words under the
  artwork, and tile titles wrap instead of cutting off. Bars of fixed height (the mini-player) stop at
  XXXL and Now Playing at accessibility 2, as the system player does; station tiles stop at
  accessibility 1.
- Calls, Siri and unplugged headphones already pause through ForgeKit's Now Playing helper, and the
  radio's retries survive the screen changing (Radio owns them).

Needs the phone: VoiceOver reading order on Now Playing and Listen, the largest sizes on every screen,
and light and dark mode.

## Milestones

| Step | What | State |
|---|---|---|
| S1 | This spec; accent; `ArtworkPalette`; `SoundscapeMotion`; artwork sizes | In this PR |
| S2 | Four tabs; Listen home; Library tab with Songs, Albums (grid) and Artists screens; Search tab; mini-player accessory | In this PR |
| S3 | Immersive Now Playing; queue sheet; lyrics in place; scrubber; volume and AirPlay | In this PR |
| L1 | Layered Navigation (above): surface identity, reuse an open surface, stacked zoom, translucent overlays | In this PR |
| S3.5 | Everyday Polish (above): Save as Playlist, one song menu, mini-player heart, Undo, saved stations, recent searches, download signs | In this PR |
| S4 | Library: album and artist pages (below) | In this PR |
| S5 | Discover: an editorial page before a search (below) | In this PR |
| S6 | Native finish, first pass (below); the rest needs the phone | In this PR |
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
8. Everyday Polish: in Up Next, swipe a song away and tap Undo, then Save as Playlist; long-press a song in
   Search for the full menu (Start Radio, Go to Artist); heart a radio song from the mini-player; pin a
   station and see it lead the shelf; search, clear Recent.
9. S4 and S5: open an album and an artist (colours behind, Top Songs, More by); Discover before typing
   shows Top This Week and the shelves; Start a Station from a genre tile.
10. S6: with VoiceOver on, two-finger double tap pauses and plays; Now Playing closes with the scrub; at
    the largest accessibility size, Listen's card and Discover's top card stack their words.
11. Layered Navigation: open an artist, one of its albums, then the album's artist link: you are back on
    the first artist page, scrolled where you left it, with one Back to go. Albums grow out of their tiles
    and pull back into them. Up Next half raised shows the player behind it; with Reduce Transparency on it
    is solid.
12. Liquid Glass (after the xtool fork is installed on the laptop): the tab bar and navigation bars are
    glass; the mini-player is a glass card that responds to a touch; Now Playing's heart, More, pin and
    thumbs buttons are clear glass; Undo is a glass banner. With Reduce Transparency on, all of them are
    solid.
