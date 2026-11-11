//! Commonplace's SQLite file: subscriptions and their articles. Only the
//! command thread touches it; every change is one transaction.

use std::path::Path;

use forge_storage::rusqlite::{params, OptionalExtension, Transaction};
use forge_storage::{Database, Opened};

use crate::feed::{self, ParsedFeed};
use crate::model::{AppError, ArticleSummary, ArticleText, Feed};

/// Schema steps: step `i` brings the file from version `i` to `i + 1`.
const MIGRATIONS: &[&str] = &["
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
"];

/// How long a row's summary is, in characters.
const SUMMARY_CHARS: usize = 200;

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
    /// Every article, newest first.
    pub articles: Vec<ArticleSummary>,
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
        Ok(Self { db })
    }

    pub fn snapshot(&self) -> Result<Snapshot, AppError> {
        let c = self.db.connection();
        let mut feeds = c.prepare(
            "SELECT f.id, f.url, COALESCE(f.custom_title, f.title), f.site_url,
                    f.refreshed_at_ms, f.last_error,
                    COUNT(a.id), COALESCE(SUM(a.read = 0), 0)
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
        Ok(Snapshot {
            feeds,
            articles,
            sources: self.sources()?,
        })
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
    /// many articles were new.
    pub fn apply(&mut self, fetched: &[Fetched], now_ms: i64) -> Result<u32, AppError> {
        self.db.transaction(|tx| {
            let mut added = 0;
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
                        added += merge_articles(tx, f.feed_id, feed, now_ms)?;
                    }
                    Err(e) => {
                        tx.execute(
                            "UPDATE feeds SET last_error = ?2 WHERE id = ?1",
                            params![f.feed_id, e.to_string()],
                        )?;
                    }
                }
            }
            Ok(added)
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
    /// returns how many changed.
    pub fn mark_all_read(&mut self, feed: Option<i64>) -> Result<u32, AppError> {
        self.db.transaction(|tx| {
            let n = match feed {
                Some(id) => tx.execute(
                    "UPDATE articles SET read = 1 WHERE read = 0 AND feed_id = ?1",
                    [id],
                )?,
                None => tx.execute("UPDATE articles SET read = 1 WHERE read = 0", [])?,
            };
            Ok(n as u32)
        })
    }

    pub fn article_text(&self, id: i64) -> Result<ArticleText, AppError> {
        self.db
            .connection()
            .query_row(
                "SELECT a.id, COALESCE(f.custom_title, f.title), a.title, a.author, a.url,
                        a.published_at_ms, a.html, a.read, a.bookmarked
                 FROM articles a JOIN feeds f ON f.id = a.feed_id WHERE a.id = ?1",
                [id],
                |r| {
                    let html: String = r.get(6)?;
                    Ok(ArticleText {
                        id: r.get(0)?,
                        feed_title: r.get(1)?,
                        title: r.get(2)?,
                        author: r.get(3)?,
                        url: r.get(4)?,
                        published_at_ms: r.get(5)?,
                        paragraphs: feed::paragraphs(&html),
                        read: r.get(7)?,
                        bookmarked: r.get(8)?,
                    })
                },
            )
            .optional()?
            .ok_or(AppError::NotFound)
    }
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
            added += 1;
        }
    }
    Ok(added)
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
