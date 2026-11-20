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
    // The same feed written another way is still the same feed.
    for again in [
        "http://fixtures.example/feed.xml",
        "https://FIXTURES.example/feed.xml/",
        "fixtures.example/feed.xml",
    ] {
        assert_eq!(
            block_on(core.add_feed(again.into(), None)),
            Err(AppError::AlreadySubscribed { feed_id: feed.id }),
            "{again}"
        );
    }
    assert!(!same_address(URL, "https://fixtures.example/other.xml"));
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
            failures: vec![],
            full_articles: 0,
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
    // The kind and status, for the screen to word; never the reason.
    assert_eq!(core.feeds()[0].last_error.as_deref(), Some("http-500"));
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
    assert_eq!(text.body.blocks.len(), 1);
    assert_eq!(text.body.blocks[0].text(), "Body of Beta.");
    assert_eq!(text.body.title, "Beta");
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
    let marked = block_on(core.mark_all_read(Some(feed.id))).unwrap();
    assert_eq!(marked.count, 2);
    assert_eq!(block_on(core.mark_all_read(None)).unwrap().count, 0);
    assert!(core.timeline(Timeline::Unread).is_empty());
}

#[test]
fn mark_all_read_can_be_undone() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _feed) = subscribed(dir.path());
    let before = core.timeline(Timeline::Unread);
    let marked = block_on(core.mark_all_read(None)).unwrap();
    assert_eq!(marked.count as usize, before.len());
    assert_eq!(
        block_on(core.undo_mark_all_read(marked.undo_token)).unwrap(),
        marked.count
    );
    assert_eq!(core.timeline(Timeline::Unread).len(), before.len());
    // Undo works once.
    assert_eq!(
        block_on(core.undo_mark_all_read(marked.undo_token)).unwrap(),
        0
    );
}

#[test]
fn undo_keeps_what_the_person_did_in_between() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _feed) = subscribed(dir.path());
    let unread = core.timeline(Timeline::Unread);
    assert!(unread.len() >= 2);
    let marked = block_on(core.mark_all_read(None)).unwrap();
    // The person opens one article, and marks another unread and read again.
    block_on(core.open_article(unread[0].id)).unwrap();
    block_on(core.set_read(unread[1].id, false)).unwrap();
    block_on(core.set_read(unread[1].id, true)).unwrap();
    let n = block_on(core.undo_mark_all_read(marked.undo_token)).unwrap();
    assert_eq!(n as usize, unread.len() - 2);
    let still_unread: Vec<i64> = core
        .timeline(Timeline::Unread)
        .iter()
        .map(|a| a.id)
        .collect();
    assert!(
        !still_unread.contains(&unread[0].id),
        "an opened article stays read"
    );
    assert!(
        !still_unread.contains(&unread[1].id),
        "an article marked by hand keeps its mark"
    );
}

#[test]
fn only_the_latest_mark_all_read_can_be_undone() {
    let dir = tempfile::tempdir().unwrap();
    let (core, feed) = subscribed(dir.path());
    let first = block_on(core.mark_all_read(Some(feed.id))).unwrap();
    let second = block_on(core.mark_all_read(None)).unwrap();
    assert_eq!(
        block_on(core.undo_mark_all_read(first.undo_token)).unwrap(),
        0
    );
    assert_eq!(
        block_on(core.undo_mark_all_read(second.undo_token)).unwrap(),
        second.count
    );
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
    assert_eq!(
        app_error_kind(AppError::Offline {
            reason: String::new()
        }),
        ErrorKind::Network
    );
    assert_eq!(app_error_kind(AppError::Timeout), ErrorKind::Network);
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

#[test]
fn new_article_alerts_are_off_until_turned_on_and_say_what_arrived() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = subscribed(dir.path());
    assert!(!core.new_article_alerts());
    // No native services on Linux, so no permission prompt: it just turns on.
    assert!(block_on(core.set_new_article_alerts(true)).unwrap());
    assert!(core.new_article_alerts());
    assert!(!block_on(core.set_new_article_alerts(false)).unwrap());
    assert!(!core.new_article_alerts());
    // Turning off twice is fine.
    assert!(!block_on(core.set_new_article_alerts(false)).unwrap());

    let one = new_articles_notification(1, &["Alpha".into()]);
    assert_eq!(one.title, "1 new article");
    assert_eq!(one.category.as_deref(), Some(NEW_ARTICLES));
    let three = new_articles_notification(5, &["A".into(), "B".into(), "C".into()]);
    assert_eq!(three.title, "5 new articles");
    assert_eq!(three.body, "A\nB\nC");
}

#[test]
fn old_read_articles_are_forgotten_once_the_feed_drops_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let day = 24 * 60 * 60 * 1000;
    let first = feed::parse(&rss(&[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")])).unwrap();
    let (id, _) = store.add_feed(URL, &first, None, None, 0).unwrap();
    let ids = |store: &Store| -> Vec<String> {
        let mut titles: Vec<String> = store
            .snapshot()
            .unwrap()
            .articles
            .into_iter()
            .map(|a| a.title)
            .collect();
        titles.sort();
        titles
    };
    let article = |store: &Store, title: &str| {
        store
            .snapshot()
            .unwrap()
            .articles
            .into_iter()
            .find(|a| a.title == title)
            .unwrap()
            .id
    };
    // Alpha read, Beta read and bookmarked, Gamma unread.
    store
        .set_flag(article(&store, "Alpha"), Flag::Read, true)
        .unwrap();
    let beta = article(&store, "Beta");
    store.set_flag(beta, Flag::Read, true).unwrap();
    store.set_flag(beta, Flag::Bookmarked, true).unwrap();

    // A month and more later the feed lists only a new post.
    let later = feed::parse(&rss(&[("d", "Delta")])).unwrap();
    let refresh = |feed: feed::ParsedFeed| Fetched {
        feed_id: id,
        result: Ok(FetchResult::Parsed {
            feed,
            etag: None,
            last_modified: None,
        }),
    };
    store.apply(&[refresh(later)], 31 * day).unwrap();
    // Read Alpha is gone; bookmarked Beta and unread Gamma stay.
    assert_eq!(ids(&store), ["Beta", "Delta", "Gamma"]);

    // A read article the feed still lists is kept, or it would come back.
    let delta = article(&store, "Delta");
    store.set_flag(delta, Flag::Read, true).unwrap();
    let still = feed::parse(&rss(&[("d", "Delta")])).unwrap();
    store.apply(&[refresh(still)], 90 * day).unwrap();
    assert_eq!(ids(&store), ["Beta", "Delta", "Gamma"]);
}

#[test]
fn an_article_body_is_a_reader_section_with_links_and_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let html = "&lt;h2&gt;Intro&lt;/h2&gt;&lt;p&gt;See &lt;a href=&quot;/more&quot;&gt;more&lt;/a&gt;.&lt;/p&gt;\
                &lt;img src=&quot;pic.jpg&quot; alt=&quot;A pic&quot;&gt;&lt;blockquote&gt;Quoted&lt;/blockquote&gt;";
    let feed = format!(
        "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>F</title>\
         <item><title>Rich</title><guid>r</guid><link>https://fixtures.example/posts/r</link>\
         <description>{html}</description></item></channel></rss>"
    );
    let http = StaticClient::new().route(Method::Get, URL, ok(feed.into_bytes(), None));
    let (core, _) = open_with(dir.path(), http);
    block_on(core.add_feed(URL.into(), None)).unwrap();
    let id = core.timeline(Timeline::All)[0].id;
    let body = block_on(core.open_article(id)).unwrap().body;
    let kinds: Vec<_> = body.blocks.iter().map(|b| b.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            ReaderBlockKind::Heading { level: 2 },
            ReaderBlockKind::Paragraph,
            ReaderBlockKind::Image {
                source: "https://fixtures.example/posts/pic.jpg".into(),
                alt: "A pic".into()
            },
            ReaderBlockKind::Quote,
        ]
    );
    // Relative links resolve against the article's own address.
    assert_eq!(
        body.blocks[1].spans[1].link.as_deref(),
        Some("https://fixtures.example/more")
    );
    // Ids are stable: the same article gives the same ids.
    assert_eq!(block_on(core.open_article(id)).unwrap().body, body);
}

#[test]
fn search_finds_titles_and_text_by_every_word() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = subscribed(dir.path());
    let titles = |query: &str| -> Vec<String> {
        block_on(core.search(query.into()))
            .unwrap()
            .into_iter()
            .map(|a| a.title)
            .collect()
    };
    assert_eq!(titles("beta"), ["Beta"]);
    // Words are prefixes; every word must match; case doesn't matter.
    assert_eq!(titles("BOD"), ["Beta", "Alpha"]);
    assert_eq!(titles("body alph"), ["Alpha"]);
    assert!(titles("body gamma").is_empty());
    // Query syntax is text, not an error.
    assert!(titles("\"AND -( *").is_empty());
    assert!(titles("   ").is_empty());
    // A search is a read: no revision, no change event.
    let revision = core.revision();
    titles("beta");
    assert_eq!(core.revision(), revision);

    // Refreshed text is searchable; unsubscribed articles are not.
    drop(core);
    let http = StaticClient::new().route(
        Method::Get,
        URL,
        ok(rss(&[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")]), None),
    );
    let (core, _) = open_with(dir.path(), http);
    block_on(core.refresh(None)).unwrap();
    let found = block_on(core.search("gamma".into())).unwrap();
    assert_eq!(found.len(), 1);
    let feed = core.feeds()[0].id;
    block_on(core.remove_feed(feed)).unwrap();
    assert!(block_on(core.search("body".into())).unwrap().is_empty());
}

#[test]
fn a_library_from_before_search_is_indexed_when_it_opens() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = subscribed(dir.path());
    core.shutdown();
    drop(core);
    // As if written by slice 1: the index is empty.
    let db = forge_storage::rusqlite::Connection::open(dir.path().join("library.sqlite")).unwrap();
    db.execute("DELETE FROM article_search", []).unwrap();
    drop(db);
    let (core, _) = open_with(dir.path(), StaticClient::new());
    assert_eq!(block_on(core.search("alpha".into())).unwrap().len(), 1);
}

#[test]
fn settings_are_kept_and_shape_the_timelines() {
    let dir = tempfile::tempdir().unwrap();
    let (core, feed) = subscribed(dir.path());
    assert_eq!(core.settings(), AppSettings::default());
    let beta = core.timeline(Timeline::All)[0].clone();
    block_on(core.set_read(beta.id, true)).unwrap();

    let settings = AppSettings {
        oldest_first: true,
        hide_read: true,
        show_summaries: false,
        open_links_in_app: false,
    };
    block_on(core.set_settings(settings.clone())).unwrap();
    let titles =
        |t: Timeline| -> Vec<String> { core.timeline(t).into_iter().map(|a| a.title).collect() };
    // Beta is read, so hidden from All and the feed, but counted.
    assert_eq!(titles(Timeline::All), ["Alpha"]);
    assert_eq!(titles(Timeline::Feed { feed_id: feed.id }), ["Alpha"]);
    assert_eq!(core.counts(Timeline::All).total, 2);
    block_on(core.set_bookmarked(beta.id, true)).unwrap();
    assert_eq!(titles(Timeline::Bookmarked), ["Beta"]);

    block_on(core.set_settings(AppSettings {
        hide_read: false,
        ..settings.clone()
    }))
    .unwrap();
    // Oldest first.
    assert_eq!(titles(Timeline::All), ["Alpha", "Beta"]);

    core.shutdown();
    drop(core);
    let (core, _) = open_with(dir.path(), StaticClient::new());
    assert_eq!(
        core.settings(),
        AppSettings {
            hide_read: false,
            ..settings
        }
    );
}

#[test]
fn the_suggested_feeds_catalogue_is_well_formed() {
    let categories = suggested::suggestions(|_| false);
    assert!(categories.len() >= 4, "a handful of categories");
    let mut ids = std::collections::HashSet::new();
    let mut urls: Vec<String> = Vec::new();
    for category in &categories {
        assert!(!category.name.trim().is_empty());
        assert!(!category.feeds.is_empty(), "{} is empty", category.name);
        for feed in &category.feeds {
            assert!(
                !feed.id.is_empty()
                    && feed
                        .id
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{}: an id is a lower-case slug",
                feed.id
            );
            assert!(ids.insert(feed.id.clone()), "{} is listed twice", feed.id);
            assert!(!feed.title.trim().is_empty() && !feed.about.trim().is_empty());
            assert!(feed.url.starts_with("https://"), "{}: https only", feed.url);
            assert_eq!(normalise_url(&feed.url).unwrap(), feed.url);
            assert!(
                !urls.iter().any(|u| same_address(u, &feed.url)),
                "{} is listed twice",
                feed.url
            );
            urls.push(feed.url.clone());
            assert!(!feed.subscribed);
        }
    }
}

#[test]
fn suggestions_are_offered_not_subscribed_and_show_what_is_subscribed() {
    let dir = tempfile::tempdir().unwrap();
    let first = suggested::suggestions(|_| false).remove(0).feeds.remove(0);
    let http = StaticClient::new().route(Method::Get, &first.url, ok(rss(&[("a", "Alpha")]), None));
    let (core, _) = open_with(dir.path(), http);
    // Listing suggestions subscribes to nothing.
    assert!(core
        .suggested_feeds()
        .iter()
        .flat_map(|c| &c.feeds)
        .all(|f| !f.subscribed));
    assert_eq!(core.feeds(), vec![]);
    // Picking one goes through add_feed, and then it shows as subscribed.
    block_on(core.add_feed(first.url.clone(), None)).unwrap();
    let marked: Vec<_> = core
        .suggested_feeds()
        .into_iter()
        .flat_map(|c| c.feeds)
        .filter(|f| f.subscribed)
        .map(|f| f.id)
        .collect();
    assert_eq!(marked, vec![first.id]);
}

/// Every suggested feed answers with a feed Commonplace can read. Needs the
/// internet, so it only runs when asked:
/// `cargo test the_suggested_feeds_answer -- --ignored`.
#[test]
#[ignore = "fetches every suggested feed over the internet"]
fn the_suggested_feeds_answer_with_feeds() {
    let http = UreqClient::new();
    let mut failures = Vec::new();
    for feed in suggested::suggestions(|_| false)
        .into_iter()
        .flat_map(|c| c.feeds)
    {
        match fetch(&http, &feed.url, None, None, &CancelHandle::new()) {
            Ok(FetchResult::Parsed { feed: parsed, .. }) => {
                println!("ok    {:<20} {} articles", feed.id, parsed.articles.len());
                if parsed.articles.is_empty() {
                    failures.push(format!("{}: no articles", feed.id));
                }
            }
            Ok(FetchResult::NotModified) => failures.push(format!("{}: 304", feed.id)),
            Err(e) => {
                println!("FAIL  {:<20} {e:?}", feed.id);
                failures.push(format!("{}: {e:?}", feed.id));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

mod back_online {
    use super::super::native::connectivity::{
        ConnectivityService, ConnectivityStatus, Reachability,
    };
    use super::super::{wait_until_back_online, AppError};
    use forge_net::CancelHandle;
    use std::time::Duration;

    fn reachability(reachability: Reachability) -> ConnectivityStatus {
        ConnectivityStatus {
            reachability,
            ..ConnectivityStatus::unknown()
        }
    }

    #[test]
    fn returns_when_the_network_comes_back_after_being_offline() {
        let (service, fake) = ConnectivityService::for_tests();
        fake.set(reachability(Reachability::Online));
        let cancel = CancelHandle::new();
        let waiting = {
            let cancel = cancel.clone();
            std::thread::spawn(move || wait_until_back_online(&service, &cancel))
        };
        while !fake.is_started() {
            std::thread::sleep(Duration::from_millis(5));
        }
        // Online on its own doesn't count: the network must come back.
        fake.set(reachability(Reachability::Online));
        std::thread::sleep(Duration::from_millis(50));
        assert!(!waiting.is_finished());
        fake.set(reachability(Reachability::Offline));
        fake.set(reachability(Reachability::Online));
        assert!(matches!(waiting.join().unwrap(), Ok(true)));
        assert!(!fake.is_started(), "the watch ends with the wait");
    }

    #[test]
    fn cancelling_stops_the_wait() {
        let (service, fake) = ConnectivityService::for_tests();
        fake.set(reachability(Reachability::Offline));
        let cancel = CancelHandle::new();
        let waiting = {
            let cancel = cancel.clone();
            std::thread::spawn(move || wait_until_back_online(&service, &cancel))
        };
        cancel.cancel();
        assert!(matches!(waiting.join().unwrap(), Err(AppError::Cancelled)));
    }
}

#[test]
fn waiting_for_the_network_without_the_service_returns_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = open_with(dir.path(), StaticClient::new());
    assert!(!block_on(core.wait_until_back_online(None)).unwrap());
}

// ---- Folders and OPML (C3) ----

const FEEDER_STYLE: &str = include_str!("../fixtures/feeder-style.opml");

fn empty_core(dir: &std::path::Path) -> Arc<AppCore> {
    open_with(dir, StaticClient::new()).0
}

fn import_all(core: &AppCore, text: &str) -> OpmlImport {
    let preview = core.preview_opml(text.into()).unwrap();
    let all = preview.feeds.iter().map(|f| f.index).collect();
    block_on(core.import_opml(text.into(), all)).unwrap()
}

fn folder_named(core: &AppCore, name: &str) -> Folder {
    core.folders().into_iter().find(|f| f.name == name).unwrap()
}

fn feed_titled(core: &AppCore, title: &str) -> Feed {
    core.feeds().into_iter().find(|f| f.title == title).unwrap()
}

#[test]
fn an_opml_file_is_read_with_folders_options_and_filters() {
    let doc = opml::parse(FEEDER_STYLE).unwrap();
    let titles: Vec<_> = doc
        .entries
        .iter()
        .map(|e| e.title.clone().unwrap())
        .collect();
    assert_eq!(
        titles,
        [
            "Loose Feed",
            "World & Politics",
            "Gadgets",
            "Listed Twice",
            "Not a Web Address",
            "Weird Notify"
        ]
    );
    let world = &doc.entries[1];
    assert_eq!(world.folder, ["News"]);
    assert_eq!(
        world.options,
        opml::FeedOptions {
            notify: Some(true),
            full_text: Some(true)
        }
    );
    // Feeder's other attributes are kept for export and named as not used.
    assert_eq!(world.not_used, ["openArticlesWith", "alternateId"]);
    assert_eq!(world.extras[0].namespace, opml::FEEDER_NS);
    assert_eq!(world.extras[0].value, "reader");
    assert_eq!(doc.entries[2].folder, ["News", "Tech"]);
    assert_eq!(doc.entries[2].options.notify, Some(false));
    // A value it can't read is named, not guessed.
    assert_eq!(doc.entries[5].options.notify, None);
    assert_eq!(doc.entries[5].not_used, ["notify"]);
    // Patterns are trimmed text; the blank one is refused.
    assert_eq!(doc.filters, ["giveaway", "sponsored"]);
    assert_eq!(doc.rejected_filters, 1);
    assert_eq!(doc.settings_not_used, ["pref_article_style", "pref_theme"]);
}

#[test]
fn a_file_that_isnt_opml_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let core = empty_core(dir.path());
    for text in [
        "",
        "<html><body>hi</body></html>",
        "<opml><body><outline",
        "not xml at all",
    ] {
        assert!(
            matches!(
                core.preview_opml(text.into()),
                Err(AppError::NotOpml { .. })
            ),
            "{text:?}"
        );
    }
    let huge = format!("<opml><body>{}</body></opml>", " ".repeat(opml::MAX_BYTES));
    assert!(matches!(
        core.preview_opml(huge),
        Err(AppError::NotOpml { .. })
    ));
    assert_eq!(
        app_error_kind(AppError::NotOpml {
            reason: String::new()
        }),
        forge_app::ErrorKind::Invalid
    );
}

#[test]
fn an_opml_file_cant_smuggle_in_entities_or_control_characters() {
    let text = "<?xml version=\"1.0\"?><!DOCTYPE opml [<!ENTITY x \"EXPANDED\">]>\
        <opml version=\"2.0\" xmlns:feeder=\"https://nononsenseapps.com/feeder\"><head>\
        <feeder:blocked pattern=\"bad&#7;pattern\"/></head><body>\
        <outline text=\"&x;\" xmlUrl=\"https://a.example/feed\"/></body></opml>";
    // An entity beyond XML's own is an error, never expanded.
    match opml::parse(text) {
        Err(_) => {}
        Ok(doc) => assert!(doc.entries.iter().all(|e| !e
            .title
            .clone()
            .unwrap_or_default()
            .contains("EXPANDED"))),
    }
    let text = "<opml version=\"2.0\" xmlns:feeder=\"https://nononsenseapps.com/feeder\"><head>\
        <feeder:blocked pattern=\"bad&#7;pattern\"/></head><body>\
        <outline text=\"Line&#10;Break\" xmlUrl=\"https://a.example/feed\"/></body></opml>";
    let doc = opml::parse(text).unwrap();
    assert_eq!(doc.filters, Vec::<String>::new());
    assert_eq!(doc.rejected_filters, 1);
    assert_eq!(doc.entries[0].title.as_deref(), Some("Line Break"));
}

#[test]
fn the_preview_says_what_importing_would_do_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = open_with(
        dir.path(),
        StaticClient::new().route(
            Method::Get,
            "https://loose.example/feed.xml",
            ok(rss(&[("a", "Alpha")]), None),
        ),
    );
    let loose = block_on(core.add_feed("https://loose.example/feed.xml".into(), None)).unwrap();
    let preview = core.preview_opml(FEEDER_STYLE.into()).unwrap();
    let statuses: Vec<_> = preview.feeds.iter().map(|f| f.status.clone()).collect();
    assert_eq!(
        statuses,
        [
            OpmlFeedStatus::AlreadySubscribed { feed_id: loose.id },
            OpmlFeedStatus::New,
            OpmlFeedStatus::New,
            OpmlFeedStatus::ListedTwice,
            OpmlFeedStatus::BadAddress,
            OpmlFeedStatus::New,
        ]
    );
    // feed:// means https://.
    assert_eq!(preview.feeds[2].url, "https://gadgets.example/atom");
    assert_eq!(
        preview.folders,
        ["News", "News / Tech", "Podcasts and Odd Things"]
    );
    assert_eq!(preview.filters, 2);
    assert_eq!(preview.rejected_filters, 1);
    assert_eq!(core.feeds().len(), 1);
    assert!(core.folders().is_empty());
}

#[test]
fn importing_adds_the_picked_feeds_in_their_folders() {
    let dir = tempfile::tempdir().unwrap();
    let core = empty_core(dir.path());
    let preview = core.preview_opml(FEEDER_STYLE.into()).unwrap();
    // Only World and Gadgets.
    let done = block_on(core.import_opml(FEEDER_STYLE.into(), vec![1, 2])).unwrap();
    assert_eq!(done.added, 2);
    assert_eq!(done.folders_created, 2);
    assert_eq!(done.filters_added, 2);
    assert_eq!(preview.feeds.len(), 6);
    let world = feed_titled(&core, "World & Politics");
    let gadgets = feed_titled(&core, "Gadgets");
    assert_eq!(world.folder_id, Some(folder_named(&core, "News").id));
    assert_eq!(gadgets.folder_id, Some(folder_named(&core, "Tech").id));
    assert!(world.notify && world.full_text);
    assert!(!gadgets.notify && !gadgets.full_text);
    let tech = folder_named(&core, "Tech");
    assert_eq!(tech.parent_id, Some(folder_named(&core, "News").id));
    assert_eq!(tech.depth, 1);
    assert_eq!(folder_named(&core, "News").feed_count, 2);
    // Skipped entries are counted, not added.
    let skipped = block_on(core.import_opml(FEEDER_STYLE.into(), vec![3, 4])).unwrap();
    assert_eq!(skipped.skipped, 1);
    assert_eq!(skipped.already_subscribed, 1);
    assert_eq!(skipped.added, 0);
}

#[test]
fn importing_twice_adds_nothing_and_changes_nothing_the_library_has() {
    let dir = tempfile::tempdir().unwrap();
    let core = empty_core(dir.path());
    let first = import_all(&core, FEEDER_STYLE);
    assert_eq!(first.added, 4);
    assert_eq!(first.skipped, 2);
    // The person changes things after the import.
    let world = feed_titled(&core, "World & Politics");
    block_on(core.rename_feed(world.id, "My World".into())).unwrap();
    block_on(core.move_feed(world.id, None)).unwrap();
    block_on(core.set_feed_notify(world.id, false)).unwrap();
    let before = core.feeds();
    let names = |core: &AppCore| {
        core.folders()
            .into_iter()
            .map(|f| (f.id, f.parent_id, f.name))
            .collect::<Vec<_>>()
    };
    let folders_before = names(&core);
    let second = import_all(&core, FEEDER_STYLE);
    assert_eq!(second.added, 0);
    assert_eq!(second.already_subscribed, 4);
    assert_eq!(second.folders_created, 0);
    assert_eq!(second.filters_added, 0);
    // World keeps the person's name, place and option.
    let world = feed_titled(&core, "My World");
    assert_eq!(world.folder_id, None);
    assert!(!world.notify);
    assert_eq!(core.feeds(), before);
    assert_eq!(core.feeds().len(), before.len());
    assert_eq!(names(&core), folders_before);
}

#[test]
fn an_export_imports_again_with_its_folders_settings_and_kept_attributes() {
    let dir = tempfile::tempdir().unwrap();
    let core = empty_core(dir.path());
    import_all(&core, FEEDER_STYLE);
    let exported = block_on(core.export_opml()).unwrap();
    // Feeder's attribute Commonplace doesn't use goes back out as it came.
    assert!(
        exported.contains("feeder:openArticlesWith=\"reader\""),
        "{exported}"
    );
    assert!(exported.contains("World &amp; Politics"));
    // Into a fresh library, and out again: the same file.
    let dir2 = tempfile::tempdir().unwrap();
    let again = empty_core(dir2.path());
    let done = import_all(&again, &exported);
    assert_eq!(done.added, 4);
    assert_eq!(done.skipped, 0);
    let shape = |core: &AppCore| {
        let folders = core.folders();
        let mut feeds: Vec<_> = core
            .feeds()
            .into_iter()
            .map(|f| {
                let folder = f
                    .folder_id
                    .and_then(|id| folders.iter().find(|x| x.id == id))
                    .map(|x| x.name.clone());
                (f.title, f.url, folder, f.notify, f.full_text)
            })
            .collect();
        feeds.sort();
        let folders: Vec<_> = folders.into_iter().map(|f| (f.name, f.depth)).collect();
        (feeds, folders)
    };
    assert_eq!(shape(&core), shape(&again));
    assert_eq!(block_on(again.export_opml()).unwrap(), exported);
}

#[test]
fn folders_are_made_renamed_and_deleted_without_losing_feeds() {
    let dir = tempfile::tempdir().unwrap();
    let core = empty_core(dir.path());
    import_all(&core, FEEDER_STYLE);
    let news = folder_named(&core, "News");
    // Same name, any case: the existing folder.
    assert_eq!(
        block_on(core.create_folder("  news ".into(), None))
            .unwrap()
            .id,
        news.id
    );
    let other = block_on(core.create_folder("Other".into(), None)).unwrap();
    assert!(matches!(
        block_on(core.create_folder("   ".into(), None)),
        Err(AppError::InvalidInput { .. })
    ));
    assert!(matches!(
        block_on(core.rename_folder(other.id, "NEWS".into())),
        Err(AppError::InvalidInput { .. })
    ));
    block_on(core.rename_folder(other.id, "Reading".into())).unwrap();
    // Deleting News moves World to the top and Tech (with Gadgets) up.
    block_on(core.delete_folder(news.id)).unwrap();
    assert_eq!(core.feeds().len(), 4);
    assert_eq!(feed_titled(&core, "World & Politics").folder_id, None);
    let tech = folder_named(&core, "Tech");
    assert_eq!((tech.parent_id, tech.depth), (None, 0));
    assert_eq!(feed_titled(&core, "Gadgets").folder_id, Some(tech.id));
    assert!(matches!(
        block_on(core.delete_folder(news.id)),
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        block_on(core.move_feed(999, None)),
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        block_on(core.move_feed(feed_titled(&core, "Gadgets").id, Some(news.id))),
        Err(AppError::NotFound)
    ));
}

#[test]
fn a_folder_timeline_covers_the_folders_inside_it() {
    let dir = tempfile::tempdir().unwrap();
    let http = StaticClient::new()
        .route(
            Method::Get,
            "https://world.example/rss",
            ok(rss(&[("w", "World story")]), None),
        )
        .route(
            Method::Get,
            "https://gadgets.example/atom",
            ok(rss(&[("g", "Gadget story")]), None),
        )
        .route(
            Method::Get,
            "https://loose.example/feed.xml",
            ok(rss(&[("l", "Loose story")]), None),
        )
        .route(
            Method::Get,
            "https://weird.example/feed",
            ok(rss(&[("x", "Weird story")]), None),
        );
    let (core, _) = open_with(dir.path(), http);
    import_all(&core, FEEDER_STYLE);
    let (report, notifying) = block_on(core.refresh_in(core.rt.lifetime(None))).unwrap();
    assert_eq!(report.new_articles, 4);
    // Gadgets said notify="false": its article doesn't count for the alert.
    assert_eq!(notifying, 3);
    let news = folder_named(&core, "News");
    let titles = |t: Timeline| {
        core.timeline(t)
            .into_iter()
            .map(|a| a.title)
            .collect::<Vec<_>>()
    };
    let mut in_news = titles(Timeline::Folder { folder_id: news.id });
    in_news.sort();
    assert_eq!(in_news, ["Gadget story", "World story"]);
    assert_eq!(
        core.counts(Timeline::Folder { folder_id: news.id }).unread,
        2
    );
    assert_eq!(folder_named(&core, "News").unread_count, 2);
    // Mark the folder read: only its articles, and Undo brings them back.
    let marked = block_on(core.mark_folder_read(news.id)).unwrap();
    assert_eq!(marked.count, 2);
    assert_eq!(core.counts(Timeline::Unread).unread, 2);
    block_on(core.undo_mark_all_read(marked.undo_token)).unwrap();
    assert_eq!(core.counts(Timeline::Unread).unread, 4);
}

#[test]
fn folders_and_options_survive_a_relaunch() {
    let dir = tempfile::tempdir().unwrap();
    {
        let core = empty_core(dir.path());
        import_all(&core, FEEDER_STYLE);
        core.shutdown();
    }
    let core = empty_core(dir.path());
    assert_eq!(core.folders().len(), 3);
    assert!(!feed_titled(&core, "Gadgets").notify);
    assert!(block_on(core.export_opml())
        .unwrap()
        .contains("commonplace:filter pattern=\"sponsored\""));
}

#[test]
fn a_library_from_before_folders_opens_with_its_feeds_at_the_top() {
    let dir = tempfile::tempdir().unwrap();
    {
        // The file as slice 2 left it: schema version 2, one feed.
        let c =
            forge_storage::rusqlite::Connection::open(dir.path().join("library.sqlite")).unwrap();
        for step in &store::MIGRATIONS[..2] {
            c.execute_batch(step).unwrap();
        }
        c.execute(
            "INSERT INTO feeds (url, title, added_at_ms) VALUES ('https://old.example/feed', 'Old', 1)",
            [],
        )
        .unwrap();
        c.pragma_update(None, "user_version", 2).unwrap();
    }
    let core = empty_core(dir.path());
    let old = feed_titled(&core, "Old");
    assert_eq!(old.folder_id, None);
    assert!(old.notify && !old.full_text);
    assert!(core.folders().is_empty());
    assert!(block_on(core.export_opml())
        .unwrap()
        .contains("xmlUrl=\"https://old.example/feed\""));
}

#[test]
fn the_phone_checks_opml_fixture_previews_as_the_steps_say() {
    let text = include_str!("../../docs/benchmark/fixtures/feeder-folders.opml")
        .replace("HOST", "192.168.1.10:8765");
    let dir = tempfile::tempdir().unwrap();
    let core = empty_core(dir.path());
    let preview = core.preview_opml(text).unwrap();
    let statuses: Vec<_> = preview.feeds.iter().map(|f| f.status.clone()).collect();
    assert_eq!(
        statuses,
        [
            OpmlFeedStatus::New,
            OpmlFeedStatus::New,
            OpmlFeedStatus::New,
            OpmlFeedStatus::ListedTwice,
            OpmlFeedStatus::BadAddress,
        ]
    );
    assert_eq!(preview.folders, ["News", "News / Tech", "Odd Ones"]);
    assert_eq!(preview.filters, 1);
    assert_eq!(preview.settings_not_used, ["pref_theme"]);
}

// ---- Full articles (C4) ----

/// A web page like a blog's: navigation, a sidebar and a footer around one
/// article of several paragraphs.
fn article_page(title: &str, word: &str) -> Vec<u8> {
    let paragraph = format!(
        "<p>The {word} story goes on for a while, with enough sentences to read like a real article. \
         It has commas, full stops and several clauses, which is what a readability score looks for. \
         A second sentence adds more of the same, so the paragraph is long enough to count.</p>"
    );
    format!(
        "<!DOCTYPE html><html><head><title>{title} | Example Blog</title></head><body>\
         <nav><a href=\"/\">Home</a> <a href=\"/about\">About</a> <a href=\"/shop\">Shop</a></nav>\
         <aside class=\"sidebar\"><p>Subscribe to our newsletter!</p><a href=\"/ad\">Buy now</a></aside>\
         <article><h1>{title}</h1>{p}{p}{p}<p><img src=\"/pics/{word}.jpg\" alt=\"A picture\"></p>{p}</article>\
         <footer>Copyright Example Blog. <a href=\"/privacy\">Privacy</a></footer></body></html>",
        p = paragraph
    )
    .into_bytes()
}

fn page(body: Vec<u8>) -> Result<Response, NetError> {
    let mut response = Response::new(200, body);
    response.headers.push(forge_net::Header {
        name: "Content-Type".into(),
        value: "text/html; charset=utf-8".into(),
    });
    Ok(response)
}

#[test]
fn the_readable_part_of_a_page_is_kept_and_the_rest_left_out() {
    let html = full::extract(
        &String::from_utf8(article_page("Moss", "walrus")).unwrap(),
        "https://fixtures.example/a",
    )
    .unwrap();
    assert!(html.contains("walrus story goes on"), "{html}");
    assert!(!html.contains("newsletter"), "{html}");
    assert!(!html.contains("Privacy"), "{html}");
    // Relative pictures resolve against the page.
    assert!(
        html.contains("https://fixtures.example/pics/walrus.jpg"),
        "{html}"
    );
    // ForgeReader lays it out as blocks, picture included.
    let blocks = forge_reader::blocks_from_html(
        &html,
        &mut forge_reader::WebResources::new(Some("https://fixtures.example/a")),
    );
    assert!(blocks.len() >= 4, "{blocks:?}");
    assert!(matches!(
        full::extract(
            "<html><body><nav>Home</nav></body></html>",
            "https://fixtures.example/a"
        ),
        Err(AppError::NoReadableText { .. })
    ));
}

fn full_text_library(dir: &std::path::Path, http: StaticClient) -> (Arc<AppCore>, Feed) {
    let http = http.route(
        Method::Get,
        URL,
        ok(rss(&[("a", "Alpha"), ("b", "Beta")]), None),
    );
    let (core, _) = open_with(dir, http);
    let feed = block_on(core.add_feed(URL.into(), None)).unwrap();
    (core, feed)
}

#[test]
fn a_feed_that_wants_full_articles_gets_them_with_the_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let http = StaticClient::new()
        .route(
            Method::Get,
            "https://fixtures.example/a",
            page(article_page("Alpha", "walrus")),
        )
        .route(
            Method::Get,
            "https://fixtures.example/b",
            Ok(Response::new(404, Vec::new())),
        );
    let (core, feed) = full_text_library(dir.path(), http);
    // Off by default: a refresh fetches no pages.
    let report = block_on(core.refresh(None)).unwrap();
    assert_eq!(report.full_articles, 0);
    block_on(core.set_feed_full_text(feed.id, true)).unwrap();
    assert!(core.feeds()[0].full_text);
    let report = block_on(core.refresh(None)).unwrap();
    assert_eq!(report.full_articles, 1);
    let alpha = core
        .timeline(Timeline::All)
        .into_iter()
        .find(|a| a.title == "Alpha")
        .unwrap();
    let beta = core
        .timeline(Timeline::All)
        .into_iter()
        .find(|a| a.title == "Beta")
        .unwrap();
    let text = block_on(core.open_article(alpha.id)).unwrap();
    assert!(text.full);
    // The page's text is searchable too.
    assert_eq!(
        block_on(core.search("walrus".into()))
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect::<Vec<_>>(),
        [alpha.id]
    );
    // Beta's page failed: it keeps the feed's text and isn't tried again.
    let text = block_on(core.open_article(beta.id)).unwrap();
    assert!(!text.full);
    assert!(!text.body.blocks.is_empty());
    assert_eq!(block_on(core.refresh(None)).unwrap().full_articles, 0);
    // Asking again (turning it off and on) gives the failure another try.
    block_on(core.set_feed_full_text(feed.id, false)).unwrap();
    block_on(core.set_feed_full_text(feed.id, true)).unwrap();
    assert_eq!(block_on(core.refresh(None)).unwrap().full_articles, 0);
}

#[test]
fn one_article_can_be_loaded_in_full_on_demand_and_stays_full() {
    let dir = tempfile::tempdir().unwrap();
    let http = StaticClient::new()
        .route(
            Method::Get,
            "https://fixtures.example/a",
            page(article_page("Alpha", "otter")),
        )
        .route(
            Method::Get,
            "https://fixtures.example/b",
            Ok({
                let mut r = Response::new(200, b"%PDF-1.7".to_vec());
                r.headers.push(forge_net::Header {
                    name: "Content-Type".into(),
                    value: "application/pdf".into(),
                });
                r
            }),
        );
    let (core, _) = full_text_library(dir.path(), http);
    let alpha = core
        .timeline(Timeline::All)
        .into_iter()
        .find(|a| a.title == "Alpha")
        .unwrap();
    let beta = core
        .timeline(Timeline::All)
        .into_iter()
        .find(|a| a.title == "Beta")
        .unwrap();
    let text = block_on(core.load_full_article(alpha.id, None)).unwrap();
    assert!(text.full);
    assert!(matches!(
        block_on(core.load_full_article(beta.id, None)),
        Err(AppError::NoReadableText { .. })
    ));
    assert!(matches!(
        block_on(core.load_full_article(999, None)),
        Err(AppError::NotFound)
    ));
    assert_eq!(
        app_error_kind(AppError::NoReadableText {
            reason: String::new()
        }),
        forge_app::ErrorKind::Invalid
    );
    // A refresh that updates the article's feed text keeps the full text.
    block_on(core.refresh(None)).unwrap();
    core.shutdown();
    drop(core);
    let core = empty_core(dir.path());
    assert!(block_on(core.open_article(alpha.id)).unwrap().full);
}
