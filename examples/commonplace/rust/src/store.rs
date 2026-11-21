//! Commonplace's SQLite file: subscriptions and their articles. Only the
//! command thread touches it; every change is one transaction.

use std::path::Path;

use forge_storage::rusqlite::{params, OptionalExtension, Transaction};
use forge_storage::{Database, Opened, Reader};

use crate::feed::{self, ParsedFeed};
use crate::model::{AppError, AppSettings, ArticleSummary, ArticleText, Feed, Folder};
use crate::opml;

/// Schema steps: step `i` brings the file from version `i` to `i + 1`.
pub(crate) const MIGRATIONS: &[&str] = &[
    "
    CREATE TABLE feeds (
        id INTEGER PRIMARY KEY,
        url TEXT NOT NULL UNIQUE,
        title TEXT NOT NULL,
        custom_title TEXT,
        site_url TEXT,
        etag TEXT,
        last_modified TEXT,
        refreshed_at_ms INTEGER,
        last_error TEXT,
        added_at_ms INTEGER NOT NULL
    );
    CREATE TABLE articles (
        id INTEGER PRIMARY KEY,
        feed_id INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
        guid TEXT NOT NULL,
        title TEXT NOT NULL,
        url TEXT,
        author TEXT,
        html TEXT NOT NULL,
        summary TEXT NOT NULL,
        published_at_ms INTEGER NOT NULL,
        fetched_at_ms INTEGER NOT NULL,
        read INTEGER NOT NULL DEFAULT 0,
        bookmarked INTEGER NOT NULL DEFAULT 0,
        UNIQUE (feed_id, guid)
    );
    CREATE INDEX articles_by_date ON articles(published_at_ms DESC, id DESC);
",
    "
    -- Search: each article's title and text, by article id. Filled by Rust
    -- (the text is the article's HTML as plain text); an article's row goes
    -- with it.
    CREATE VIRTUAL TABLE article_search USING fts5(
        title, body, tokenize = 'unicode61 remove_diacritics 2'
    );
    CREATE TRIGGER article_search_forget AFTER DELETE ON articles BEGIN
        DELETE FROM article_search WHERE rowid = old.id;
    END;
    -- The person's settings (AppSettings), one row per setting.
    CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
",
    "
    -- Folders (C3): a folder can sit in a folder. Deleting one moves what
    -- it holds up a level (Rust does that first; the cascade is a backstop).
    CREATE TABLE folders (
        id INTEGER PRIMARY KEY,
        parent_id INTEGER REFERENCES folders(id) ON DELETE CASCADE,
        name TEXT NOT NULL,
        added_at_ms INTEGER NOT NULL
    );
    ALTER TABLE feeds ADD COLUMN folder_id INTEGER REFERENCES folders(id) ON DELETE SET NULL;
    -- Per-feed options: counts for the new-articles notification; fetch
    -- whole articles (used once full-article fetching exists).
    ALTER TABLE feeds ADD COLUMN notify INTEGER NOT NULL DEFAULT 1;
    ALTER TABLE feeds ADD COLUMN full_text INTEGER NOT NULL DEFAULT 0;
    -- Attributes an OPML import found and Commonplace doesn't use, as JSON
    -- ([{namespace, name, value}]), written back by an export.
    ALTER TABLE feeds ADD COLUMN opml_extras TEXT;
    -- Filter patterns (plain text, never run), from an import for now.
    CREATE TABLE filters (
        id INTEGER PRIMARY KEY,
        pattern TEXT NOT NULL UNIQUE,
        added_at_ms INTEGER NOT NULL
    );
",
    "
    -- Full articles (C4): the readable part of the article's page, and what
    -- fetching it did: NULL (not tried), 'ok', or the kind of failure.
    ALTER TABLE articles ADD COLUMN full_html TEXT;
    ALTER TABLE articles ADD COLUMN full_status TEXT;
",
];

/// The most results a search returns.
const MAX_RESULTS: usize = 500;

/// How long a row's summary is, in characters.
const SUMMARY_CHARS: usize = 200;
/// Read articles that aren't bookmarked are forgotten this long after they
/// were fetched, once the feed no longer lists them (a feed still listing
/// one would bring it back as new). Keeps the file, and every snapshot,
/// from growing for ever.
const KEEP_READ_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// What a fetch learnt about a feed, applied in one transaction.
pub struct Fetched {
    pub feed_id: i64,
    pub result: Result<FetchResult, AppError>,
}

pub enum FetchResult {
    /// HTTP 304: nothing new.
    NotModified,
    Parsed {
        feed: ParsedFeed,
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

/// What the app holds, read in one go for the published snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub feeds: Vec<Feed>,
    /// Every folder, as a tree: each followed by the folders inside it, by
    /// name.
    pub folders: Vec<Folder>,
    /// Every article, newest first.
    pub articles: Vec<ArticleSummary>,
    pub settings: AppSettings,
    /// Where each feed is fetched from, with its cache validators.
    pub sources: Vec<FeedSource>,
}

/// A feed's address and its cache validators, for a conditional fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedSource {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

pub struct Store {
    db: Database,
}

impl Store {
    /// Opens `library.sqlite` in `directory`. A damaged file is moved aside
    /// and a fresh one made (logged; the feeds can be re-added).
    pub fn open(directory: &Path) -> Result<Self, AppError> {
        let (db, opened) = Database::open(directory.join("library.sqlite"), MIGRATIONS)?;
        if let Opened::Recovered { reason, .. } = &opened {
            forge_runtime::logging::warn!("library was damaged and set aside: {reason}");
        }
        db.connection()
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(AppError::from)?;
        let mut store = Self { db };
        store.index_if_behind()?;
        Ok(store)
    }

    /// Fills the search index when it doesn't hold every article: once
    /// after the update that added search, and after anything that left it
    /// short.
    fn index_if_behind(&mut self) -> Result<(), AppError> {
        let c = self.db.connection();
        let articles: i64 = c.query_row("SELECT COUNT(*) FROM articles", [], |r| r.get(0))?;
        let indexed: i64 = c.query_row("SELECT COUNT(*) FROM article_search", [], |r| r.get(0))?;
        if articles == indexed {
            return Ok(());
        }
        self.db.transaction(|tx| {
            tx.execute("DELETE FROM article_search", [])?;
            let rows: Vec<(i64, String, String)> = {
                let mut q = tx.prepare("SELECT id, title, html FROM articles")?;
                let rows = q
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            for (id, title, html) in &rows {
                index_article(tx, *id, title, html)?;
            }
            forge_runtime::logging::info!("search index rebuilt for {} articles", rows.len());
            Ok::<_, AppError>(())
        })
    }

    /// The person's settings (defaults for any never set).
    pub fn settings(&self) -> Result<AppSettings, AppError> {
        let mut settings = AppSettings::default();
        let mut q = self
            .db
            .connection()
            .prepare("SELECT key, value FROM settings")?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (key, value) in rows {
            let on = value == "1";
            match key.as_str() {
                "oldest_first" => settings.oldest_first = on,
                "hide_read" => settings.hide_read = on,
                "show_summaries" => settings.show_summaries = on,
                "open_links_in_app" => settings.open_links_in_app = on,
                // A setting from a newer version: kept, not understood.
                _ => {}
            }
        }
        Ok(settings)
    }

    pub fn set_settings(&mut self, settings: &AppSettings) -> Result<(), AppError> {
        let values = [
            ("oldest_first", settings.oldest_first),
            ("hide_read", settings.hide_read),
            ("show_summaries", settings.show_summaries),
            ("open_links_in_app", settings.open_links_in_app),
        ];
        self.db.transaction(|tx| {
            for (key, on) in values {
                tx.execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2)
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                    params![key, if on { "1" } else { "0" }],
                )?;
            }
            Ok(())
        })
    }

    pub fn snapshot(&self) -> Result<Snapshot, AppError> {
        let c = self.db.connection();
        let mut feeds = c.prepare(
            "SELECT f.id, f.url, COALESCE(f.custom_title, f.title), f.site_url,
                    f.refreshed_at_ms, f.last_error,
                    COUNT(a.id), COALESCE(SUM(a.read = 0), 0),
                    f.folder_id, f.notify, f.full_text
             FROM feeds f LEFT JOIN articles a ON a.feed_id = f.id
             GROUP BY f.id
             ORDER BY COALESCE(f.custom_title, f.title) COLLATE NOCASE, f.id",
        )?;
        let feeds = feeds
            .query_map([], |r| {
                Ok(Feed {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    title: r.get(2)?,
                    site_url: r.get(3)?,
                    refreshed_at_ms: r.get(4)?,
                    last_error: r.get(5)?,
                    article_count: r.get(6)?,
                    unread_count: r.get(7)?,
                    folder_id: r.get(8)?,
                    notify: r.get(9)?,
                    full_text: r.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut articles = c.prepare(
            "SELECT a.id, a.feed_id, COALESCE(f.custom_title, f.title), a.title, a.summary,
                    a.url, a.author, a.published_at_ms, a.read, a.bookmarked
             FROM articles a JOIN feeds f ON f.id = a.feed_id
             ORDER BY a.published_at_ms DESC, a.id DESC",
        )?;
        let articles = articles
            .query_map([], |r| {
                Ok(ArticleSummary {
                    id: r.get(0)?,
                    feed_id: r.get(1)?,
                    feed_title: r.get(2)?,
                    title: r.get(3)?,
                    summary: r.get(4)?,
                    url: r.get(5)?,
                    author: r.get(6)?,
                    published_at_ms: r.get(7)?,
                    read: r.get(8)?,
                    bookmarked: r.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let folders = self.folders(&feeds)?;
        Ok(Snapshot {
            feeds,
            folders,
            articles,
            sources: self.sources()?,
            settings: self.settings()?,
        })
    }

    /// Every folder in tree order, with its feed and unread counts
    /// (including the folders inside it).
    fn folders(&self, feeds: &[Feed]) -> Result<Vec<Folder>, AppError> {
        let mut q = self
            .db
            .connection()
            .prepare("SELECT id, parent_id, name FROM folders ORDER BY name COLLATE NOCASE, id")?;
        let rows: Vec<(i64, Option<i64>, String)> = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::with_capacity(rows.len());
        fn walk(
            parent: Option<i64>,
            depth: u32,
            rows: &[(i64, Option<i64>, String)],
            feeds: &[Feed],
            out: &mut Vec<Folder>,
        ) {
            for (id, parent_id, name) in rows.iter().filter(|r| r.1 == parent) {
                let at = out.len();
                out.push(Folder {
                    id: *id,
                    parent_id: *parent_id,
                    name: name.clone(),
                    depth,
                    feed_count: 0,
                    unread_count: 0,
                });
                // A loop in the file (never written by Commonplace) stops here.
                if depth < 64 {
                    walk(Some(*id), depth + 1, rows, feeds, out);
                }
                let inside = folder_subtree(*id, out);
                let (count, unread) = feeds
                    .iter()
                    .filter(|f| f.folder_id.is_some_and(|f| inside.contains(&f)))
                    .fold((0, 0), |(c, u), f| (c + 1, u + f.unread_count));
                out[at].feed_count = count;
                out[at].unread_count = unread;
            }
        }
        walk(None, 0, &rows, feeds, &mut out);
        Ok(out)
    }

    pub fn sources(&self) -> Result<Vec<FeedSource>, AppError> {
        let mut q = self.db.connection().prepare(
            "SELECT id, url, COALESCE(custom_title, title), etag, last_modified
             FROM feeds ORDER BY id",
        )?;
        let rows = q
            .query_map([], |r| {
                Ok(FeedSource {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    title: r.get(2)?,
                    etag: r.get(3)?,
                    last_modified: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Subscribes to a feed that was just fetched, with its articles.
    pub fn add_feed(
        &mut self,
        url: &str,
        parsed: &ParsedFeed,
        etag: Option<String>,
        last_modified: Option<String>,
        now_ms: i64,
    ) -> Result<(i64, u32), AppError> {
        self.db.transaction(|tx| {
            if let Some(id) = tx
                .query_row("SELECT id FROM feeds WHERE url = ?1", [url], |r| r.get(0))
                .optional()?
            {
                return Err(AppError::AlreadySubscribed { feed_id: id });
            }
            tx.execute(
                "INSERT INTO feeds (url, title, site_url, etag, last_modified, refreshed_at_ms, added_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    url,
                    parsed.title.clone().unwrap_or_else(|| host_of(url)),
                    parsed.site_url,
                    etag,
                    last_modified,
                    now_ms
                ],
            )?;
            let id = tx.last_insert_rowid();
            let added = merge_articles(tx, id, parsed, now_ms)?;
            Ok((id, added))
        })
    }

    /// Applies a refresh: new articles in, changed ones updated (read and
    /// bookmark state kept), validators and errors recorded. Returns how
    /// many articles were new, and how many of those are from feeds that
    /// notify.
    pub fn apply(&mut self, fetched: &[Fetched], now_ms: i64) -> Result<(u32, u32), AppError> {
        self.db.transaction(|tx| {
            let mut added = 0;
            let mut notify = 0;
            for f in fetched {
                match &f.result {
                    // Unsubscribed while it was being fetched: nothing to do.
                    _ if !feed_exists(tx, f.feed_id)? => {}
                    Ok(FetchResult::NotModified) => {
                        tx.execute(
                            "UPDATE feeds SET refreshed_at_ms = ?2, last_error = NULL WHERE id = ?1",
                            params![f.feed_id, now_ms],
                        )?;
                    }
                    Ok(FetchResult::Parsed {
                        feed,
                        etag,
                        last_modified,
                    }) => {
                        tx.execute(
                            "UPDATE feeds SET title = COALESCE(?2, title), site_url = COALESCE(?3, site_url),
                                    etag = ?4, last_modified = ?5, refreshed_at_ms = ?6, last_error = NULL
                             WHERE id = ?1",
                            params![f.feed_id, feed.title, feed.site_url, etag, last_modified, now_ms],
                        )?;
                        let new = merge_articles(tx, f.feed_id, feed, now_ms)?;
                        added += new;
                        let notifies: bool = tx.query_row(
                            "SELECT notify FROM feeds WHERE id = ?1",
                            [f.feed_id],
                            |r| r.get(0),
                        )?;
                        if notifies {
                            notify += new;
                        }
                        forget_old_articles(tx, f.feed_id, feed, now_ms)?;
                    }
                    Err(e) => {
                        // The kind, not the message: the screen words it,
                        // and a reason can hold the address.
                        tx.execute(
                            "UPDATE feeds SET last_error = ?2 WHERE id = ?1",
                            params![f.feed_id, e.problem()],
                        )?;
                    }
                }
            }
            Ok((added, notify))
        })
    }

    pub fn remove_feed(&mut self, id: i64) -> Result<(), AppError> {
        self.db.transaction(|tx| {
            if tx.execute("DELETE FROM feeds WHERE id = ?1", [id])? == 0 {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
    }

    /// Sets the person's name for a feed; blank goes back to the feed's own.
    pub fn rename_feed(&mut self, id: i64, title: &str) -> Result<(), AppError> {
        let title = title.trim();
        let custom = (!title.is_empty()).then_some(title);
        self.db.transaction(|tx| {
            if tx.execute(
                "UPDATE feeds SET custom_title = ?2 WHERE id = ?1",
                params![id, custom],
            )? == 0
            {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
    }

    pub fn set_flag(&mut self, article: i64, column: Flag, on: bool) -> Result<(), AppError> {
        let sql = match column {
            Flag::Read => "UPDATE articles SET read = ?2 WHERE id = ?1",
            Flag::Bookmarked => "UPDATE articles SET bookmarked = ?2 WHERE id = ?1",
        };
        self.db.transaction(|tx| {
            if tx.execute(sql, params![article, on])? == 0 {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
    }

    /// Marks every unread article of a feed (or of all feeds) read, and
    /// returns the ones that changed, so the change can be undone.
    pub fn mark_all_read(&mut self, feed: Option<i64>) -> Result<Vec<i64>, AppError> {
        self.mark_feeds_read(feed.map(|f| vec![f]))
    }

    /// Marks every unread article of these feeds (of all, with None) read,
    /// and returns the ones that changed.
    pub fn mark_feeds_read(&mut self, feeds: Option<Vec<i64>>) -> Result<Vec<i64>, AppError> {
        self.db.transaction(|tx| {
            let ids = {
                let mut statement =
                    tx.prepare("SELECT id, feed_id FROM articles WHERE read = 0 ORDER BY id")?;
                let rows = statement
                    .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
                rows.collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .filter(|(_, feed)| feeds.as_ref().is_none_or(|f| f.contains(feed)))
                    .map(|(id, _)| id)
                    .collect::<Vec<_>>()
            };
            for id in &ids {
                tx.execute("UPDATE articles SET read = 1 WHERE id = ?1", [id])?;
            }
            Ok(ids)
        })
    }

    /// Marks these articles unread again (Undo after Mark All as Read), and
    /// returns how many changed. Articles deleted since are skipped.
    pub fn mark_unread(&mut self, articles: &[i64]) -> Result<u32, AppError> {
        self.db.transaction(|tx| {
            let mut changed = 0;
            for id in articles {
                changed += tx.execute(
                    "UPDATE articles SET read = 0 WHERE id = ?1 AND read = 1",
                    [id],
                )?;
            }
            Ok(changed as u32)
        })
    }

    pub fn article_text(&self, id: i64) -> Result<ArticleText, AppError> {
        self.db
            .connection()
            .query_row(
                "SELECT a.id, COALESCE(f.custom_title, f.title), a.title, a.author, a.url,
                        a.published_at_ms, COALESCE(a.full_html, a.html), a.read, a.bookmarked,
                        a.full_html IS NOT NULL
                 FROM articles a JOIN feeds f ON f.id = a.feed_id WHERE a.id = ?1",
                [id],
                |r| {
                    let html: String = r.get(6)?;
                    let url: Option<String> = r.get(4)?;
                    let title: String = r.get(2)?;
                    let blocks = forge_reader::blocks_from_html(
                        &html,
                        &mut forge_reader::WebResources::new(url.as_deref()),
                    );
                    Ok(ArticleText {
                        id: r.get(0)?,
                        feed_title: r.get(1)?,
                        title: title.clone(),
                        author: r.get(3)?,
                        url,
                        published_at_ms: r.get(5)?,
                        body: forge_reader::ReaderSection {
                            id: "article".into(),
                            title,
                            blocks,
                        },
                        read: r.get(7)?,
                        bookmarked: r.get(8)?,
                        full: r.get(9)?,
                    })
                },
            )
            .optional()?
            .ok_or(AppError::NotFound)
    }
}

/// A subscription to add from an OPML file: its address already checked.
pub struct Importing<'a> {
    pub url: String,
    pub entry: &'a opml::Entry,
}

/// What `Store::import` did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Imported {
    pub added: u32,
    pub already_subscribed: u32,
    pub folders_created: u32,
    pub filters_added: u32,
}

impl Store {
    /// Makes a folder in `parent` (at the top with None), or returns the
    /// one of that name already there (names match ignoring case).
    pub fn create_folder(
        &mut self,
        name: &str,
        parent: Option<i64>,
        now_ms: i64,
    ) -> Result<i64, AppError> {
        let name = folder_name(name)?;
        self.db.transaction(|tx| {
            if let Some(parent) = parent {
                folder_exists(tx, parent)?;
            }
            Ok(ensure_folder(tx, parent, &name, now_ms)?.0)
        })
    }

    /// Renames a folder. Another folder beside it with that name is refused.
    pub fn rename_folder(&mut self, id: i64, name: &str) -> Result<(), AppError> {
        let name = folder_name(name)?;
        self.db.transaction(|tx| {
            let parent: Option<i64> = tx
                .query_row("SELECT parent_id FROM folders WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?
                .ok_or(AppError::NotFound)?;
            if let Some(other) = find_folder(tx, parent, &name)? {
                if other != id {
                    return Err(AppError::InvalidInput {
                        reason: "a folder beside it already has that name".into(),
                    });
                }
            }
            tx.execute(
                "UPDATE folders SET name = ?2 WHERE id = ?1",
                params![id, name],
            )?;
            Ok(())
        })
    }

    /// Deletes a folder. Its feeds and folders move up into its parent (or
    /// to the top); no subscription is lost.
    pub fn delete_folder(&mut self, id: i64) -> Result<(), AppError> {
        self.db.transaction(|tx| {
            let parent: Option<i64> = tx
                .query_row("SELECT parent_id FROM folders WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?
                .ok_or(AppError::NotFound)?;
            tx.execute(
                "UPDATE feeds SET folder_id = ?2 WHERE folder_id = ?1",
                params![id, parent],
            )?;
            tx.execute(
                "UPDATE folders SET parent_id = ?2 WHERE parent_id = ?1",
                params![id, parent],
            )?;
            tx.execute("DELETE FROM folders WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Puts a feed in a folder (at the top with None).
    pub fn move_feed(&mut self, feed: i64, folder: Option<i64>) -> Result<(), AppError> {
        self.db.transaction(|tx| {
            if let Some(folder) = folder {
                folder_exists(tx, folder)?;
            }
            if tx.execute(
                "UPDATE feeds SET folder_id = ?2 WHERE id = ?1",
                params![feed, folder],
            )? == 0
            {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
    }

    /// Whether a feed's new articles count for the notification.
    pub fn set_feed_notify(&mut self, feed: i64, notify: bool) -> Result<(), AppError> {
        self.db.transaction(|tx| {
            if tx.execute(
                "UPDATE feeds SET notify = ?2 WHERE id = ?1",
                params![feed, notify],
            )? == 0
            {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
    }

    /// Whether a feed's articles are fetched in full from their pages.
    /// Turning it on leaves its existing articles to be fetched next time.
    pub fn set_feed_full_text(&mut self, feed: i64, on: bool) -> Result<(), AppError> {
        self.db.transaction(|tx| {
            if tx.execute("UPDATE feeds SET full_text = ?2 WHERE id = ?1", params![feed, on])? == 0 {
                return Err(AppError::NotFound);
            }
            if on {
                // Failures get another try now the person asked again.
                tx.execute(
                    "UPDATE articles SET full_status = NULL WHERE feed_id = ?1 AND full_html IS NULL",
                    [feed],
                )?;
            }
            Ok(())
        })
    }

    /// Up to `limit` articles of feeds that want full articles and haven't
    /// been tried, newest first: their ids and addresses.
    /// Each with its page's address and its feed's.
    pub fn full_articles_wanted(&self, limit: u32) -> Result<Vec<(i64, String, String)>, AppError> {
        let mut q = self.db.connection().prepare(
            "SELECT a.id, a.url, f.url FROM articles a JOIN feeds f ON f.id = a.feed_id
             WHERE f.full_text = 1 AND a.full_status IS NULL AND a.url IS NOT NULL
             ORDER BY a.published_at_ms DESC, a.id DESC LIMIT ?1",
        )?;
        let rows = q
            .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The address of one article's page, and of its feed.
    pub fn article_url(&self, id: i64) -> Result<(Option<String>, String), AppError> {
        self.db
            .connection()
            .query_row(
                "SELECT a.url, f.url FROM articles a JOIN feeds f ON f.id = a.feed_id
                 WHERE a.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(AppError::NotFound)
    }

    /// Saves what fetching full articles did: the article's text (also put
    /// in the search index), or the kind of failure, so it isn't tried
    /// again on every refresh. Articles deleted since are skipped. Returns
    /// how many were saved in full.
    pub fn save_full_articles(
        &mut self,
        results: &[(i64, Result<String, AppError>)],
    ) -> Result<u32, AppError> {
        self.db.transaction(|tx| {
            let mut saved = 0;
            for (id, result) in results {
                match result {
                    Ok(html) => {
                        let title: Option<String> = tx
                            .query_row("SELECT title FROM articles WHERE id = ?1", [id], |r| {
                                r.get(0)
                            })
                            .optional()?;
                        let Some(title) = title else { continue };
                        tx.execute(
                            "UPDATE articles SET full_html = ?2, full_status = 'ok' WHERE id = ?1",
                            params![id, html],
                        )?;
                        index_article(tx, *id, &title, html)?;
                        saved += 1;
                    }
                    // Stopped before it finished: tried again next time.
                    Err(AppError::Cancelled | AppError::Interrupted | AppError::ShutDown) => {}
                    Err(e) => {
                        tx.execute(
                            "UPDATE articles SET full_status = ?2 WHERE id = ?1",
                            params![id, e.problem()],
                        )?;
                    }
                }
            }
            Ok(saved)
        })
    }

    /// The filter patterns, in the order they were added.
    pub fn filters(&self) -> Result<Vec<String>, AppError> {
        let mut q = self
            .db
            .connection()
            .prepare("SELECT pattern FROM filters ORDER BY id")?;
        let rows = q
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(rows)
    }

    /// Adds the picked subscriptions from an OPML file, in one transaction,
    /// without changing anything the library already has: a feed already
    /// subscribed keeps its name, folder and options. `existing` says which address is
    /// already which feed (the caller matches addresses).
    pub fn import(
        &mut self,
        picked: &[Importing<'_>],
        existing: &dyn Fn(&str) -> Option<i64>,
        filters: &[String],
        now_ms: i64,
    ) -> Result<Imported, AppError> {
        self.db.transaction(|tx| {
            let mut done = Imported::default();
            for item in picked {
                let entry = item.entry;
                if existing(&item.url).is_some() {
                    done.already_subscribed += 1;
                    continue;
                }
                let folder = ensure_path(tx, &entry.folder, now_ms, &mut done.folders_created)?;
                let extras = (!entry.extras.is_empty())
                    .then(|| serde_json::to_string(&entry.extras))
                    .transpose()
                    .map_err(|e| AppError::Storage {
                        reason: e.to_string(),
                    })?;
                tx.execute(
                    "INSERT INTO feeds (url, title, custom_title, site_url, added_at_ms, folder_id,
                                        notify, full_text, opml_extras)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        item.url,
                        entry.title.clone().unwrap_or_else(|| host_of(&item.url)),
                        // The name the person knew it by in the other app.
                        entry.title,
                        entry.site_url,
                        now_ms,
                        folder,
                        entry.options.notify.unwrap_or(true),
                        entry.options.full_text.unwrap_or(false),
                        extras,
                    ],
                )?;
                done.added += 1;
            }
            for pattern in filters {
                done.filters_added += tx.execute(
                    "INSERT OR IGNORE INTO filters (pattern, added_at_ms) VALUES (?1, ?2)",
                    params![pattern, now_ms],
                )? as u32;
            }
            Ok(done)
        })
    }

    /// The library as an OPML tree: folders by name, feeds by title.
    pub fn export_tree(&self) -> Result<opml::ExportFolder, AppError> {
        let c = self.db.connection();
        let folders: Vec<(i64, Option<i64>, String)> = c
            .prepare("SELECT id, parent_id, name FROM folders ORDER BY name COLLATE NOCASE, id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let feeds: Vec<(Option<i64>, opml::ExportFeed)> = c
            .prepare(
                "SELECT folder_id, COALESCE(custom_title, title), url, site_url, notify, full_text, opml_extras
                 FROM feeds ORDER BY COALESCE(custom_title, title) COLLATE NOCASE, id",
            )?
            .query_map([], |r| {
                let extras: Option<String> = r.get(6)?;
                Ok((
                    r.get(0)?,
                    opml::ExportFeed {
                        title: r.get(1)?,
                        url: r.get(2)?,
                        site_url: r.get(3)?,
                        notify: r.get(4)?,
                        full_text: r.get(5)?,
                        // Unreadable kept text is dropped, not fatal.
                        extras: extras
                            .and_then(|e| serde_json::from_str(&e).ok())
                            .unwrap_or_default(),
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut feeds: Vec<Option<(Option<i64>, opml::ExportFeed)>> =
            feeds.into_iter().map(Some).collect();
        let ids: std::collections::HashSet<i64> = folders.iter().map(|f| f.0).collect();
        fn build(
            id: Option<i64>,
            name: String,
            depth: u32,
            folders: &[(i64, Option<i64>, String)],
            feeds: &mut [Option<(Option<i64>, opml::ExportFeed)>],
        ) -> opml::ExportFolder {
            let mine = feeds
                .iter_mut()
                .filter(|f| f.as_ref().is_some_and(|(folder, _)| *folder == id))
                .filter_map(|f| f.take().map(|(_, feed)| feed))
                .collect();
            let subs = if depth < 64 {
                folders
                    .iter()
                    .filter(|f| f.1 == id)
                    .map(|f| build(Some(f.0), f.2.clone(), depth + 1, folders, feeds))
                    .collect()
            } else {
                Vec::new()
            };
            opml::ExportFolder {
                name,
                folders: subs,
                feeds: mine,
            }
        }
        // A feed whose folder is gone (never, with the foreign key) goes at the top.
        for (folder, _) in feeds.iter_mut().flatten() {
            if folder.is_some_and(|f| !ids.contains(&f)) {
                *folder = None;
            }
        }
        Ok(build(None, String::new(), 0, &folders, &mut feeds))
    }
}

/// The ids of a folder and every folder inside it.
pub fn folder_subtree(id: i64, folders: &[Folder]) -> std::collections::HashSet<i64> {
    let mut inside = std::collections::HashSet::from([id]);
    loop {
        let before = inside.len();
        for f in folders {
            if f.parent_id.is_some_and(|p| inside.contains(&p)) {
                inside.insert(f.id);
            }
        }
        if inside.len() == before {
            return inside;
        }
    }
}

/// A folder name the person typed: cleaned, and not empty.
fn folder_name(raw: &str) -> Result<String, AppError> {
    opml::clean_name(raw).ok_or_else(|| AppError::InvalidInput {
        reason: "a folder needs a name".into(),
    })
}

fn folder_exists(tx: &Transaction<'_>, id: i64) -> Result<(), AppError> {
    tx.query_row("SELECT 1 FROM folders WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .ok_or(AppError::NotFound)
}

fn find_folder(
    tx: &Transaction<'_>,
    parent: Option<i64>,
    name: &str,
) -> Result<Option<i64>, AppError> {
    Ok(tx
        .query_row(
            "SELECT id FROM folders WHERE parent_id IS ?1 AND name = ?2 COLLATE NOCASE ORDER BY id",
            params![parent, name],
            |r| r.get(0),
        )
        .optional()?)
}

/// The folder called `name` in `parent`, made if missing. True when made.
fn ensure_folder(
    tx: &Transaction<'_>,
    parent: Option<i64>,
    name: &str,
    now_ms: i64,
) -> Result<(i64, bool), AppError> {
    if let Some(id) = find_folder(tx, parent, name)? {
        return Ok((id, false));
    }
    tx.execute(
        "INSERT INTO folders (parent_id, name, added_at_ms) VALUES (?1, ?2, ?3)",
        params![parent, name, now_ms],
    )?;
    Ok((tx.last_insert_rowid(), true))
}

/// The folder at `path` (outermost first), made where missing.
fn ensure_path(
    tx: &Transaction<'_>,
    path: &[String],
    now_ms: i64,
    created: &mut u32,
) -> Result<Option<i64>, AppError> {
    let mut parent = None;
    for name in path {
        let (id, made) = ensure_folder(tx, parent, name, now_ms)?;
        *created += made as u32;
        parent = Some(id);
    }
    Ok(parent)
}

/// The article flags a person sets.
#[derive(Clone, Copy)]
pub enum Flag {
    Read,
    Bookmarked,
}

fn feed_exists(tx: &Transaction<'_>, id: i64) -> Result<bool, AppError> {
    Ok(tx
        .query_row("SELECT 1 FROM feeds WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

/// Inserts new articles and updates the text of known ones, keeping their
/// read and bookmark state and their date (so an edited post doesn't jump
/// to the top). Returns how many were new.
fn merge_articles(
    tx: &Transaction<'_>,
    feed_id: i64,
    parsed: &ParsedFeed,
    now_ms: i64,
) -> Result<u32, AppError> {
    let mut added = 0;
    for a in &parsed.articles {
        let summary = feed::summary(&a.html, SUMMARY_CHARS);
        let changed = tx.execute(
            "UPDATE articles SET title = ?3, url = ?4, author = ?5, html = ?6, summary = ?7
             WHERE feed_id = ?1 AND guid = ?2",
            params![feed_id, a.guid, a.title, a.url, a.author, a.html, summary],
        )?;
        if changed > 0 {
            let id: i64 = tx.query_row(
                "SELECT id FROM articles WHERE feed_id = ?1 AND guid = ?2",
                params![feed_id, a.guid],
                |r| r.get(0),
            )?;
            index_article(tx, id, &a.title, &a.html)?;
        }
        if changed == 0 {
            tx.execute(
                "INSERT INTO articles (feed_id, guid, title, url, author, html, summary,
                                       published_at_ms, fetched_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    feed_id,
                    a.guid,
                    a.title,
                    a.url,
                    a.author,
                    a.html,
                    summary,
                    a.published_at_ms.unwrap_or(now_ms),
                    now_ms
                ],
            )?;
            index_article(tx, tx.last_insert_rowid(), &a.title, &a.html)?;
            added += 1;
        }
    }
    Ok(added)
}

/// Deletes the feed's read, unbookmarked articles fetched more than
/// `KEEP_READ_MS` ago that the feed no longer lists. Unread and bookmarked
/// articles are always kept.
fn forget_old_articles(
    tx: &Transaction<'_>,
    feed_id: i64,
    parsed: &ParsedFeed,
    now_ms: i64,
) -> Result<u32, AppError> {
    let listed: std::collections::HashSet<&str> =
        parsed.articles.iter().map(|a| a.guid.as_str()).collect();
    let old: Vec<(i64, String)> = {
        let mut q = tx.prepare(
            "SELECT id, guid FROM articles
             WHERE feed_id = ?1 AND read = 1 AND bookmarked = 0 AND fetched_at_ms < ?2",
        )?;
        let rows = q
            .query_map(params![feed_id, now_ms - KEEP_READ_MS], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut forgotten = 0;
    for (id, guid) in old {
        if !listed.contains(guid.as_str()) {
            forgotten += tx.execute("DELETE FROM articles WHERE id = ?1", [id])? as u32;
        }
    }
    Ok(forgotten)
}

/// Puts an article's title and text in the search index (replacing what
/// was there).
fn index_article(tx: &Transaction<'_>, id: i64, title: &str, html: &str) -> Result<(), AppError> {
    tx.execute("DELETE FROM article_search WHERE rowid = ?1", [id])?;
    tx.execute(
        "INSERT INTO article_search (rowid, title, body) VALUES (?1, ?2, ?3)",
        params![id, title, feed::paragraphs(html).join("\n")],
    )?;
    Ok(())
}

/// Searches the library through its own read-only connection, so a search
/// never waits for a command and never counts as a change (SQLite's WAL
/// lets it read while the command thread writes).
pub struct Searcher {
    reader: Reader,
}

impl Searcher {
    /// A searcher over `store`'s file (forge-storage's shared reader).
    pub fn open(store: &Store) -> Result<Self, AppError> {
        Ok(Self {
            reader: store.db.reader()?,
        })
    }

    /// The ids of the articles whose title or text holds every word of
    /// `query` (a word matches as a prefix: "feed" finds "feeds"). Empty
    /// for a query with no words.
    pub fn search(&self, query: &str) -> Result<Vec<i64>, AppError> {
        let Some(expression) = match_expression(query) else {
            return Ok(Vec::new());
        };
        self.reader.read(|connection| {
            let mut q = connection.prepare_cached(
                "SELECT rowid FROM article_search WHERE article_search MATCH ?1 LIMIT ?2",
            )?;
            let ids = q
                .query_map(params![expression, MAX_RESULTS as i64], |r| r.get(0))?
                .collect::<Result<Vec<i64>, _>>()?;
            Ok(ids)
        })
    }
}

/// What the person typed as an FTS5 query: each word quoted (so `AND`,
/// `-` or a `"` in it is text, not syntax) and matched as a prefix.
fn match_expression(query: &str) -> Option<String> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// A name for a feed that has no title: its address's host.
fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .filter(|h| !h.is_empty())
        .unwrap_or(url)
        .to_string()
}
