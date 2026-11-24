# Soundscape design specification

The complete Soundscape experience this programme works towards. Built on, not instead of,
[examples/soundscape/docs/design.md](../../examples/soundscape/docs/design.md) (S1 to S6, Radio, Layered
Navigation), which stays the record of what is built. Tokens: [DESIGN_TOKENS.md](DESIGN_TOKENS.md#2-soundscape-tokens-app-local-soundscapecolors-soundscapemetrics-soundscapemotion).
References: [DESIGN_REFERENCES.md](DESIGN_REFERENCES.md#soundscape-references).

## Identity

An expressive, immersive music app: a premium record collection, an editorial music magazine and a
listening room. Artwork-led, atmospheric, editorial, emotionally engaging. Not Spotify, not Apple Music.

- **Atmosphere comes from the music.** The artwork palette colours Now Playing, album and artist pages and
  the Continue Listening card. The fixed palette (ink, surface, ivory, lavender accent) is the frame.
- **Colour in the background, controls constant** (the Luma rule): artwork colour never tints buttons.
- **Playback first** (Plexamp): Listen starts with what to play now; the library is the source, not the
  home.
- **A record collection** (Doppler): albums and artists are first-class units.
- **Density without settings** (Marvis): Listen and Library are adjusted in place.

## What must not change

Existing behaviour and data compatibility of the Rust library, collection (an existing `collection.json`
must keep loading unchanged), history, queue, station mixer, Smart Playlists and offline store. Additions
the design needs (a stored shelf order, an artist ranking) are allowed only as their own feature PRs, with a
compatible, defaulted field and tests, merged before the visual PR (see the
[programme rule](README.md#visual-work-and-feature-work-stay-apart)). Also unchanged:
`TakePlayer` and AVPlayer (gapless, crossfade, sleep timer, lock screen, interruptions, saved queue); the
Layered Navigation rules (surface identity, reuse an open surface); downloads and licence checks; every
accessibility identifier used by `uitests/`.

## Screens

Each screen lists its states. "Offline" means no network: the library and downloads still work; catalogue
shelves say so calmly and offer what is available offline.

### A. Listen (priority 1)

| Part | Spec | Today | Work |
|---|---|---|---|
| Hero | Continue Listening as a large card in the current song's colours, with Play and the next two songs | Card exists | Composition and colour (V3.1) |
| Hear Nonstop Music | Station tiles: saved stations first | Exists | Keep |
| Made for You | Smart Playlists' built-ins (Recently Added, Most Played, Never Played, Forgotten Favourites) as artwork tiles | Built-ins exist in Library only | Surface on Listen (V3.1); no new Rust |
| Recently Played | Shelf | Exists | Keep |
| Favourite Artists | Circular artist tiles from play counts | Play counts exist; no shelf | V3.1 if play counts by artist are already exposed; otherwise a separate Rust PR first |
| Your Playlists, Recently Added, Favourite Songs, Popular on Jamendo | Shelves | Exist | Keep |
| Contextual order | Shelves the person uses rise; empty ones hide | Fixed order | V3.1: order by use. Time-of-day shelves need new Rust: not planned unless asked |
| Edit in place | A shelf's menu: Move Up, Move Down, Hide; Show Hidden at the bottom | None | Needs a stored shelf order: a separate Soundscape feature PR first, then V3.1 draws it |

States: **empty library** (one card: import music or explore Discover; radio from Jamendo if a client ID is
set); **loading** (shelf skeletons at their final size, no spinners in the middle of the page);
**offline** (catalogue shelves replaced by one quiet line "Jamendo needs a connection"; everything local
stays); **failed catalogue** (the shelf says what failed with Try Again; the rest of the page is unaffected).

### B. Now Playing (priority 2, the signature screen)

Full screen, dark, over the artwork palette. Layout as design.md S3. Additions:

- **Track change:** artwork cross-fades, palette transitions (0.6 s), title and artist slide-fade together
  (V3.2: title and artist don't transition today).
- **Queue:** a half-height sheet with the current song pinned at the top, Now Playing visible and usable
  behind it (`presentationBackgroundInteraction` up through medium); full height on drag.
- **Swipe artwork:** optional, to lyrics; a visualisation only later, only if it stops off screen, in the
  background and under Reduce Motion.
- **Known defect, found in review of this plan:** the artist line uses white at 60%, but `ArtworkPalette`
  only darkens the top stop until opaque white reaches 4.5:1, so the artist line can fall to about 3:1.
  Fixed in its own small Soundscape PR (clamp against the composited 60% colour or make the line opaque,
  with a palette test for it), independent of the redesign milestones.
- **Controls:** white glyphs without circles for transport; small round controls on clear glass with a dim
  layer when the artwork is bright (HIG); grouped in one glass container.

States: **nothing playing** (Now Playing can't open; the mini-player says Not Playing); **buffering**
(the play button shows a spinner in place, controls stay live); **stream failed** (one line under the
scrubber with Try Again, and the queue moves on after the retry rules in design.md); **radio retrying**
(exists); **no lyrics** (Lyrics button disabled with a reason, not hidden); **no artwork** (the coloured
initial and its hue drive the palette).

### C. Album and artist pages (priority 3)

As design.md S4. Additions: the tile's artwork is the page's anchor from the first frame (the zoom lands on
the header already drawn in its colours); the header fades into the page as it scrolls; Play, Shuffle and
Station as one row of glass-free buttons (content layer).

States: **album with one song**, **artist with no artwork** (circle with initial), **catalogue album
offline** (songs greyed with "Needs a connection", downloaded ones playable).

### D. Discover (priority 4)

As design.md S5 (Top This Week, Popular, Start a Station, New in your genres, Recently Discovered, Browse by
Genre). Additions: varied composition (one wide card, one grid, the rest shelves, not ten identical rows);
Moods only if a genre-to-mood mapping can be done in Swift from Jamendo tags without a new service.

States: **no Jamendo client ID** (the page explains and shows the Internet Archive); **offline** (one
calm state with what is downloaded); **failed** (per shelf).

### E. Library (priority 5)

Category tiles (Playlists, Artists, Albums, Songs, Smart Playlists, Favourites, Downloaded, To Pick,
Recently Played), recently added albums as a grid. Additions: sort and filter on Songs, Albums and Artists
as a small floating overlay (planned in Layered Navigation); queue grouped by album when an album plays.

States: **empty** (import and server actions), **huge library** (10,000 songs scroll without hitches;
see QA budgets), **unavailable file** (marked on its row with Re-import / Remove).

### F. Search (priority 6)

Recent searches and genres before typing (exist). Additions: suggested artists before typing (most
played); results grouped by kind (Top Result, Artists, Albums, Songs) with the source shown on each row
rather than as the grouping; Play on every song row without opening it.

States: **no results** (says where it looked), **offline** (local results only, said once), **slow
catalogue** (local results appear at once; catalogue results stream in below).

### G. Mini-player, queue, overlays (priority 7)

Mini-player: glass card above the tab bar (#437), artwork, title, heart, play. Opening: zoom from its
artwork (exists, iOS 18+). Overlays: translucent standard material (not glass), solid under Reduce
Transparency.

## Signature interactions

| Interaction | Behaviour | Reduce Motion |
|---|---|---|
| Track change | Artwork crossfade, palette 0.6 s, text slide-fade | Instant |
| Open Now Playing | Mini-player expands (zoom from its artwork) | Cover |
| Open album | Artwork becomes the page's anchor | Push |
| Queue | Half-height floating sheet, current song visible | Same, no fade |
| Swipe artwork | Lyrics (later an optional visualisation) | No animated visuals |
| Favourite | Heart fill with symbol bounce and the light tap | No bounce |
| Radio | Unobtrusive station row, quiet top-up states | Same |

Every gesture has a button: Queue, Lyrics, Close, More.

## Milestones (from the programme)

V3.1 Listen and Library · V3.2 Now Playing and album / artist anchor · V3.3 Discover and Search · V3.4
transitions and polish. Each PR carries the evidence in [DESIGN_QA.md](DESIGN_QA.md).
