# Specification: NetNewsWire as the reference for Commonplace

Each claim about NetNewsWire says where it comes from. **Expected** means it is from general knowledge of the app and still has to be confirmed by a walk on the phone (`forge ui explore --app`, [README](README.md#running-the-comparison)); after the walk it becomes **Seen** with the walk's folder and screen id. Nothing here is Seen yet.

## Screens

| # | NetNewsWire screen | Evidence | Commonplace screen | Identifiers in Commonplace |
|---|---|---|---|---|
| S1 | Feeds list: smart feeds (Today, All Unread, Starred), then folders and feeds with unread counts; toolbar with settings, add and refresh | Expected | `FeedListView`: All Unread, Bookmarks, All Articles, then feeds with unread counts; settings and add in the navigation bar, refresh and status in the bottom bar | `feeds.unread`, `feeds.bookmarks`, `feeds.all`, `feeds.feed.<id>`, `feeds.settings`, `feeds.add`, `feeds.refresh`, `feeds.status` |
| S2 | Timeline: articles newest first, unread marker, feed name, date, summary | Expected | `TimelineView` | `timeline.article.<id>`, `timeline.markAllRead` |
| S3 | Article: title, feed, author, date, body with images and links; toolbar with star, read state, share, open in browser | Expected | `ArticleView`: details, then the body on ForgeReader (headings, quotes, lists, pictures, links); text size and font menu; links open in the web sheet or Safari as Settings say | `article.title`, `article.body`, `article.textSettings`, `article.bookmark`, `article.more`, `article.toggleRead`, `article.showOriginal`, `article.openInBrowser`, `article.share` |
| S4 | Add feed: an address field, then a confirmation with the feed's name and folder | Expected | `AddFeedView`: the address only; the feed's own name is used | `addFeed.url`, `addFeed.save`, `addFeed.cancel` |
| S5 | Settings: timeline order, hide read, preview lines, article theme and font, where links open, refresh | Expected | `SettingsView` (sheet): Oldest First, Hide Read Articles, Show Summaries, Open Links in Commonplace, Font, Notify About New Articles | `settings.oldestFirst`, `settings.hideRead`, `settings.showSummaries`, `settings.openLinksInApp`, `settings.font`, `settings.newArticleAlerts`, `settings.done` |
| S6 | Empty states (no feeds, nothing unread) | Expected | `ForgeStateView` on S1 and S2 | `feeds.addFirst` |
| S7 | Search: a search field over every article, results as timeline rows | Expected | The feeds list's search field ("Search Articles"; SwiftUI gives it no identifier); results replace the lists | `search.article.<id>` |

Differences already known and accepted: Commonplace has no Today timeline, says "bookmark" rather than "star", asks for no folder when adding (a feed moves to a folder from its menu), shows a folder as its own screen rather than expanding it in place, and has no article themes beyond the font and text size.

## Flows

Each is a `forge ui run` file for Commonplace (in `examples/commonplace/uitests/`) and a walk or a person's check for NetNewsWire.

| # | Flow | NetNewsWire | Commonplace |
|---|---|---|---|
| F1 | Add the fixture feed by address; its articles appear | Expected | `subscribe-and-read.toml` |
| F2 | Open an article; it is marked read and the unread count drops | Expected | `subscribe-and-read.toml` (the fixture feed's 4 articles go to 3 unread); passed on Cati 2026-11-10 up to that check, which expected the wrong number until it was corrected |
| F3 | Bookmark (star) an article; it appears under Bookmarks (Starred) | Expected | `subscribe-and-read.toml` |
| F4 | Mark all as read in a feed | Expected | `mark-all-read.toml`, passed on Cati 2026-11-10 |
| F5 | Refresh with nothing new: no new articles, no duplicates | Expected | Rust test `an_unchanged_or_failing_feed_is_reported_not_fatal` |
| F6 | Unsubscribe; the feed and its articles are gone | Expected | Rust test `feeds_can_be_renamed_and_removed_and_everything_persists` |
| F7 | Relaunch; feeds, read state and bookmarks are kept | Expected | Rust test; `subscribe-and-read.toml` relaunches and checks the bookmark |
| F8 | Search for a word in an article's text; the article is found and opens | Expected | Rust test `search_finds_titles_and_text_by_every_word`; a person's check ([walk.md](walk.md)) |
| F9 | Change the timeline order and hide read articles; the timelines follow and the choice is kept | Expected | Rust test `settings_are_kept_and_shape_the_timelines`; a person's check |
| F10 | Tap a link in an article; it opens in the app's browser (or Safari) | Expected | A person's check |
| F11 | Larger text in the article; the text grows, the place is kept | Expected | A person's check |
| F12 | Make a folder, move a feed into it; the folder's timeline shows its articles, and deleting the folder keeps the feed | Expected | Rust tests `folders_are_made_renamed_and_deleted_without_losing_feeds`, `a_folder_timeline_covers_the_folders_inside_it`; a person's check |
| F13 | Import an OPML file: a preview first, folders kept, nothing duplicated on a second import | Expected | Rust tests `the_preview_says_what_importing_would_do_and_changes_nothing`, `importing_twice_adds_nothing_and_changes_nothing_the_library_has`; a person's check with `fixtures/feeder-folders.opml` |
| F14 | Export OPML and import it elsewhere with the same folders | Expected | Rust test `an_export_imports_again_with_its_folders_settings_and_kept_attributes`; a person's check |

## States and edge cases

| # | Situation | Fixture | NetNewsWire | Commonplace |
|---|---|---|---|---|
| E1 | Address that isn't a feed | `not-a-feed.html` | Expected: an error, or a search of the page for a feed link | "There's no RSS, Atom or JSON feed at that address." (it doesn't look for links in the page: gap `feed-discovery`) |
| E2 | Feed that is broken XML | `broken.xml` | Expected: an error | Same as E1 |
| E3 | 404 and 500 | `/missing.xml`, `/error.xml` | Expected: an error on add; on refresh the feed keeps its articles | Error with the HTTP status; on refresh the feed shows "Couldn't refresh" and keeps its articles |
| E4 | Slow server | `/slow.xml` | Expected: the add waits with a spinner | Spinner; leaving the app stops it (Interrupted) |
| E5 | Offline | Laptop server stopped | Expected: an error | "You appear to be offline." |
| E6 | Feed with no articles | `empty.xml` | Expected: an empty timeline | "No Articles" |
| E7 | Article with no title | `rss.xml` item 4 | Expected: shown by its text | Titled with its first words |
| E8 | Second refresh with nothing changed | any | Expected: no duplicates | HTTP 304 via If-Modified-Since / If-None-Match |
