//! Listen: the take server client, against canned answers.

use std::sync::Arc;

use forge_net::{Method, NetError, Response, StaticClient};
use soundscape_core::listen::{self, normalize_server};
use soundscape_core::{loudness_match, SoundscapeCore, SoundscapeError, TakeMark};

const SERVER: &str = "http://100.64.0.7:8642";

const LIBRARY: &str = r#"{"songs": [
  {"slug": "swim", "title": "swim", "takes": 1, "updated": 1700000000, "pick": null, "pickLabel": null, "rated": 0},
  {"slug": "gavs-song", "title": "Gav's Song", "takes": 2, "updated": 1800000000.5,
   "pick": "kaggle/runs/r1/renders/gavs-song/gavs-song-7202", "pickLabel": "seed 7202", "rated": 1}
], "band": "/home/red/band", "measuring": true}"#;

const SONG: &str = r#"{"slug": "gavs-song", "title": "Gav's Song", "pick": "kaggle/runs/r1/renders/gavs-song/gavs-song-7202",
 "takes": [
  {"id": "kaggle/runs/r1/renders/gavs-song/gavs-song-7202", "file": "kaggle/runs/r1/renders/gavs-song/gavs-song-7202.mp3",
   "label": "seed 7202", "run": "r1", "mtime": 1800000000, "size": 9, "seed": 7202, "model": "turbo", "seconds": 180,
   "bpm": 92, "key": "D minor", "caption": "fiddle, banjo", "lyrics": "[verse]\nhello", "lufs": -12.0, "stars": 4, "note": "tight"},
  {"id": "kaggle/runs/r1/renders/gavs-song/gavs-song-7202-xl-turbo", "file": "kaggle/runs/r1/renders/gavs-song/gavs-song 7202 xl.mp3",
   "label": "seed 7202 · xl-turbo", "run": "r1", "mtime": 1800000001, "size": 9, "seed": 7202, "model": "xl-turbo",
   "seconds": 180, "bpm": "92", "key": null, "caption": "fiddle, banjo", "lyrics": "[verse]\nhello", "lufs": null, "stars": 0, "note": ""}
 ]}"#;

fn core_with(http: StaticClient) -> (tempfile::TempDir, Arc<SoundscapeCore>, Arc<StaticClient>) {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(http);
    let core = SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        http.clone(),
    )
    .unwrap();
    (dir, core, http)
}

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

#[test]
fn addresses_are_tidied_and_checked() {
    assert_eq!(
        normalize_server("100.64.0.7").unwrap(),
        "http://100.64.0.7:8642"
    );
    assert_eq!(
        normalize_server(" http://100.64.0.7:9000/ ").unwrap(),
        "http://100.64.0.7:9000"
    );
    assert_eq!(
        normalize_server("192.168.1.169:8642").unwrap(),
        "http://192.168.1.169:8642"
    );
    assert_eq!(
        normalize_server("https://band.example.ts.net").unwrap(),
        "https://band.example.ts.net:443"
    );
    assert_eq!(
        normalize_server("[fd7a:115c::1]:8642").unwrap(),
        "http://[fd7a:115c::1]:8642"
    );
    for bad in [
        "",
        "create",
        "http://create:8642",
        "ftp://1.2.3.4",
        "1.2.3.4/api",
        "1.2.3.4:port",
        "https://a b",
    ] {
        assert!(
            matches!(
                normalize_server(bad),
                Err(SoundscapeError::BadAddress { .. })
            ),
            "{bad:?} should be refused"
        );
    }
}

#[test]
fn loudness_is_matched_by_turning_the_louder_take_down() {
    let m = loudness_match(Some(-12.0), Some(-18.0));
    assert!(m.matched);
    assert_eq!(m.b_volume, 1.0);
    assert!((m.a_volume - 0.501).abs() < 0.001, "{}", m.a_volume); // 6 dB down
    assert_eq!(m.difference_db, 6.0);
    let m = loudness_match(Some(-20.0), Some(-14.0));
    assert_eq!(m.a_volume, 1.0);
    assert!(m.b_volume < 1.0);
    let unmeasured = loudness_match(Some(-12.0), None);
    assert!(!unmeasured.matched);
    assert_eq!((unmeasured.a_volume, unmeasured.b_volume), (1.0, 1.0));
    // a broken measurement can't silence a take
    let m = loudness_match(Some(0.0), Some(-90.0));
    assert!(m.a_volume > 0.06);
}

#[test]
fn songs_and_takes_come_from_the_server() {
    let http = StaticClient::new()
        .route(
            Method::Get,
            format!("{SERVER}/api/library"),
            Ok(Response::new(200, LIBRARY)),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=gavs-song"),
            Ok(Response::new(200, SONG)),
        );
    let (_dir, core, _) = core_with(http);
    assert_eq!(core.listen_server(), None);
    assert!(matches!(
        block(core.listen_songs(false, None)),
        Err(SoundscapeError::BadAddress { .. })
    ));

    let saved = block(core.set_listen_server("100.64.0.7".into())).unwrap();
    assert_eq!(saved.as_deref(), Some(SERVER));
    assert_eq!(core.listen_server().as_deref(), Some(SERVER));

    let songs = block(core.listen_songs(false, None)).unwrap();
    assert_eq!(
        songs.iter().map(|s| s.slug.as_str()).collect::<Vec<_>>(),
        ["gavs-song", "swim"]
    );
    assert_eq!(songs[0].title, "Gav's Song");
    assert_eq!(songs[0].pick_label.as_deref(), Some("seed 7202"));
    assert_eq!(songs[0].updated_ms, 1_800_000_000_500);

    let song = block(core.listen_song("gavs-song".into(), None)).unwrap();
    assert_eq!(song.caption.as_deref(), Some("fiddle, banjo"));
    assert_eq!(song.takes.len(), 2);
    let (a, b) = (&song.takes[0], &song.takes[1]);
    assert!(a.picked && !b.picked);
    assert_eq!(
        (a.stars, a.note.as_str(), a.lufs),
        (4, "tight", Some(-12.0))
    );
    assert_eq!(
        (a.seed, a.bpm, a.key.as_deref()),
        (Some(7202), Some(92.0), Some("D minor"))
    );
    assert_eq!(b.bpm, Some(92.0)); // a number sent as text
    assert_eq!(
        b.audio_url,
        format!("{SERVER}/audio/kaggle/runs/r1/renders/gavs-song/gavs-song%207202%20xl.mp3")
    );
}

#[test]
fn the_address_is_kept_and_can_be_forgotten() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let caches = dir.path().join("caches");
    {
        let core = SoundscapeCore::open_with_http(
            data.to_str().unwrap(),
            caches.to_str().unwrap(),
            Arc::new(StaticClient::new()),
        )
        .unwrap();
        block(core.set_listen_server("100.64.0.7".into())).unwrap();
        core.shutdown();
        drop(core);
    }
    let core = SoundscapeCore::open_with_http(
        data.to_str().unwrap(),
        caches.to_str().unwrap(),
        Arc::new(StaticClient::new()),
    )
    .unwrap();
    assert_eq!(core.listen_server().as_deref(), Some(SERVER));
    assert_eq!(block(core.set_listen_server("  ".into())).unwrap(), None);
    assert_eq!(core.listen_server(), None);
    // a bad address changes nothing
    block(core.set_listen_server("100.64.0.7".into())).unwrap();
    assert!(block(core.set_listen_server("create".into())).is_err());
    assert_eq!(core.listen_server().as_deref(), Some(SERVER));
}

#[test]
fn marking_a_take_posts_it_and_returns_the_song() {
    let http = StaticClient::new()
        .route(
            Method::Post,
            format!("{SERVER}/api/pick"),
            Ok(Response::new(200, r#"{"ok": true}"#)),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=gavs-song"),
            Ok(Response::new(200, SONG)),
        );
    let (_dir, core, http) = core_with(http);
    block(core.set_listen_server(SERVER.into())).unwrap();
    let take = "kaggle/runs/r1/renders/gavs-song/gavs-song-7202".to_string();
    let mark = TakeMark {
        picked: Some(true),
        stars: Some(5),
        note: Some(" keeper ".into()),
    };
    let song = block(core.mark_take("gavs-song".into(), take.clone(), mark)).unwrap();
    assert_eq!(song.slug, "gavs-song");
    let sent = &http.received()[0];
    let body: serde_json::Value = serde_json::from_slice(&sent.body).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"song": "gavs-song", "take": take, "pick": true, "stars": 5, "note": "keeper"})
    );
    // only what changes is sent
    assert_eq!(
        listen::mark_body(
            "s",
            "t",
            &TakeMark {
                stars: Some(2),
                ..Default::default()
            }
        )
        .unwrap(),
        br#"{"song":"s","stars":2,"take":"t"}"#.to_vec()
    );
    assert!(listen::mark_body(
        "s",
        "t",
        &TakeMark {
            stars: Some(6),
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn failures_say_what_went_wrong() {
    let http = StaticClient::new()
        .route(
            Method::Get,
            format!("{SERVER}/api/library"),
            Err(NetError::Offline {
                reason: "no route".into(),
            }),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=x"),
            Ok(Response::new(200, "<html>")),
        );
    let (_dir, core, _) = core_with(http);
    block(core.set_listen_server(SERVER.into())).unwrap();
    assert!(matches!(
        block(core.listen_songs(false, None)),
        Err(SoundscapeError::Unreachable { .. })
    ));
    assert!(matches!(
        block(core.listen_song("x".into(), None)),
        Err(SoundscapeError::Server { .. })
    ));
    // unknown song: StaticClient answers 404
    assert!(matches!(
        block(core.listen_song("gone".into(), None)),
        Err(SoundscapeError::Server { .. })
    ));
}

#[test]
fn songs_are_sectioned_by_act() {
    let body = r#"{"songs": [
      {"slug": "ah-one-more-night", "title": "AFTER HOURS · One More Night", "act": "after-hours",
       "actTitle": "AFTER HOURS", "takes": 2, "updated": 300, "pickLabel": "seed 1501"},
      {"slug": "ah-everybody-knows", "title": "AFTER HOURS · Everybody Knows", "act": "after-hours",
       "actTitle": "AFTER HOURS", "takes": 2, "updated": 100},
      {"slug": "nc-swim", "title": "Natural Capital · Swim at Your Own Risk", "act": "natural-capital",
       "actTitle": "Natural Capital", "takes": 3, "updated": 200},
      {"slug": "swim", "title": "swim", "takes": 1, "updated": 900},
      {"slug": "lone", "title": "Lone", "act": "lowbridge", "takes": 1, "updated": 50}
    ]}"#;
    let songs = listen::parse_library(body.as_bytes()).unwrap();
    let swim = songs.iter().find(|s| s.slug == "nc-swim").unwrap();
    // the act is shown apart, so it comes off the title
    assert_eq!(swim.title, "Swim at Your Own Risk");
    assert_eq!(swim.act.as_deref(), Some("Natural Capital"));
    // no actTitle: the act's id stands in
    assert_eq!(
        songs
            .iter()
            .find(|s| s.slug == "lone")
            .unwrap()
            .act
            .as_deref(),
        Some("lowbridge")
    );

    let groups = listen::group_songs(songs.clone(), false);
    let shape: Vec<(Option<&str>, Vec<&str>)> = groups
        .iter()
        .map(|g| {
            (
                g.act.as_deref(),
                g.songs.iter().map(|s| s.slug.as_str()).collect(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        vec![
            (
                Some("AFTER HOURS"),
                vec!["ah-one-more-night", "ah-everybody-knows"]
            ),
            (Some("Natural Capital"), vec!["nc-swim"]),
            (Some("lowbridge"), vec!["lone"]),
            (None, vec!["swim"]),
        ]
    );

    let unpicked = listen::group_songs(songs, true);
    assert_eq!(unpicked[0].act.as_deref(), Some("Natural Capital"));
    assert_eq!(unpicked.iter().map(|g| g.songs.len()).sum::<usize>(), 4);
    assert!(listen::group_songs(Vec::new(), true).is_empty());
}
