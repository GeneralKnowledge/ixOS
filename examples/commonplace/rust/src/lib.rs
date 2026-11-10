//! Commonplace's Rust core: a local-first reader of RSS, Atom and JSON
//! feeds. Everything the SwiftUI app calls is in the `#[uniffi::export]`
//! blocks here and in `model.rs`; `forge build` turns them into Swift
//! (`import ForgeFFI`).
//!
//! It follows the ixOS application model (docs/app-model-review.md):
//!
//! - **commands** (subscribe, unsubscribe, rename, mark read, bookmark,
//!   apply a refresh) run one at a time on the runtime's command thread, each
//!   in one SQLite transaction, and publish a new snapshot before the
//!   runtime announces the change;
//! - **queries** (`feeds`, `timeline`, `article`) read that snapshot and
//!   never wait for a command;
//! - **network work** (fetching feeds) runs off the command thread as
//!   on-screen work: the app going to the background, the host's
//!   [`Cancellation`] or shutdown stop it. Only its result is applied by a
//!   command.
//!
//! The reader's text comes through [`ArticleText`], the boundary the native
//! reader from docs/native-capability-roadmap.md will replace.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use forge_app::{Cancellation, DataDirectory, Published, Runtime};
use forge_net::{CancelHandle, HttpClient, Request, UreqClient};
use forge_runtime::now_ms;

mod feed;
mod model;
mod store;

pub use model::*;
use store::{FetchResult, Fetched, Flag, Snapshot, Store};

uniffi::setup_scaffolding!("forge_ffi");

// Native services the host implements (Keychain, app directories, local
// notifications, permissions, deep links), as `crate::native`.
forge_services::native_services!();

// `forge_log` for the host: Swift's `ForgeLog("Category")` writes into the
// same log as Rust.
forge_runtime::log_export!();

/// How long one feed may take to answer.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// What the command thread owns: the database, and where it publishes.
struct State {
    store: Store,
    published: Arc<Published<Snapshot>>,
}

impl State {
    /// Publishes what the database now holds. Called after a transaction
    /// committed, so a failed read is logged, not returned: the change
    /// itself happened.
    fn publish(&self) {
        match self.store.snapshot() {
            Ok(snapshot) => self.published.set(snapshot),
            Err(e) => forge_runtime::logging::error!("could not read the library back: {e}"),
        }
    }
}

/// The app instance. ForgeKit's `ForgeHost` opens one per process.
#[derive(uniffi::Object)]
pub struct AppCore {
    rt: Runtime<State>,
    snapshot: Arc<Published<Snapshot>>,
    http: Arc<dyn HttpClient>,
    #[cfg(feature = "devtools")]
    _devtools: Option<forge_devtools::DevTools>,
}

#[uniffi::export]
impl AppCore {
    /// Opens the library in `storage_directory` (created if missing) and
    /// holds it, so no second core can write there.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, AppError> {
        Self::open(storage_directory, Arc::new(UreqClient::new()))
    }

    /// Every subscription, by title.
    pub fn feeds(&self) -> Vec<Feed> {
        self.snapshot.get().feeds.clone()
    }

    /// The articles a timeline shows, newest first.
    pub fn timeline(&self, timeline: Timeline) -> Vec<ArticleSummary> {
        let snapshot = self.snapshot.get();
        snapshot
            .articles
            .iter()
            .filter(|a| match &timeline {
                Timeline::All => true,
                Timeline::Unread => !a.read,
                Timeline::Bookmarked => a.bookmarked,
                Timeline::Feed { feed_id } => a.feed_id == *feed_id,
            })
            .cloned()
            .collect()
    }

    /// How many articles a timeline holds and how many of them are unread.
    pub fn counts(&self, timeline: Timeline) -> TimelineCounts {
        let articles = self.timeline(timeline);
        TimelineCounts {
            total: articles.len() as u32,
            unread: articles.iter().filter(|a| !a.read).count() as u32,
        }
    }

    /// One article's row, or nil once its feed is gone.
    pub fn article(&self, id: i64) -> Option<ArticleSummary> {
        self.snapshot
            .get()
            .articles
            .iter()
            .find(|a| a.id == id)
            .cloned()
    }

    /// Subscribes to the feed at `url` (`https://` is assumed when no scheme
    /// is given; `feed://` means `https://`): fetches it, checks it is a
    /// feed, and saves it with its articles. On-screen work until it is
    /// saved: `cancellation`, the background or shutdown stop the fetch.
    pub async fn add_feed(
        &self,
        url: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Feed, AppError> {
        let url = normalise_url(&url)?;
        if let Some(existing) = self.snapshot.get().feeds.iter().find(|f| f.url == url) {
            return Err(AppError::AlreadySubscribed {
                feed_id: existing.id,
            });
        }
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        let fetched_url = url.clone();
        let fetched = self
            .rt
            .run("fetch-new-feed", work, move |cancel| {
                fetch(&*http, &fetched_url, None, None, cancel)
            })
            .await?;
        let FetchResult::Parsed {
            feed,
            etag,
            last_modified,
        } = fetched
        else {
            return Err(AppError::NotAFeed {
                reason: "the server said nothing changed for a first request".into(),
            });
        };
        self.rt
            .command("add-feed", move |state| {
                let (id, added) =
                    state
                        .store
                        .add_feed(&url, &feed, etag, last_modified, now_ms())?;
                state.publish();
                forge_runtime::logging::info!("subscribed with {added} articles");
                state
                    .published
                    .get()
                    .feeds
                    .iter()
                    .find(|f| f.id == id)
                    .cloned()
                    .ok_or(AppError::NotFound)
            })
            .await
    }

    /// Unsubscribes, deleting the feed's articles (bookmarked ones too).
    pub async fn remove_feed(&self, feed_id: i64) -> Result<(), AppError> {
        self.rt
            .command("remove-feed", move |state| {
                state.store.remove_feed(feed_id)?;
                state.publish();
                Ok(())
            })
            .await
    }

    /// Gives a feed the person's own name; blank goes back to the feed's.
    pub async fn rename_feed(&self, feed_id: i64, title: String) -> Result<(), AppError> {
        self.rt
            .command("rename-feed", move |state| {
                state.store.rename_feed(feed_id, &title)?;
                state.publish();
                Ok(())
            })
            .await
    }

    /// Fetches every feed (conditionally: unchanged feeds cost one small
    /// request) and saves what is new. One feed failing doesn't stop the
    /// others; each failure is in the report and on the feed. On-screen
    /// work: stopped by `cancellation`, the background or shutdown, in
    /// which case nothing is saved.
    pub async fn refresh(
        &self,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<RefreshReport, AppError> {
        let sources = self.snapshot.get().sources.clone();
        let http = Arc::clone(&self.http);
        let work = self.rt.foreground(cancellation.as_deref());
        let fetched = self
            .rt
            .run("refresh", work, move |cancel| {
                let mut fetched = Vec::with_capacity(sources.len());
                for source in &sources {
                    let result = fetch(
                        &*http,
                        &source.url,
                        source.etag.as_deref(),
                        source.last_modified.as_deref(),
                        cancel,
                    );
                    if cancel.is_cancelled() {
                        return Err(AppError::Cancelled);
                    }
                    fetched.push((source.clone(), result));
                }
                Ok::<_, AppError>(fetched)
            })
            .await?;
        let mut report = RefreshReport {
            feeds: fetched.len() as u32,
            unchanged: 0,
            new_articles: 0,
            failures: Vec::new(),
        };
        let mut apply = Vec::with_capacity(fetched.len());
        for (source, result) in fetched {
            match &result {
                Ok(FetchResult::NotModified) => report.unchanged += 1,
                Ok(_) => {}
                Err(e) => report.failures.push(FeedFailure {
                    feed_id: source.id,
                    title: source.title.clone(),
                    error: e.clone(),
                }),
            }
            apply.push(Fetched {
                feed_id: source.id,
                result,
            });
        }
        report.new_articles = self
            .rt
            .command("apply-refresh", move |state| {
                let added = state.store.apply(&apply, now_ms())?;
                state.publish();
                Ok::<_, AppError>(added)
            })
            .await?;
        forge_runtime::logging::info!(
            "refreshed {} feeds: {} new articles, {} unchanged, {} failed",
            report.feeds,
            report.new_articles,
            report.unchanged,
            report.failures.len()
        );
        Ok(report)
    }

    /// Opens an article for reading: marks it read and returns its text.
    pub async fn open_article(&self, article_id: i64) -> Result<ArticleText, AppError> {
        self.rt
            .command("open-article", move |state| {
                state.store.set_flag(article_id, Flag::Read, true)?;
                let text = state.store.article_text(article_id)?;
                state.publish();
                Ok(text)
            })
            .await
    }

    pub async fn set_read(&self, article_id: i64, read: bool) -> Result<(), AppError> {
        self.rt
            .command("set-read", move |state| {
                state.store.set_flag(article_id, Flag::Read, read)?;
                state.publish();
                Ok(())
            })
            .await
    }

    pub async fn set_bookmarked(&self, article_id: i64, bookmarked: bool) -> Result<(), AppError> {
        self.rt
            .command("set-bookmarked", move |state| {
                state
                    .store
                    .set_flag(article_id, Flag::Bookmarked, bookmarked)?;
                state.publish();
                Ok(())
            })
            .await
    }

    /// Marks every article of one feed (or of every feed, with nil) read.
    /// Returns how many changed.
    pub async fn mark_all_read(&self, feed_id: Option<i64>) -> Result<u32, AppError> {
        self.rt
            .command("mark-all-read", move |state| {
                let n = state.store.mark_all_read(feed_id)?;
                state.publish();
                Ok(n)
            })
            .await
    }
}

// The runtime half of the API: `events()`, `set_phase(phase)`, `phase()`,
// `revision()` and `shutdown()`. ForgeKit's `ForgeHost` calls them.
forge_app::export_runtime!(AppCore, rt);

/// How many articles a timeline holds.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TimelineCounts {
    pub total: u32,
    pub unread: u32,
}

impl AppCore {
    /// Opens the core with `http` for its requests (a test double in tests).
    fn open(storage_directory: String, http: Arc<dyn HttpClient>) -> Result<Arc<Self>, AppError> {
        forge_runtime::logging::init();
        if storage_directory.is_empty() {
            return Err(AppError::InvalidInput {
                reason: "storage directory must not be empty".into(),
            });
        }
        let storage_directory = PathBuf::from(storage_directory);
        let directory = DataDirectory::open(&storage_directory)?;
        let store = Store::open(directory.path())?;
        let snapshot = store.snapshot()?;
        // Never log feed addresses, titles or article text.
        forge_runtime::logging::info!(
            "opened with {} feeds and {} articles",
            snapshot.feeds.len(),
            snapshot.articles.len()
        );
        let published = Arc::new(Published::new(snapshot));
        let state = State {
            store,
            published: Arc::clone(&published),
        };
        let rt = Runtime::with_directory("commonplace", directory, state);
        Ok(Arc::new(Self {
            #[cfg(feature = "devtools")]
            _devtools: devtools::start(&storage_directory, &published, &rt),
            rt,
            snapshot: published,
            http,
        }))
    }
}

/// The address to subscribe to, from what the person typed.
fn normalise_url(input: &str) -> Result<String, AppError> {
    let input = input.trim();
    if input.is_empty() || input.contains(char::is_whitespace) {
        return Err(AppError::InvalidInput {
            reason: "a feed address has no spaces and isn't empty".into(),
        });
    }
    let url = if let Some(rest) = input.strip_prefix("feed://") {
        format!("https://{rest}")
    } else if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let lower = url.to_ascii_lowercase();
    let host = url.split("://").nth(1).unwrap_or("");
    if !(lower.starts_with("https://") || lower.starts_with("http://")) || host.is_empty() {
        return Err(AppError::InvalidInput {
            reason: "a feed address starts with http:// or https://".into(),
        });
    }
    Ok(url)
}

/// Fetches and parses one feed, asking only for changes when the last
/// fetch left validators (ETag, Last-Modified).
fn fetch(
    http: &dyn HttpClient,
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
    cancel: &CancelHandle,
) -> Result<FetchResult, AppError> {
    let mut request = Request::get(url)
        .timeout(FETCH_TIMEOUT)
        .header(
            "Accept",
            "application/rss+xml, application/atom+xml, application/feed+json, application/json;q=0.9, application/xml;q=0.9, */*;q=0.5",
        )
        .header("User-Agent", "Commonplace/0.1 (ixOS)");
    if let Some(etag) = etag {
        request = request.header("If-None-Match", etag);
    }
    if let Some(date) = last_modified {
        request = request.header("If-Modified-Since", date);
    }
    let response = http.send_cancellable(&request, cancel)?;
    if response.status == 304 {
        return Ok(FetchResult::NotModified);
    }
    if !response.is_success() {
        return Err(AppError::Http {
            status: response.status,
        });
    }
    let feed = feed::parse(&response.body)?;
    Ok(FetchResult::Parsed {
        feed,
        etag: response.header("ETag").map(str::to_owned),
        last_modified: response.header("Last-Modified").map(str::to_owned),
    })
}

/// What this app shows `forge device` in development builds. Never feed
/// addresses or article text.
#[cfg(feature = "devtools")]
mod devtools {
    use super::*;
    use forge_devtools::{Config, DevTools};

    pub(crate) fn start(
        storage_directory: &std::path::Path,
        snapshot: &Arc<Published<Snapshot>>,
        rt: &Runtime<State>,
    ) -> Option<DevTools> {
        let app = option_env!("FORGE_APP_ID").unwrap_or(env!("CARGO_PKG_NAME"));
        let config = Config::new(
            app,
            env!("CARGO_PKG_VERSION"),
            storage_directory.join(".forge-devtools-pairing"),
        );
        let tools = match DevTools::start(config, rt.lifecycle().clone()) {
            Ok(tools) => tools,
            Err(e) => {
                forge_runtime::logging::warn!("devtools did not start: {e}");
                return None;
            }
        };
        let feeds = Arc::clone(snapshot);
        tools.probe("feeds", move || feeds.get().feeds.len().to_string());
        let unread = Arc::clone(snapshot);
        tools.probe("unread", move || {
            unread
                .get()
                .articles
                .iter()
                .filter(|a| !a.read)
                .count()
                .to_string()
        });
        Some(tools)
    }
}

#[cfg(test)]
mod tests;
