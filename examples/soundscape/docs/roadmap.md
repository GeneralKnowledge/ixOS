# Soundscape roadmap: a more complete player

Reuben's direction (2026-11-22): narrow Soundscape's next phase to making the music library you already have
more useful, easier to keep tidy, portable and properly part of iOS. The player should feel like a complete
product, not a bigger one.

**Out of scope for now:** M14 to M17. That means no EQ expansion, no sync between devices, no recommendation
engine and no overhaul of how sources are unified. The Apple Music-style redesign (S1 to S6, [design.md](design.md)) runs alongside in its own thread; these milestones build on its screens rather than adding their own.

Podcasts and live radio are proposed separately in [podcasts-and-radio.md](podcasts-and-radio.md) (not yet approved).

## Principles, for all five milestones

1. **Local-first.** Smart playlists, statistics, maintenance and backups work without a cloud account or a paid
   service.
2. **Non-destructive by default.** Corrections, duplicate handling and imports are shown before they are applied.
   Nothing silently deletes or rewrites your collection. Undo where practical.
3. **Portable data.** Documented formats and stable identifiers. Soundscape is never the only place your
   playlists and listening history can live.
4. **Private insights.** Listening history stays on the phone unless you choose to export or share it.
5. **Native iOS where it matters.** M18 uses Apple's real integration points, not lookalikes inside the app.

## Order

The numbers are names, not the order. M10 and M12 share groundwork (queries over the library's details and play
counts).

| Order | Milestone | Why here |
|---|---|---|
| 1 | [M10 Smart Playlists](#m10-smart-playlists) | Sets up a stronger way to organise the library |
| 2 | [M12 Library Maintenance](#m12-library-maintenance) | Makes the collection underneath cleaner and consistent |
| 3 | [M13 Collection Portability](#m13-collection-portability) | Makes the collection portable before more features depend on it |
| 4 | [M11 Listening Insights](#m11-listening-insights) | Builds on listening history that is by then reliable |
| 5 | [M18 Deep iOS Integration](#m18-deep-ios-integration) | Brings the finished player into the rest of the iPhone |

Each milestone is one or more PRs that can be reviewed on their own, each with Rust tests on Linux and a short
phone check.

## What there is to extend

Audited on main after #416 (one song, several sources) and against #418 (redesign and Radio), so each milestone
extends the existing model instead of starting a parallel one.

| Part | Today | What the roadmap needs |
|---|---|---|
| Collection (`collection.rs`, `collection.json`) | Favourites (id with the time added), manual playlists (ids from `next_playlist`), history of plays (track, recording, time) capped at the newest 2,000. #418 adds likes and dislikes (`feelings`) for Radio | M10: smart playlists stored next to manual ones, same id counter. A play tally per song (count, first and last play) that isn't capped, since the history drops old plays |
| Date added | Imported files and catalogue songs keep `added_ms`; take server songs only have the time of their newest take | M10: one `added_ms` on every library song (a server song's oldest take) |
| Merging (`merge.rs`) | Moves favourites, playlist entries, history and offline choices to the kept song | M10: a merged song's play counts move with it. M12 builds on the duplicate finder |
| Details (`library.rs` `edits`) | Edit one song's title, artist, album, number, genre and artwork | M12: the same edits applied to many songs, previewed first |
| MusicBrainz (N3) | Identify Song links one song | M12: suggestions across the library, reviewed before applying |
| Saved files | `library.json`, `collection.json`, `offline.json`, `playback.json`, each with a schema version | M13: one documented, versioned backup format made from them |
| Now Playing | `ForgeNowPlaying`: lock screen, Control Centre and remote commands | M18: the rest (widgets, Live Activity, intents, Share Sheet, CarPlay) |

## M10 Smart Playlists

**First.** Make the library organise itself.

- Playlists made from rules, with several conditions, matching all or any of them.
- Rules on artist, album, genre, title, play count, date added, last played, favourite and length.
- Built in: **Recently Added**, **Never Played**, **Most Played** and **Forgotten Favourites** (favourites not
  played for a while).
- Live: a smart playlist is worked out from the library each time it is shown, so it follows edits, merges,
  imports and plays. It stores its rules, never its songs.
- Manual playlists stay exactly as they are.
- Sort order (most played, recently added, recently played, title, artist) and an optional song limit.

**Deliverable:** a smart playlist editor and the four built-in smart playlists.

### M10, as built

- **Rules** (`rust/src/smart.rs`): text rules on title, artist, album and genre (is, is not, contains, does
  not contain, begins with; capitals and extra spaces don't count, and "genre is" nothing finds songs without
  one); number rules on plays and length (exactly, at least, at most); date rules on date added and last played
  (in the last, or not in the last, so many days; a song never played was not played lately); and favourite or
  not. All or any of up to 20 rules, a sort (title, artist, album, recently added, most played, recently played,
  least recently played) and an optional limit.
- **Kept:** `collection.json` gains `smart_playlists` (name and rules only; ids from the same `next_playlist` as
  manual ones) and `plays` (per song: how many plays, first and last). Plays are counted from the history once
  for a collection saved before. Clearing the history clears the counts too. A merge adds the merged song's
  plays to the kept song's.
- **Date added:** every library song has `added_ms`: an imported file's or catalogue song's time of adding, a
  take server song's oldest take, the earliest of a merged song's sources.
- **Built in:** Recently Added (the newest 50), Most Played (the top 25), Never Played, and Forgotten Favourites
  (favourite songs not played for 60 days). They can't be edited; Make Your Own from These Rules copies them.
- **Screens** (`SmartPlaylists.swift`): a Smart Playlists section at the top of Playlists, New Smart Playlist
  under +, a smart playlist's screen with its rules in words and its songs (play counts beside Most Played), and
  an editor that counts matching songs as you change it. Make Playlist of These Songs keeps a fixed copy.

## M12 Library Maintenance

Help keep the collection clean and consistent.

- Duplicate songs (the finder from #416) and duplicate albums.
- Songs and albums missing artwork or details.
- Artist and album names written inconsistently ("The Beatles" and "Beatles", different capitals or spacing).
- MusicBrainz identification suggestions for songs not yet linked.
- Correcting details in bulk.
- Review before apply, with undo where practical.

**Deliverable:** a Library Health screen listing problems you can act on, with safe tools to fix them.

## M13 Collection Portability

Your organisation of your music is yours and can move.

- M3U and M3U8 playlist import and export.
- Export favourites and listening history.
- Back up and restore playlists, details and preferences.
- Keep track identifiers and source information.
- Imports and restores cope with files that are missing.

The backup format is versioned and documented here when it is built. That gives later features a solid base
without committing to sync between devices.

**Deliverable:** a complete export and import, and backup and restore.

## M11 Listening Insights

A useful picture of your listening, not a leaderboard.

- Listening time by day, week, month and year. This needs time listened, which plays don't record yet: M11 adds
  a small per-day total.
- Most played artists, albums and songs (from M10's tally).
- Music you have rediscovered.
- Trends, and genres where the details allow.
- A personal yearly recap.

**Deliverable:** an Insights section.

## M18 Deep iOS Integration

Make Soundscape feel part of the iPhone.

- Home Screen and Lock Screen widgets.
- A Live Activity and Dynamic Island playback controls.
- Siri, Shortcuts and App Intents.
- Share Sheet.
- CarPlay.
- Native system media controls and Now Playing details (mostly there through `ForgeNowPlaying`).

**To check before starting:** widgets and Live Activities are app extensions, so they need xtool to build and sign
an extension with the free team (each extension is another App ID, and the team gets 10 new ones a week). CarPlay
audio apps need Apple's CarPlay entitlement, which Apple grants on request; that probably waits for a paid team.

**Deliverable:** a properly integrated iOS music experience.
