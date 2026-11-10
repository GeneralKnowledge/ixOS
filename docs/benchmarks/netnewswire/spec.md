# Specification: NetNewsWire as the reference for Commonplace

Each claim about NetNewsWire says where it comes from. **Expected** means it is from general knowledge of the app and still has to be confirmed by a walk on the phone (`forge ui explore --app`, [README](README.md#running-the-comparison)); after the walk it becomes **Seen** with the walk's folder and screen id. Nothing here is Seen yet.

## Screens

| # | NetNewsWire screen | Evidence | Commonplace screen | Identifiers in Commonplace |
|---|---|---|---|---|
| S1 | Feeds list: smart feeds (Today, All Unread, Starred), then folders and feeds with unread counts; toolbar with settings, add and refresh | Expected | `FeedListView`: All Unread, Bookmarks, All Articles, then feeds with unread counts; add in the navigation bar, refresh and status in the bottom bar | `feeds.unread`, `feeds.bookmarks`, `feeds.all`, `feeds.feed.<id>`, `feeds.add`, `feeds.refresh`, `feeds.status` |
| S2 | Timeline: articles newest first, unread marker, feed name, date, summary | Expected | `TimelineView` | `timeline.article.<id>`, `timeline.markAllRead` |
| S3 | Article: title, feed, author, date, body; toolbar with star, read state, share, open in browser | Expected | `ArticleView` with `ArticleReader` (plain paragraphs in slice 1) | `article.title`, `article.body`, `article.bookmark`, `article.more`, `article.toggleRead`, `article.openInBrowser`, `article.share` |
| S4 | Add feed: an address field, then a confirmation with the feed's name and folder | Expected | `AddFeedView`: the address only; the feed's own name is used | `addFeed.url`, `addFeed.save`, `addFeed.cancel` |
| S5 | Settings | Expected | None in slice 1 | |
| S6 | Empty states (no feeds, nothing unread) | Expected | `ForgeStateView` on S1 and S2 | `feeds.addFirst` |

Differences already known and accepted: Commonplace has no Today timeline in slice 1, says "bookmark" rather than "star", and asks for no folder when adding.

## Flows

Each is a `forge ui run` file for Commonplace (in `examples/commonplace/uitests/`) and a walk or a person's check for NetNewsWire.

| # | Flow | NetNewsWire | Commonplace |
|---|---|---|---|
| F1 | Add the fixture feed by address; its articles appear | Expected | `subscribe-and-read.toml` |
| F2 | Open an article; it is marked read and the unread count drops | Expected | `subscribe-and-read.toml` (the feed's count goes from 3 to 2 unread) |
| F3 | Bookmark (star) an article; it appears under Bookmarks (Starred) | Expected | `subscribe-and-read.toml` |
| F4 | Mark all as read in a feed | Expected | `mark-all-read.toml` |
| F5 | Refresh with nothing new: no new articles, no duplicates | Expected | Rust test `an_unchanged_or_failing_feed_is_reported_not_fatal` |
| F6 | Unsubscribe; the feed and its articles are gone | Expected | Rust test `feeds_can_be_renamed_and_removed_and_everything_persists` |
| F7 | Relaunch; feeds, read state and bookmarks are kept | Expected | Rust test; `subscribe-and-read.toml` relaunches and checks the bookmark |

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
