# Reference deep audit 3: NetNewsWire, for Commonplace and the reading view

Status: 2026-11-17. Phase 1b of [ixOS Experience Research](../README.md). Research only: no change to Commonplace or ForgeReader.

**Reference:** [NetNewsWire](https://github.com/Ranchero-Software/NetNewsWire) for iOS at `22ce4cc` (2026-11-16), MIT, UIKit with some SwiftUI. Paths below are relative to its repository; its words are from its `NSLocalizedString` calls.
**ixOS side:** Commonplace on main, and [ForgeReader](../../../forge-reader.md), the native reading view Commonplace shares with Pocket Library.
**Not a repeat of the earlier benchmark.** Commonplace's [benchmark](../../../../examples/commonplace/docs/benchmark/README.md) (#148) compares features and records platform gaps; its phone walk hasn't been done yet. This audit compares the experience: how reading feels, how fast the common loop is, and what happens after a mistake. The two meet when the walk is done.

**Evidence:** **S** (source verified) on both sides. NetNewsWire was read, not run; no screenshots of it are reproduced.

## The reading loop, side by side

The task people repeat most in a feed reader is: open the app, read what's new, move on to the next one, keep one or two, and clear the rest.

| Step | NetNewsWire | Commonplace |
|---|---|---|
| First run | 10 default feeds imported (`Shared/Importers/DefaultFeeds.opml`), so the first screen has articles | Empty: "No Feeds Yet", Add Feed |
| Add a feed | URL field takes a site's address; `Modules/FeedFinder` finds the feed in the page; optional title and folder | Needs the feed's own URL (`AddFeedView`, prompt `example.com/feed.xml`); a site address fails (friction audit F11) |
| Go to the next article | In the article: **Next Unread** (the toolbar's highest-priority item), Previous and Next Article buttons, and a horizontal swipe between articles (`iOS/Article/ArticleViewController.swift`, `UIPageViewController`); keyboard shortcuts | Back to the timeline, tap the next row (`ArticleView` toolbar has text settings, bookmark, read state, share; nothing to move between articles) |
| Clear what you've read | Swipe read and star; the row's context menu adds **Mark Above as Read** and **Mark Below as Read**; Mark All as Read asks once, says "You can turn this confirmation off in Settings", and can be undone (`Shared/Commands/MarkStatusCommand.swift` registers undo) | Swipe read and bookmark; Mark All as Read with a confirmation (#274); no context menu on article rows; no undo |
| Hide what's read | A filter toggle on each timeline and on the feed list ("Filter Read Articles", "Filter Read Feeds") | One global setting ("Hide Read Articles" in Settings) |
| Know it refreshed | A thin progress bar while refreshing, then "Updated Just Now" / "Updated …" in the toolbar (`iOS/MainFeed/RefreshProgressView.swift`) | Status text in the bottom bar: "Refreshing…", counts, "Updated 10:41", or a long offline sentence that may truncate (V4) |
| Recognise a source | Feed icons on rows (`iOS/MainTimeline/Cell/MainTimelineCell.swift`, `IconView`) | Text only (audit CP4) |

## Lessons, and where each belongs

### 1. Next unread is the reader's main button

**NetNewsWire:** Next Unread is the article toolbar's highest-priority item, and articles page sideways like a book.
**Commonplace:** no way to move between articles without going back (audit CP1, P1).
**Why it's better:** reading a timeline becomes read, next, read, next, with no trips back to the list. It is the single largest speed difference between the two apps.
**Where:** ForgeReader for the controls (Pocket Library's reader already has previous and next chapter, so this is its second use), Commonplace for "what is next" (its timeline order and unread state come from Rust).
**Proposal (P1):** ForgeReader takes optional previous, next and "next unread" actions and shows them in its toolbar; Commonplace supplies them from the timeline the article was opened from. Sideways paging is a later step.

### 2. Start with something to read

**NetNewsWire:** a handful of default feeds on first run, removable like any other.
**Commonplace:** empty until the person finds a feed URL.
**Why it's better:** the app explains itself by working; the person learns by removing what they don't want.
**Where:** App.
**Proposal (P2):** offer a short list of suggested feeds on the empty screen ("Add a few to start"), chosen, not imported silently, so nothing personal is assumed. Pair it with lesson 3.

### 3. Paste a site, get its feed

**NetNewsWire:** the URL field accepts a site address and finds the feed (`Modules/FeedFinder`).
**Commonplace:** fails with "There's no RSS, Atom or JSON feed at that address" (F11).
**Where:** Rust (discovery from the page's `<link rel="alternate">`), App (pick one when a site has several).
**Proposal (P1):** feed discovery; it is already listed in the audit (CP2) and the feed-reader expansion.

### 4. Clearing has more than one size

**NetNewsWire:** one article (swipe), everything above or below this one (context menu), everything in this timeline (toolbar, confirmed and undoable).
**Commonplace:** one article or everything.
**Why it's better:** "I've read down to here" is the most common way people actually triage a timeline.
**Where:** App + Rust (mark a range).
**Proposal (P2):** a context menu on article rows with Mark as Read or Unread, Bookmark, Mark Above as Read, Mark Below as Read, Share; it also fixes the swipe-only finding (X11) for these rows.

### 5. Confirm once, then let the person decide; undo either way

**NetNewsWire:** Mark All as Read asks, tells you the question can be turned off, and the action can be undone.
**Commonplace:** always confirms (#274), cannot undo.
**Why it's better:** the confirmation protects a newcomer; the undo protects everyone; the setting respects the expert. It is the audit's "undo frequent, confirm rare" guideline with a sensible bridge.
**Where:** ForgeAppKit for the undo (the banner value proposed by the [Ice Cubes audit](ice-cubes.md#3-a-small-banner-for-work-that-continues-and-for-undo)); App for the wording.
**Proposal (P2):** once undo exists, Mark All as Read uses it and drops the confirmation, or keeps it only above a large count. No new setting needed if undo is there.

### 6. Filters where you use them

**NetNewsWire:** hiding read articles is a toggle on the timeline itself.
**Commonplace:** a global setting two screens away.
**Where:** App.
**Proposal (P3):** a filter button in the timeline toolbar; Settings keeps the default.

### 7. Refresh status that fits

**NetNewsWire:** a progress bar while working, a short "Updated …" after.
**Commonplace:** sentences in a narrow bottom bar (V4).
**Where:** App; the offline wording joins the audit's inline notice (X17).
**Proposal (P3):** short status ("Updated 10:41", "Offline"), with any longer explanation as a notice above the list or on tap.

## For ForgeReader (shared with Pocket Library)

- **Navigation between items** (lesson 1) is the one change that serves both apps: chapters in Pocket Library, articles in Commonplace. It is the extraction rule working as intended: two real uses.
- **Reader View** (NetNewsWire extracts the full article from the web page when a feed sends only a summary) belongs to Commonplace's planned article extraction, not ForgeReader: ForgeReader displays, it doesn't fetch.
- **Article themes:** NetNewsWire ships several article styles (`Themes/`). ForgeReader's text size and serif or system font are enough; themes are a customisation we don't need.

## What Commonplace already does as well or better

- **Search over every article** (SQLite FTS5) with results grouped and counted.
- **Native text layout** on ForgeReader: Dynamic Type, dark mode and VoiceOver from SwiftUI text, where NetNewsWire renders articles in a web view.
- **Everything local, nothing to sign in to**, which matches NetNewsWire's own default (On My iPhone).

## What NetNewsWire gets wrong, so we don't import it

- **Default feeds chosen by the developers** reflect their circle; for Commonplace, suggestions should be offered, never imported silently (lesson 2).
- **Settings for confirmations** ("you can turn this off") add a setting to excuse an interruption; undo makes most of them unnecessary.
- **UIKit and storyboards**, web-view articles and themes: the experience is the lesson, not the implementation.

## Where each proposal belongs

| # | Proposal | Priority | Where | Depends on |
|---|---|---|---|---|
| 1 | Next, previous and next unread in the reader | P1 | ForgeReader (controls), Commonplace (order) | Second use: Pocket Library chapters |
| 2 | Suggested feeds on the empty screen | P2 | Commonplace | Lesson 3 |
| 3 | Feed discovery from a site address | P1 | Commonplace Rust + app | |
| 4 | Article context menu with Mark Above/Below as Read | P2 | Commonplace + Rust | |
| 5 | Undo for Mark All as Read; confirm only when large | P2 | ForgeAppKit (undo), Commonplace | Undo banner (Ice Cubes lesson 3) |
| 6 | Read filter on the timeline | P3 | Commonplace | |
| 7 | Short refresh status; longer text as a notice | P3 | Commonplace | Inline notice (X17) |

## Suggested next step

Phase 1b is complete with this audit. The phase 2 design-direction doc can now draw on all three references. Lesson 1 is the strongest candidate for the first app change once phase 3 starts, because it improves two apps through one shared piece.
