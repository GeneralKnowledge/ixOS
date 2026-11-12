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
//!   command. The same refresh also runs in the background when iOS grants
//!   a window (`native::background`), as work that lives until iOS ends the
//!   window.
//!
//! The reader's text comes through [`ArticleText`], the boundary the native
//! reader from docs/native-capability-roadmap.md will replace.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use forge_app::{Cancellation, DataDirectory, Published, Runtime, Work};
use forge_net::{CancelHandle, HttpClient, Request, UreqClient};
use forge_runtime::now_ms;

mod feed;
mod model;
mod store;

pub use model::*;
use store::{FetchResult, Fetched, Flag, Snapshot, Store};

// No namespace argument: UniFFI takes the library's name (commonplace_core), so this
// crate keeps its own bindings when another app links it.
uniffi::setup_scaffolding!();

// Native services the host implements (Keychain, app directories, local
// notifications, permissions, deep links), as `crate::native`, `http` (on
// iOS feeds are fetched with URLSession) and `background` (refreshing while
// the app is closed).
forge_services::native_services!(http, background);

/// The background refresh task: forge.toml `[ios] background_tasks`.
const BACKGROUND_REFRESH: &str = "org.forgemobile.agentlab.commonplace.refresh";
/// How often to ask iOS for a background refresh. iOS decides when (and
/// whether) it runs, from how the app is used.
const BACKGROUND_REFRESH_EVERY: Duration = Duration::from_secs(60 * 60);
/// The notification after a background refresh found new articles; also
/// its category (a Mark All as Read button).
const NEW_ARTICLES: &str = "commonplace.new-articles";
/// The file whose presence turns those notifications on.
const NEW_ARTICLE_ALERTS_FILE: &str = "new-article-alerts";

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
    storage_directory: PathBuf,
    /// Set by `new`: tells the refresh log whether URLSession or ureq ran.
    transport: std::sync::OnceLock<Arc<native::http::PreferInstalled>>,
    #[cfg(feature = "devtools")]
    _devtools: Option<forge_devtools::DevTools>,
}

#[uniffi::export]
impl AppCore {
    /// Opens the library in `storage_directory` (created if missing) and
    /// holds it, so no second core can write there.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, AppError> {
        // URLSession once the host has installed its services (on iOS, just
        // after this core is made); ureq before that, and in Linux tests.
        let http = Arc::new(native::http::HttpService::prefer_installed(
            Arc::new(UreqClient::new()),
            |c| c,
        ));
        let core = Self::open(storage_directory, http.clone())?;
        // Only for the log line saying which transport fetched the feeds.
        let _ = core.transport.set(http);
        core.register_background_refresh();
        core.handle_notification_actions();
        Ok(core)
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
        let work = self.rt.foreground(cancellation.as_deref());
        self.refresh_in(work).await
    }
}

impl AppCore {
    /// Fetches every feed as `work` and applies the result.
    async fn refresh_in(&self, work: Work) -> Result<RefreshReport, AppError> {
        let sources = self.snapshot.get().sources.clone();
        let http = Arc::clone(&self.http);
        let fetched = self
            .rt
            .run("refresh", work, move |cancel| {
                // One feed after another for now. Nothing below depends on
                // that: each fetch is independent and results are applied
                // together in one command, so a bounded fan-out (gap
                // `concurrent-fetches`) replaces only this loop.
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
        // Error kinds only: feed titles and addresses are the user's.
        let kinds: Vec<&str> = report.failures.iter().map(|f| f.error.kind()).collect();
        let transport = match self.transport.get() {
            Some(t) if t.uses_host() => "URLSession",
            Some(_) => "ureq",
            None => "test client",
        };
        forge_runtime::logging::info!(
            "refreshed {} feeds through {}: {} new articles, {} unchanged, {} failed {:?}",
            report.feeds,
            transport,
            report.new_articles,
            report.unchanged,
            report.failures.len(),
            kinds
        );
        Ok(report)
    }

    /// Asks iOS to refresh the feeds in the background about hourly, while
    /// the app is closed. Does nothing where the host has no background
    /// tasks (Linux tests, an app without the service installed).
    fn register_background_refresh(self: &Arc<Self>) {
        let Ok(background) = native::background::BackgroundService::installed() else {
            return;
        };
        let core = Arc::downgrade(self);
        let registered = background.register_refresh(
            BACKGROUND_REFRESH,
            BACKGROUND_REFRESH_EVERY.as_secs_f64(),
            move |run| {
                let core = core.clone();
                async move {
                    let Some(core) = core.upgrade() else {
                        return false;
                    };
                    // iOS ending the window stops the fetches.
                    let cancellation = Cancellation::new();
                    let stop = cancellation.clone();
                    run.on_cancel(move || stop.cancel());
                    // Not on-screen work: it lives until iOS ends the window.
                    let work = core.rt.lifetime(Some(&*cancellation));
                    match core.refresh_in(work).await {
                        Ok(report) => {
                            core.announce_new_articles(report.new_articles).await;
                            true
                        }
                        Err(_) => false,
                    }
                }
            },
        );
        if let Err(e) = registered {
            forge_runtime::logging::warn!("background refresh not registered: {e}");
        }
    }

    /// After a background refresh: tells the person what's new, if they
    /// turned the alerts on. Replaces the previous one, so they never pile up.
    async fn announce_new_articles(&self, new_articles: u32) {
        if new_articles == 0 || !self.new_article_alerts() {
            return;
        }
        let Ok(services) = native::NativeServices::installed() else {
            return;
        };
        let titles: Vec<String> = self
            .timeline(Timeline::Unread)
            .into_iter()
            .take(new_articles.min(3) as usize)
            .map(|a| a.title)
            .collect();
        if let Err(e) = services
            .schedule_notification(new_articles_notification(new_articles, &titles))
            .await
        {
            // Turned off in Settings since, most likely.
            forge_runtime::logging::warn!("new-articles notification not shown: {e}");
        }
    }

    /// The Mark All as Read button on that notification.
    fn handle_notification_actions(self: &Arc<Self>) {
        let Ok(services) = native::NativeServices::installed() else {
            return;
        };
        let category = native::NotificationCategory {
            id: NEW_ARTICLES.into(),
            actions: vec![native::NotificationAction::button(
                "read",
                "Mark All as Read",
            )],
        };
        if let Err(e) = services.set_notification_categories(vec![category]) {
            forge_runtime::logging::warn!("notification actions not set: {e}");
        }
        let core = Arc::downgrade(self);
        native::on_notification_response(move |response| {
            let Some(core) = core.upgrade() else { return };
            if response.category.as_deref() == Some(NEW_ARTICLES) && response.action_id == "read" {
                // iOS gives a background action a few seconds; the command
                // is one SQLite transaction.
                std::thread::spawn(move || {
                    if let Err(e) = forge_runtime::block_on(core.mark_all_read(None)) {
                        forge_runtime::logging::warn!(
                            "mark all read from a notification failed: {e}"
                        );
                    }
                });
            }
        });
    }
}

/// "3 new articles", with the first few titles.
fn new_articles_notification(count: u32, titles: &[String]) -> native::LocalNotification {
    let title = if count == 1 {
        "1 new article".to_string()
    } else {
        format!("{count} new articles")
    };
    let mut notification = native::LocalNotification::now(NEW_ARTICLES, title, titles.join("\n"));
    notification.category = Some(NEW_ARTICLES.into());
    notification
}

#[uniffi::export]
impl AppCore {
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

    /// Whether a background refresh that finds new articles shows a
    /// notification.
    pub fn new_article_alerts(&self) -> bool {
        self.storage_directory
            .join(NEW_ARTICLE_ALERTS_FILE)
            .exists()
    }

    /// Turns those notifications on (asking for permission the first time;
    /// returns false if the person declined) or off.
    pub async fn set_new_article_alerts(&self, enabled: bool) -> Result<bool, AppError> {
        let path = self.storage_directory.join(NEW_ARTICLE_ALERTS_FILE);
        let io = |e: std::io::Error| AppError::Storage {
            reason: format!("save the setting: {e}"),
        };
        if !enabled {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
                _ => Ok(false),
            };
        }
        if let Ok(services) = native::NativeServices::installed() {
            let status = services
                .request_permission(native::NativePermission::Notifications)
                .await;
            if !matches!(
                status,
                native::PermissionStatus::Granted | native::PermissionStatus::Provisional
            ) {
                return Ok(false);
            }
        }
        std::fs::write(&path, b"").map_err(io)?;
        Ok(true)
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
            storage_directory,
            transport: std::sync::OnceLock::new(),
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
