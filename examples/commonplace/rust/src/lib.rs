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
//! - **search** reads the library through its own read-only SQLite
//!   connection (FTS5), so it never waits for a command and never counts as
//!   a change.
//!
//! An article's text is a ForgeReader section ([`ArticleText::body`]):
//! the feed's HTML as styled blocks with links and pictures, laid out by
//! ForgeReader's SwiftUI views.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use forge_app::{Cancellation, DataDirectory, Published, Runtime, Work};
use forge_net::{CancelHandle, HttpClient, Request, UreqClient};
use forge_runtime::now_ms;

mod feed;
mod full;
mod model;
mod opml;
pub mod organise;
mod store;
mod suggested;

pub use model::*;
use store::{folder_subtree, FetchResult, Fetched, Flag, Importing, Searcher, Snapshot, Store};
pub use suggested::{SuggestedCategory, SuggestedFeed};

// No namespace argument: UniFFI takes the library's name (commonplace_core), so this
// crate keeps its own bindings when another app links it.
uniffi::setup_scaffolding!();

// ForgeReader's types (an article's body is a `ReaderSection`), with its
// SwiftUI reader views.
forge_reader::reader_types!();

// Native services the host implements (Keychain, app directories, local
// notifications, permissions, deep links), as `crate::native`, `http` (on
// iOS feeds are fetched with URLSession), `background` (refreshing while
// the app is closed) and `connectivity` (refreshing when the network comes
// back after a refresh found the device offline).
forge_services::native_services!(http, background, connectivity);

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
/// The most full articles one refresh fetches; the rest wait for the next.
const FULL_ARTICLES_PER_REFRESH: u32 = 10;

/// What the command thread owns: the database, and where it publishes.
struct State {
    store: Store,
    published: Arc<Published<Snapshot>>,
    /// What the last Mark All as Read can still undo. Kept in memory only:
    /// Undo is offered for a few seconds.
    undo: Option<ReadUndo>,
    next_undo: u64,
}

/// The articles a Mark All as Read changed and the person hasn't touched
/// since. Undo marks only these unread again, so an article read or marked
/// in the meantime keeps the state the person gave it.
struct ReadUndo {
    token: u64,
    articles: std::collections::HashSet<i64>,
}

impl State {
    /// The person set an article's read state themselves: Undo must leave
    /// it alone.
    fn touched(&mut self, article: i64) {
        if let Some(undo) = &mut self.undo {
            undo.articles.remove(&article);
        }
    }
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
    searcher: Arc<Searcher>,
    /// How full-article pages are fetched (the resolver is a fake in tests).
    pages: full::Policy,
    /// Set by `new`: tells the refresh log whether URLSession or ureq ran.
    transport: std::sync::OnceLock<Arc<native::http::PreferInstalled>>,
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
        let core = Self::open(storage_directory, http.clone(), full::system_lookup)?;
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

    /// The articles a timeline shows, newest first (oldest first with that
    /// setting; read ones left out of All Articles and feeds with
    /// "hide read").
    pub fn timeline(&self, timeline: Timeline) -> Vec<ArticleSummary> {
        let snapshot = self.snapshot.get();
        let hide_read = snapshot.settings.hide_read;
        let in_folder = feeds_in(&snapshot, &timeline);
        let articles = snapshot.articles.iter().filter(|a| match &timeline {
            Timeline::All => !(hide_read && a.read),
            Timeline::Unread => !a.read,
            Timeline::Bookmarked => a.bookmarked,
            Timeline::Feed { feed_id } => a.feed_id == *feed_id && !(hide_read && a.read),
            Timeline::Folder { .. } => in_folder.contains(&a.feed_id) && !(hide_read && a.read),
        });
        ordered(articles.cloned(), &snapshot.settings)
    }

    /// How many articles a timeline holds and how many of them are unread
    /// (whatever "hide read" leaves out of the list).
    pub fn counts(&self, timeline: Timeline) -> TimelineCounts {
        let snapshot = self.snapshot.get();
        let in_folder = feeds_in(&snapshot, &timeline);
        let articles: Vec<_> = snapshot
            .articles
            .iter()
            .filter(|a| match &timeline {
                Timeline::All => true,
                Timeline::Unread => !a.read,
                Timeline::Bookmarked => a.bookmarked,
                Timeline::Feed { feed_id } => a.feed_id == *feed_id,
                Timeline::Folder { .. } => in_folder.contains(&a.feed_id),
            })
            .collect();
        TimelineCounts {
            total: articles.len() as u32,
            unread: articles.iter().filter(|a| !a.read).count() as u32,
        }
    }

    /// The person's settings.
    pub fn settings(&self) -> AppSettings {
        self.snapshot.get().settings.clone()
    }

    /// Saves the person's settings; timelines follow them at once.
    pub async fn set_settings(&self, settings: AppSettings) -> Result<(), AppError> {
        self.rt
            .command("set-settings", move |state| {
                state.store.set_settings(&settings)?;
                state.publish();
                Ok(())
            })
            .await
    }

    /// The articles whose title or text holds every word of `query`, in
    /// timeline order. Reads the library directly (never waits for a
    /// command); an empty query finds nothing.
    pub async fn search(&self, query: String) -> Result<Vec<ArticleSummary>, AppError> {
        let searcher = Arc::clone(&self.searcher);
        let snapshot = Arc::clone(&self.snapshot);
        let work = self.rt.lifetime(None);
        self.rt
            .run("search", work, move |_| {
                let ids: std::collections::HashSet<i64> =
                    searcher.search(&query)?.into_iter().collect();
                let snapshot = snapshot.get();
                let found = snapshot.articles.iter().filter(|a| ids.contains(&a.id));
                Ok(ordered(found.cloned(), &snapshot.settings))
            })
            .await
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
        if let Some(existing) = self
            .snapshot
            .get()
            .feeds
            .iter()
            .find(|f| same_address(&f.url, &url))
        {
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

    /// Popular feeds to start from, by category, each marked when the
    /// person already subscribes to it. Offered only: nothing is subscribed
    /// until the person picks one, which goes through `add_feed`.
    pub fn suggested_feeds(&self) -> Vec<SuggestedCategory> {
        let snapshot = self.snapshot.get();
        suggested::suggestions(|url| snapshot.feeds.iter().any(|f| same_address(&f.url, url)))
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
        let (mut report, _) = self.refresh_in(work).await?;
        report.full_articles = self
            .fetch_full_articles(self.rt.foreground(cancellation.as_deref()))
            .await;
        Ok(report)
    }

    /// Whether a feed's articles are fetched in full from their web pages
    /// (for feeds that send only a summary). Turning it on fetches its
    /// articles with the next refresh.
    pub async fn set_feed_full_text(&self, feed_id: i64, on: bool) -> Result<(), AppError> {
        self.rt
            .command("set-feed-full-text", move |state| {
                state.store.set_feed_full_text(feed_id, on)?;
                state.publish();
                Ok(())
            })
            .await
    }

    /// Fetches one article's page now, saves its readable text and returns
    /// the article with it. On-screen work: `cancellation`, the background
    /// or shutdown stop it, and nothing is saved.
    pub async fn load_full_article(
        &self,
        article_id: i64,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<ArticleText, AppError> {
        let (url, feed_url) = self
            .rt
            .command("article-url", move |state| {
                state.store.article_url(article_id)
            })
            .await?;
        let url = url.ok_or_else(|| AppError::NoReadableText {
            reason: "the article has no address".into(),
        })?;
        let http = Arc::clone(&self.http);
        let pages = self.pages;
        let work = self.rt.foreground(cancellation.as_deref());
        let html = self
            .rt
            .run("fetch-full-article", work, move |cancel| {
                full::fetch_article(&*http, &url, Some(&feed_url), &pages, cancel)
            })
            .await?;
        self.rt
            .command("save-full-article", move |state| {
                state.store.save_full_articles(&[(article_id, Ok(html))])?;
                let text = state.store.article_text(article_id)?;
                state.publish();
                Ok(text)
            })
            .await
    }

    /// Waits until the network comes back after being unavailable, so the
    /// host can retry a refresh that found the device offline. `true` when
    /// it came back; `false` straight away where there is no connectivity
    /// service (tests on Linux). Stopped by `cancellation` or shutdown.
    pub async fn wait_until_back_online(
        &self,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<bool, AppError> {
        let Ok(connectivity) = native::connectivity::ConnectivityService::installed() else {
            return Ok(false);
        };
        let work = self.rt.lifetime(cancellation.as_deref());
        self.rt
            .run("wait-online", work, move |cancel| {
                wait_until_back_online(&connectivity, cancel)
            })
            .await
    }
}

/// Blocks until `connectivity` reports online after reporting anything
/// else, or `cancel` stops it. Watching the service costs nothing while the
/// network doesn't change.
fn wait_until_back_online(
    connectivity: &native::connectivity::ConnectivityService,
    cancel: &CancelHandle,
) -> Result<bool, AppError> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;

    let was_offline = AtomicBool::new(false);
    let (back, came_back) = mpsc::channel();
    let _watch = connectivity.watch(move |status| {
        if !status.is_online() {
            was_offline.store(true, Ordering::SeqCst);
        } else if was_offline.swap(false, Ordering::SeqCst) {
            let _ = back.send(());
        }
    });
    loop {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        match came_back.recv_timeout(Duration::from_millis(250)) {
            Ok(()) => return Ok(true),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(false),
        }
    }
}

impl AppCore {
    /// Fetches every feed as `work` and applies the result.
    /// Also returns how many new articles are from feeds that notify.
    async fn refresh_in(&self, work: Work) -> Result<(RefreshReport, u32), AppError> {
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
            full_articles: 0,
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
        let (added, notifying) = self
            .rt
            .command("apply-refresh", move |state| {
                let added = state.store.apply(&apply, now_ms())?;
                state.publish();
                Ok::<_, AppError>(added)
            })
            .await?;
        report.new_articles = added;
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
        Ok((report, notifying))
    }

    /// Fetches full articles for feeds that want them, a few per refresh,
    /// and saves each one's text or why it failed. Never fails the refresh:
    /// stopping it keeps what was fetched, and the rest wait for next time.
    /// Returns how many were saved.
    async fn fetch_full_articles(&self, work: Work) -> u32 {
        let wanted = match self
            .rt
            .command("full-articles-wanted", |state| {
                state.store.full_articles_wanted(FULL_ARTICLES_PER_REFRESH)
            })
            .await
        {
            Ok(wanted) if !wanted.is_empty() => wanted,
            Ok(_) => return 0,
            Err(e) => {
                forge_runtime::logging::warn!("full articles not listed: {}", e.kind());
                return 0;
            }
        };
        let http = Arc::clone(&self.http);
        let pages = self.pages;
        let fetched = self
            .rt
            .run("fetch-full-articles", work, move |cancel| {
                let mut results = Vec::with_capacity(wanted.len());
                for (id, url, feed_url) in wanted {
                    if cancel.is_cancelled() {
                        break;
                    }
                    let result = full::fetch_article(&*http, &url, Some(&feed_url), &pages, cancel);
                    if cancel.is_cancelled() {
                        break;
                    }
                    results.push((id, result));
                }
                Ok::<_, AppError>(results)
            })
            .await
            .unwrap_or_default();
        let tried = fetched.len();
        let saved = self
            .rt
            .command("save-full-articles", move |state| {
                let saved = state.store.save_full_articles(&fetched)?;
                state.publish();
                Ok::<_, AppError>(saved)
            })
            .await
            .unwrap_or(0);
        // Counts only: addresses are the person's.
        forge_runtime::logging::info!("full articles: {saved} of {tried} fetched");
        saved
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
                        Ok((_, notifying)) => {
                            core.announce_new_articles(notifying).await;
                            core.fetch_full_articles(core.rt.lifetime(Some(&*cancellation)))
                                .await;
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
        let notifying: std::collections::HashSet<i64> = self
            .feeds()
            .iter()
            .filter(|f| f.notify)
            .map(|f| f.id)
            .collect();
        let titles: Vec<String> = self
            .timeline(Timeline::Unread)
            .into_iter()
            .filter(|a| notifying.contains(&a.feed_id))
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
                state.touched(article_id);
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
                state.touched(article_id);
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
    /// Returns how many changed, and a token for `undo_mark_all_read`.
    pub async fn mark_all_read(&self, feed_id: Option<i64>) -> Result<MarkedRead, AppError> {
        self.rt
            .command("mark-all-read", move |state| {
                let ids = state.store.mark_all_read(feed_id)?;
                Ok(state.marked_read(ids))
            })
            .await
    }

    /// Undo for the Mark All as Read that returned `undo_token`: marks its
    /// articles unread again, except those the person has opened or
    /// marked since. Only the latest Mark All as Read can be undone, once.
    /// Returns how many changed.
    pub async fn undo_mark_all_read(&self, undo_token: u64) -> Result<u32, AppError> {
        self.rt
            .command("undo-mark-all-read", move |state| {
                let Some(undo) = state.undo.take_if(|undo| undo.token == undo_token) else {
                    return Ok(0);
                };
                let mut articles: Vec<i64> = undo.articles.into_iter().collect();
                articles.sort_unstable();
                let n = state.store.mark_unread(&articles)?;
                state.publish();
                Ok(n)
            })
            .await
    }
}

/// Folders, per-feed options and OPML (slice C3).
#[uniffi::export]
impl AppCore {
    /// Every folder, as a tree (each followed by the folders inside it).
    pub fn folders(&self) -> Vec<Folder> {
        self.snapshot.get().folders.clone()
    }

    /// Makes a folder in `parent_id` (at the top with nil), or returns the
    /// one of that name already there.
    pub async fn create_folder(
        &self,
        name: String,
        parent_id: Option<i64>,
    ) -> Result<Folder, AppError> {
        self.rt
            .command("create-folder", move |state| {
                let id = state.store.create_folder(&name, parent_id, now_ms())?;
                state.publish();
                state
                    .published
                    .get()
                    .folders
                    .iter()
                    .find(|f| f.id == id)
                    .cloned()
                    .ok_or(AppError::NotFound)
            })
            .await
    }

    pub async fn rename_folder(&self, folder_id: i64, name: String) -> Result<(), AppError> {
        self.rt
            .command("rename-folder", move |state| {
                state.store.rename_folder(folder_id, &name)?;
                state.publish();
                Ok(())
            })
            .await
    }

    /// Deletes a folder; what it holds moves up a level. No feed is lost.
    pub async fn delete_folder(&self, folder_id: i64) -> Result<(), AppError> {
        self.rt
            .command("delete-folder", move |state| {
                state.store.delete_folder(folder_id)?;
                state.publish();
                Ok(())
            })
            .await
    }

    /// Puts a feed in a folder, or at the top with nil.
    pub async fn move_feed(&self, feed_id: i64, folder_id: Option<i64>) -> Result<(), AppError> {
        self.rt
            .command("move-feed", move |state| {
                state.store.move_feed(feed_id, folder_id)?;
                state.publish();
                Ok(())
            })
            .await
    }

    /// "Organise my feeds": the feeds and folders a folder plan may use,
    /// as JSON ([`organise::OrganiseSnapshot`]). JSON because it travels
    /// through Swift to Device Lab's A2UI Lab, which reads it with these
    /// same Rust types.
    pub fn organise_snapshot_json(&self) -> String {
        let snapshot = self.snapshot.get();
        let organise = organise::snapshot(&snapshot.feeds, &snapshot.folders);
        serde_json::to_string(&organise).unwrap_or_default()
    }

    /// Applies a confirmed folder plan ([`organise::FolderPlan`] as JSON):
    /// checks it again against the library as it is now, makes its new
    /// folders and moves its feeds. Refused whole if anything moved since
    /// the plan was made. Returns how many feeds moved.
    pub async fn apply_folder_plan_json(&self, plan_json: String) -> Result<u32, AppError> {
        let plan: organise::FolderPlan =
            serde_json::from_str(&plan_json).map_err(|_| AppError::InvalidInput {
                reason: "The folder plan isn't in the expected form.".into(),
            })?;
        self.rt
            .command("apply-folder-plan", move |state| {
                let snapshot = state.store.snapshot()?;
                let current = organise::snapshot(&snapshot.feeds, &snapshot.folders);
                let moves = organise::check(&current, &plan)
                    .map_err(|e| AppError::InvalidInput { reason: e.0 })?;
                let mut made: Vec<(String, i64)> = Vec::new();
                for m in &moves {
                    let folder_id = match &m.to {
                        organise::Target::Folder { id } => Some(*id),
                        organise::Target::NoFolder => None,
                        organise::Target::NewFolder { name } => {
                            match made.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                                Some((_, id)) => Some(*id),
                                None => {
                                    let id = state.store.create_folder(name, None, now_ms())?;
                                    made.push((name.clone(), id));
                                    Some(id)
                                }
                            }
                        }
                    };
                    state.store.move_feed(m.feed_id, folder_id)?;
                }
                state.publish();
                Ok(moves.len() as u32)
            })
            .await
    }

    /// Whether a feed's new articles count for the new-articles
    /// notification (when that is on at all).
    pub async fn set_feed_notify(&self, feed_id: i64, notify: bool) -> Result<(), AppError> {
        self.rt
            .command("set-feed-notify", move |state| {
                state.store.set_feed_notify(feed_id, notify)?;
                state.publish();
                Ok(())
            })
            .await
    }

    /// Marks every article in a folder (and the folders inside it) read,
    /// with Undo like `mark_all_read`.
    pub async fn mark_folder_read(&self, folder_id: i64) -> Result<MarkedRead, AppError> {
        let feeds: Vec<i64> = {
            let snapshot = self.snapshot.get();
            feeds_in(&snapshot, &Timeline::Folder { folder_id })
                .into_iter()
                .collect()
        };
        self.rt
            .command("mark-folder-read", move |state| {
                let ids = state.store.mark_feeds_read(Some(feeds))?;
                Ok(state.marked_read(ids))
            })
            .await
    }

    /// Reads an OPML file's text and says what importing it would do.
    /// Changes nothing.
    pub fn preview_opml(&self, contents: String) -> Result<OpmlPreview, AppError> {
        let doc = read_opml(&contents)?;
        let snapshot = self.snapshot.get();
        let plan = plan_import(&doc, &snapshot.feeds);
        let mut folders: Vec<String> = Vec::new();
        for entry in &doc.entries {
            for depth in 1..=entry.folder.len() {
                let path = entry.folder[..depth].join(" / ");
                if !folders.contains(&path) {
                    folders.push(path);
                }
            }
        }
        Ok(OpmlPreview {
            feeds: doc
                .entries
                .iter()
                .zip(plan)
                .enumerate()
                .map(|(index, (entry, (status, url)))| OpmlFeed {
                    index: index as u32,
                    title: entry
                        .title
                        .clone()
                        .unwrap_or_else(|| url.clone().unwrap_or_else(|| entry.url.clone())),
                    url: url.unwrap_or_else(|| entry.url.clone()),
                    folder: entry.folder.clone(),
                    status,
                    not_used: entry.not_used.clone(),
                })
                .collect(),
            folders,
            filters: doc.filters.len() as u32,
            rejected_filters: doc.rejected_filters,
            settings_not_used: doc.settings_not_used,
        })
    }

    /// Imports the feeds at `selected` (indexes from `preview_opml`) and
    /// the file's filter patterns. Never duplicates or changes a
    /// subscription the library has (its name, folder and options stay
    /// as they are); feeds listed twice or with a bad
    /// address are skipped. New feeds get their articles with the next
    /// refresh.
    pub async fn import_opml(
        &self,
        contents: String,
        selected: Vec<u32>,
    ) -> Result<OpmlImport, AppError> {
        self.rt
            .command("import-opml", move |state| {
                let doc = read_opml(&contents)?;
                let feeds = state.published.get().feeds.clone();
                let plan = plan_import(&doc, &feeds);
                let mut picked = Vec::new();
                let mut skipped = 0;
                for (index, (entry, (status, url))) in doc.entries.iter().zip(plan).enumerate() {
                    if !selected.contains(&(index as u32)) {
                        continue;
                    }
                    // A feed listed twice counts once: the first picked.
                    match (status, url) {
                        (OpmlFeedStatus::BadAddress, _) | (_, None) => skipped += 1,
                        (_, Some(url)) if picked.iter().any(|p: &Importing<'_>| same_address(&p.url, &url)) => {
                            skipped += 1
                        }
                        (_, Some(url)) => picked.push(Importing { url, entry }),
                    }
                }
                let existing = |url: &str| feeds.iter().find(|f| same_address(&f.url, url)).map(|f| f.id);
                let done = state.store.import(&picked, &existing, &doc.filters, now_ms())?;
                state.publish();
                // Counts only: names and addresses are the person's.
                forge_runtime::logging::info!(
                    "OPML import: {} added, {} already subscribed, {} skipped, {} folders, {} filters",
                    done.added,
                    done.already_subscribed,
                    skipped,
                    done.folders_created,
                    done.filters_added
                );
                Ok(OpmlImport {
                    added: done.added,
                    already_subscribed: done.already_subscribed,
                    folders_created: done.folders_created,
                    filters_added: done.filters_added,
                    skipped,
                })
            })
            .await
    }

    /// The library's subscriptions as OPML 2.0 text: folders, feeds, their
    /// options, and the filter patterns.
    pub async fn export_opml(&self) -> Result<String, AppError> {
        self.rt
            .command("export-opml", move |state| {
                let tree = state.store.export_tree()?;
                let filters = state.store.filters()?;
                Ok(opml::write(&tree, &filters))
            })
            .await
    }
}

impl State {
    /// Keeps what a Mark All as Read changed, for Undo.
    fn marked_read(&mut self, ids: Vec<i64>) -> MarkedRead {
        self.publish();
        self.next_undo += 1;
        let marked = MarkedRead {
            count: ids.len() as u32,
            undo_token: self.next_undo,
        };
        self.undo = Some(ReadUndo {
            token: self.next_undo,
            articles: ids.into_iter().collect(),
        });
        marked
    }
}

/// The feeds a folder timeline covers (empty for other timelines).
fn feeds_in(snapshot: &Snapshot, timeline: &Timeline) -> std::collections::HashSet<i64> {
    let Timeline::Folder { folder_id } = timeline else {
        return Default::default();
    };
    let inside = folder_subtree(*folder_id, &snapshot.folders);
    snapshot
        .feeds
        .iter()
        .filter(|f| f.folder_id.is_some_and(|id| inside.contains(&id)))
        .map(|f| f.id)
        .collect()
}

fn read_opml(contents: &str) -> Result<opml::Document, AppError> {
    opml::parse(contents).map_err(|e| AppError::NotOpml {
        reason: match e {
            opml::OpmlError::TooLarge => format!("larger than {} bytes", opml::MAX_BYTES),
            opml::OpmlError::NotOpml { reason } => reason,
        },
    })
}

/// For each entry of a file: what importing it would do, and the address
/// it would be subscribed at.
fn plan_import(doc: &opml::Document, feeds: &[Feed]) -> Vec<(OpmlFeedStatus, Option<String>)> {
    let mut seen: Vec<String> = Vec::new();
    doc.entries
        .iter()
        .map(|entry| {
            let Ok(url) = normalise_url(&entry.url) else {
                return (OpmlFeedStatus::BadAddress, None);
            };
            if seen.iter().any(|s| same_address(s, &url)) {
                return (OpmlFeedStatus::ListedTwice, Some(url));
            }
            seen.push(url.clone());
            match feeds.iter().find(|f| same_address(&f.url, &url)) {
                Some(f) => (
                    OpmlFeedStatus::AlreadySubscribed { feed_id: f.id },
                    Some(url),
                ),
                None => (OpmlFeedStatus::New, Some(url)),
            }
        })
        .collect()
}

/// What Mark All as Read did.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct MarkedRead {
    /// How many articles it marked read.
    pub count: u32,
    /// Hands the change to `undo_mark_all_read`.
    pub undo_token: u64,
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
    /// Opens the core with `http` for its requests and `lookup` for the
    /// host names of full-article pages (test doubles in tests).
    fn open(
        storage_directory: String,
        http: Arc<dyn HttpClient>,
        lookup: full::Lookup,
    ) -> Result<Arc<Self>, AppError> {
        forge_runtime::logging::init();
        if storage_directory.is_empty() {
            return Err(AppError::InvalidInput {
                reason: "storage directory must not be empty".into(),
            });
        }
        let storage_directory = PathBuf::from(storage_directory);
        let directory = DataDirectory::open(&storage_directory)?;
        let store = Store::open(directory.path())?;
        let searcher = Arc::new(Searcher::open(&store)?);
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
            undo: None,
            next_undo: 0,
        };
        let rt = Runtime::with_directory("commonplace", directory, state);
        Ok(Arc::new(Self {
            rt,
            snapshot: published,
            http,
            storage_directory,
            searcher,
            pages: full::Policy::new(lookup),
            transport: std::sync::OnceLock::new(),
        }))
    }
}

/// Articles (newest first, as the snapshot holds them) in the order the
/// settings ask for.
fn ordered(
    articles: impl Iterator<Item = ArticleSummary>,
    settings: &AppSettings,
) -> Vec<ArticleSummary> {
    let mut articles: Vec<_> = articles.collect();
    if settings.oldest_first {
        articles.reverse();
    }
    articles
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

/// Whether two feed addresses name the same feed: `http` and `https`, the
/// host's case and a trailing `/` don't make a different feed.
pub(crate) fn same_address(a: &str, b: &str) -> bool {
    fn key(url: &str) -> String {
        let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        format!(
            "{}/{}",
            host.to_ascii_lowercase(),
            path.trim_end_matches('/')
        )
    }
    key(a) == key(b)
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

#[cfg(test)]
mod tests;
