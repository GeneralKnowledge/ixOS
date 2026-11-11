//! The core's behaviour, on Linux: subscribing, refreshing (with HTTP
//! caching), read and bookmark state, persistence, and errors.

use super::*;
use forge_app::{AppPhase, RuntimeEvent};
use forge_net::{Method, NetError, Response, StaticClient};
use forge_runtime::block_on;

const URL: &str = "https://fixtures.example/feed.xml";

fn rss(items: &[(&str, &str)]) -> Vec<u8> {
    let items: String = items
        .iter()
        .enumerate()
        .map(|(i, (guid, title))| {
            format!(
                "<item><title>{title}</title><guid>{guid}</guid><link>https://fixtures.example/{guid}</link>\
                 <pubDate>Mon, 09 Nov 2026 1{i}:00:00 GMT</pubDate>\
                 <description>&lt;p&gt;Body of {title}.&lt;/p&gt;</description></item>"
            )
        })
        .collect();
    format!(
        "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>Fixture Feed</title>\
         <link>https://fixtures.example/</link>{items}</channel></rss>"
    )
    .into_bytes()
}

fn ok(body: Vec<u8>, etag: Option<&str>) -> Result<Response, NetError> {
    let mut response = Response::new(200, body);
    if let Some(etag) = etag {
        response.headers.push(forge_net::Header {
            name: "ETag".into(),
            value: etag.into(),
        });
    }
    Ok(response)
}

fn open_with(dir: &std::path::Path, http: StaticClient) -> (Arc<AppCore>, Arc<StaticClient>) {
    let http = Arc::new(http);
    let core = AppCore::open(
        dir.to_string_lossy().into_owned(),
        Arc::clone(&http) as Arc<dyn HttpClient>,
    )
    .unwrap();
    (core, http)
}

fn subscribed(dir: &std::path::Path) -> (Arc<AppCore>, Feed) {
    let http = StaticClient::new().route(
        Method::Get,
        URL,
        ok(rss(&[("a", "Alpha"), ("b", "Beta")]), Some("\"v1\"")),
    );
    let (core, _) = open_with(dir, http);
    let feed = block_on(core.add_feed(URL.into(), None)).unwrap();
    (core, feed)
}

#[test]
fn subscribing_saves_the_feed_and_its_articles() {
    let dir = tempfile::tempdir().unwrap();
    let (core, feed) = subscribed(dir.path());
    assert_eq!(feed.title, "Fixture Feed");
    assert_eq!(feed.site_url.as_deref(), Some("https://fixtures.example/"));
    assert_eq!((feed.article_count, feed.unread_count), (2, 2));
    let timeline = core.timeline(Timeline::All);
    // Newest first.
    assert_eq!(
        timeline
            .iter()
            .map(|a| a.title.as_str())
            .collect::<Vec<_>>(),
        ["Beta", "Alpha"]
    );
    assert_eq!(timeline[0].summary, "Body of Beta.");
    assert_eq!(timeline[0].feed_title, "Fixture Feed");
    assert_eq!(
        core.counts(Timeline::Unread),
        TimelineCounts {
            total: 2,
            unread: 2
        }
    );
}

#[test]
fn addresses_are_normalised_and_checked() {
    assert_eq!(
        normalise_url(" example.com/feed ").unwrap(),
        "https://example.com/feed"
    );
    assert_eq!(
        normalise_url("feed://example.com/rss").unwrap(),
        "https://example.com/rss"
    );
    assert_eq!(
        normalise_url("http://192.168.1.169:8000/a.xml").unwrap(),
        "http://192.168.1.169:8000/a.xml"
    );
    for bad in ["", "two words", "ftp://example.com/feed", "https://"] {
        assert!(
            matches!(normalise_url(bad), Err(AppError::InvalidInput { .. })),
            "{bad}"
        );
    }
}

#[test]
fn a_feed_can_only_be_added_once_and_must_be_a_feed() {
    let dir = tempfile::tempdir().unwrap();
    let (core, feed) = subscribed(dir.path());
    assert_eq!(
        block_on(core.add_feed(URL.into(), None)),
        Err(AppError::AlreadySubscribed { feed_id: feed.id })
    );
    drop(core);

    let dir = tempfile::tempdir().unwrap();
    let http = StaticClient::new()
        .route(
            Method::Get,
            "https://fixtures.example/page",
            ok(b"<html><body>hi</body></html>".to_vec(), None),
        )
        .route(
            Method::Get,
            "https://fixtures.example/offline",
            Err(NetError::Offline {
                reason: "no route".into(),
            }),
        );
    let (core, _) = open_with(dir.path(), http);
    assert!(matches!(
        block_on(core.add_feed("https://fixtures.example/page".into(), None)),
        Err(AppError::NotAFeed { .. })
    ));
    assert_eq!(
        block_on(core.add_feed("https://fixtures.example/missing".into(), None)),
        Err(AppError::Http { status: 404 })
    );
    assert!(matches!(
        block_on(core.add_feed("https://fixtures.example/offline".into(), None)),
        Err(AppError::Offline { .. })
    ));
    // Nothing was saved, and nothing was announced.
    assert!(core.feeds().is_empty());
    assert_eq!(core.revision(), 0);
}

#[test]
fn refresh_adds_new_articles_and_keeps_read_and_bookmarks() {
    let dir = tempfile::tempdir().unwrap();
    let (core, feed) = subscribed(dir.path());
    let alpha = core.timeline(Timeline::All)[1].clone();
    block_on(core.set_bookmarked(alpha.id, true)).unwrap();
    block_on(core.set_read(alpha.id, true)).unwrap();
    drop(core);

    // The feed now has a third post, and Alpha's title changed.
    let http = StaticClient::new().route(
        Method::Get,
        URL,
        ok(
            rss(&[("a", "Alpha, edited"), ("b", "Beta"), ("c", "Gamma")]),
            Some("\"v2\""),
        ),
    );
    let (core, http) = open_with(dir.path(), http);
    let report = block_on(core.refresh(None)).unwrap();
    assert_eq!(
        report,
        RefreshReport {
            feeds: 1,
            unchanged: 0,
            new_articles: 1,
            failures: vec![]
        }
    );
    // The validators from the first fetch were sent.
    let sent = &http.received()[0];
    assert!(sent
        .headers
        .iter()
        .any(|h| h.name == "If-None-Match" && h.value == "\"v1\""));
    let alpha = core.article(alpha.id).unwrap();
    assert_eq!(alpha.title, "Alpha, edited");
    assert!(alpha.read && alpha.bookmarked);
    assert_eq!(core.timeline(Timeline::Bookmarked).len(), 1);
    assert_eq!(core.timeline(Timeline::Unread).len(), 2);
    assert_eq!(core.feeds()[0].id, feed.id);
    assert_eq!(core.feeds()[0].unread_count, 2);
}

#[test]
fn an_unchanged_or_failing_feed_is_reported_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = subscribed(dir.path());
    drop(core);

    let http = StaticClient::new().route(Method::Get, URL, Ok(Response::new(304, Vec::new())));
    let (core, _) = open_with(dir.path(), http);
    let report = block_on(core.refresh(None)).unwrap();
    assert_eq!((report.unchanged, report.new_articles), (1, 0));
    assert!(core.feeds()[0].last_error.is_none());
    drop(core);

    let http = StaticClient::new().route(Method::Get, URL, Ok(Response::new(500, Vec::new())));
    let (core, _) = open_with(dir.path(), http);
    let report = block_on(core.refresh(None)).unwrap();
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].error, AppError::Http { status: 500 });
    assert_eq!(report.failures[0].title, "Fixture Feed");
    assert_eq!(core.feeds()[0].last_error.as_deref(), Some("HTTP 500"));
    // The articles are still there.
    assert_eq!(core.timeline(Timeline::All).len(), 2);
}

#[test]
fn opening_an_article_marks_it_read_and_gives_its_text() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = subscribed(dir.path());
    let beta = core.timeline(Timeline::All)[0].clone();
    let events = core.events().unwrap();
    let text = block_on(core.open_article(beta.id)).unwrap();
    assert_eq!(text.title, "Beta");
    assert_eq!(text.paragraphs, vec!["Body of Beta."]);
    assert_eq!(text.url.as_deref(), Some("https://fixtures.example/b"));
    assert!(text.read);
    // Saved, then announced.
    assert!(matches!(
        block_on(events.next()),
        Some(RuntimeEvent::Changed { .. })
    ));
    assert!(core.article(beta.id).unwrap().read);
    assert_eq!(block_on(core.open_article(9999)), Err(AppError::NotFound));
}

#[test]
fn mark_all_read_by_feed_or_everything() {
    let dir = tempfile::tempdir().unwrap();
    let (core, feed) = subscribed(dir.path());
    assert_eq!(block_on(core.mark_all_read(Some(feed.id))).unwrap(), 2);
    assert_eq!(block_on(core.mark_all_read(None)).unwrap(), 0);
    assert!(core.timeline(Timeline::Unread).is_empty());
}

#[test]
fn feeds_can_be_renamed_and_removed_and_everything_persists() {
    let dir = tempfile::tempdir().unwrap();
    let (core, feed) = subscribed(dir.path());
    block_on(core.rename_feed(feed.id, "  My Feed ".into())).unwrap();
    assert_eq!(core.feeds()[0].title, "My Feed");
    assert_eq!(core.timeline(Timeline::All)[0].feed_title, "My Feed");
    let beta = core.timeline(Timeline::All)[0].id;
    block_on(core.set_bookmarked(beta, true)).unwrap();
    core.shutdown();
    drop(core);

    let (core, _) = open_with(dir.path(), StaticClient::new());
    assert_eq!(core.feeds()[0].title, "My Feed");
    assert!(core.article(beta).unwrap().bookmarked);
    // Blank goes back to the feed's own title.
    block_on(core.rename_feed(feed.id, " ".into())).unwrap();
    assert_eq!(core.feeds()[0].title, "Fixture Feed");

    block_on(core.remove_feed(feed.id)).unwrap();
    assert!(core.feeds().is_empty());
    assert!(core.timeline(Timeline::All).is_empty());
    assert_eq!(block_on(core.remove_feed(feed.id)), Err(AppError::NotFound));
    assert_eq!(
        block_on(core.rename_feed(feed.id, "x".into())),
        Err(AppError::NotFound)
    );
}

#[test]
fn one_core_per_data_directory() {
    let dir = tempfile::tempdir().unwrap();
    let (_core, _) = open_with(dir.path(), StaticClient::new());
    assert_eq!(
        AppCore::new(dir.path().to_string_lossy().into_owned()).err(),
        Some(AppError::InUse)
    );
    assert!(matches!(
        AppCore::new(String::new()),
        Err(AppError::InvalidInput { .. })
    ));
}

#[test]
fn the_background_interrupts_a_refresh_and_nothing_is_saved() {
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    let dir = tempfile::tempdir().unwrap();
    // A real client against a server that never answers.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/feed.xml", listener.local_addr().unwrap());
    let core = AppCore::new(dir.path().to_string_lossy().into_owned()).unwrap();
    core.set_phase(AppPhase::Active);
    let started = std::time::Instant::now();
    std::thread::spawn({
        let core = Arc::clone(&core);
        move || {
            std::thread::sleep(Duration::from_millis(150));
            core.set_phase(AppPhase::Background);
        }
    });
    assert_eq!(
        block_on(core.add_feed(url, None)),
        Err(AppError::Interrupted)
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(core.feeds().is_empty());
}

#[test]
fn shutdown_refuses_new_work() {
    let dir = tempfile::tempdir().unwrap();
    let (core, feed) = subscribed(dir.path());
    core.shutdown();
    assert_eq!(block_on(core.remove_feed(feed.id)), Err(AppError::ShutDown));
    assert_eq!(block_on(core.refresh(None)), Err(AppError::ShutDown));
}

#[test]
fn errors_have_framework_kinds() {
    use forge_app::ErrorKind;
    assert_eq!(
        app_error_kind(AppError::Interrupted),
        ErrorKind::Interrupted
    );
    assert_eq!(
        app_error_kind(AppError::NotAFeed {
            reason: String::new()
        }),
        ErrorKind::Invalid
    );
    assert_eq!(
        app_error_kind(AppError::Http { status: 500 }),
        ErrorKind::Unavailable
    );
}

#[test]
fn bookmarking_an_opened_article_keeps_it_read() {
    // The F2 flow on Cati: open, then bookmark; the article stays read.
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = subscribed(dir.path());
    let before = core.counts(Timeline::All).unread;
    let first = core.timeline(Timeline::All)[0].clone();
    block_on(core.open_article(first.id)).unwrap();
    block_on(core.set_bookmarked(first.id, true)).unwrap();
    assert_eq!(core.counts(Timeline::All).unread, before - 1);
    assert_eq!(core.feeds()[0].unread_count, before - 1);
    assert!(core.article(first.id).unwrap().read);
}

#[test]
fn background_refresh_runs_while_the_app_is_in_the_background() {
    use native::background::testing::FakeScheduler;
    use native::background::BackgroundService;

    let dir = tempfile::tempdir().unwrap();
    let (core, _feed) = subscribed(dir.path());
    drop(core);
    let http = StaticClient::new().route(
        Method::Get,
        URL,
        ok(rss(&[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")]), None),
    );
    let (core, _) = open_with(dir.path(), http);
    // In the background, on-screen work stops; the background refresh is
    // not on-screen work.
    core.set_phase(AppPhase::Background);

    let (service, fake) = BackgroundService::for_tests(&[BACKGROUND_REFRESH]);
    service.clone().install();
    core.register_background_refresh();
    BackgroundService::uninstall();
    assert_eq!(
        fake.submitted.lock().unwrap()[0].identifier,
        BACKGROUND_REFRESH
    );

    let (_run, results) = FakeScheduler::start(&service, BACKGROUND_REFRESH);
    for _ in 0..300 {
        if !results.lock().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(*results.lock().unwrap(), vec![true]);
    assert_eq!(core.timeline(Timeline::All).len(), 3);
}
