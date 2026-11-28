//! Live radio through Radio Browser: stations are read safely from its
//! answers, servers are learnt and skipped when they fail, and saved and
//! recent stations are kept by UUID and looked up again when stale, against
//! canned answers in Radio Browser's shapes.

use std::sync::Arc;

use forge_net::{CancelHandle, Header, Method, NetError, Response, StaticClient};
use soundscape_core::live_radio::{
    categories_path, for_map, parse_categories, parse_servers, parse_stations, rest_for,
    station_path, stations_path, Directory, LiveLocation, LiveRadioFile, BUSY_REST, MAX_RECENTS,
    MAX_REST, STALE_MS,
};
use soundscape_core::{LiveAsk, LiveCategoryKind, LiveStation, SoundscapeCore, SoundscapeError};

const KYOTO: &str = "11111111-2222-3333-4444-555555555555";
const LONDON: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

fn stations_json(london_url: &str) -> String {
    format!(
        r#"[
  {{"stationuuid": "{KYOTO}", "name": "  Kyoto Jazz FM ", "url": "http://kyoto.example/listen.pls",
   "url_resolved": "https://stream.kyoto.example/jazz", "homepage": "https://kyoto.example",
   "favicon": "https://kyoto.example/logo.png", "country": "Japan", "countrycode": "jp",
   "language": "japanese,english, Japanese", "tags": "jazz,smooth jazz,,", "codec": "AAC+",
   "bitrate": 64, "hls": 0, "clickcount": 812, "lastcheckok": 1}},
  {{"stationuuid": "{LONDON}", "name": "London Talk", "url": "{london_url}", "url_resolved": "",
   "homepage": "javascript:alert(1)", "favicon": "", "country": "The United Kingdom Of Great Britain And Northern Ireland",
   "countrycode": "GB", "language": "english", "tags": "news,talk", "codec": "MP3", "bitrate": 128,
   "hls": 1, "clickcount": 90}},
  {{"stationuuid": "{KYOTO}", "name": "Kyoto Jazz FM (copy)", "url": "https://x.example/a"}},
  {{"stationuuid": "99999999-9999-9999-9999-999999999999", "name": "Ogg Only", "url": "https://ogg.example/s", "codec": "OGG"}},
  {{"stationuuid": "88888888-8888-8888-8888-888888888888", "name": "No Stream", "url": "rtsp://bad.example/s"}},
  {{"stationuuid": "not-a-uuid", "name": "Broken", "url": "https://broken.example/s"}},
  {{"stationuuid": "77777777-7777-7777-7777-777777777777", "name": "", "url": "https://noname.example/s"}}
]"#
    )
}

const SERVERS: &str = r#"[
  {"ip": "1.2.3.4", "name": "nl1.api.radio-browser.info"},
  {"ip": "5.6.7.8", "name": "at1.api.radio-browser.info"},
  {"ip": "5.6.7.9", "name": "at1.api.radio-browser.info"},
  {"ip": "6.6.6.6", "name": "evil.example.com"}
]"#;

const COUNTRIES: &str = r#"[
  {"name": "Japan", "iso_3166_1": "JP", "stationcount": 412},
  {"name": "Nowhere", "iso_3166_1": "XX", "stationcount": 0},
  {"name": "Bad Code", "iso_3166_1": "XYZ", "stationcount": 3},
  {"name": "The United Kingdom Of Great Britain And Northern Ireland", "iso_3166_1": "gb", "stationcount": "2210"}
]"#;

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

#[test]
fn stations_are_read_safely_and_unplayable_ones_left_out() {
    let stations = parse_stations(stations_json("http://london.example/live").as_bytes()).unwrap();
    let names: Vec<&str> = stations.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Kyoto Jazz FM", "London Talk"]);

    let kyoto = &stations[0];
    assert_eq!(kyoto.uuid, KYOTO);
    assert_eq!(kyoto.stream_url, "https://stream.kyoto.example/jazz");
    assert_eq!(kyoto.listed_url, "http://kyoto.example/listen.pls");
    assert!(kyoto.secure);
    assert_eq!(kyoto.country_code, "JP");
    assert_eq!(
        kyoto.languages,
        ["japanese", "english"],
        "duplicates and case"
    );
    assert_eq!(kyoto.tags, ["jazz", "smooth jazz"]);
    assert_eq!(kyoto.codec, "AAC+");
    assert_eq!(kyoto.bitrate_kbps, 64);
    assert_eq!(kyoto.plays, 812);
    assert!(!kyoto.hls);

    let london = &stations[1];
    assert_eq!(
        london.stream_url, "http://london.example/live",
        "listed address when none resolved"
    );
    assert!(!london.secure);
    assert!(london.hls);
    assert_eq!(london.homepage, None, "only http(s) addresses");
    assert_eq!(london.artwork_url, None);

    assert!(parse_stations(b"{\"error\": 1}").is_err());
    assert!(parse_stations(b"not json").is_err());
}

#[test]
fn requests_are_built_from_checked_input() {
    let top = stations_path(&LiveAsk::Top).unwrap();
    assert!(
        top.contains("hidebroken=true") && top.contains("order=clickcount"),
        "{top}"
    );
    let search = stations_path(&LiveAsk::Search {
        text: "jazz & blues&countrycode=GB".into(),
    })
    .unwrap();
    assert!(
        search.contains("name=jazz%20%26%20blues%26countrycode%3DGB"),
        "{search}"
    );
    assert_eq!(
        stations_path(&LiveAsk::Search { text: "  ".into() }).unwrap(),
        top,
        "an empty search is the top stations"
    );
    assert!(stations_path(&LiveAsk::Country { code: "jp".into() })
        .unwrap()
        .contains("countrycode=JP"));
    assert!(stations_path(&LiveAsk::Country { code: "J1".into() }).is_err());
    assert!(stations_path(&LiveAsk::Country { code: "JPN".into() }).is_err());
    assert!(stations_path(&LiveAsk::Genre { tag: "".into() }).is_err());
    assert!(stations_path(&LiveAsk::Language {
        name: "Japanese".into()
    })
    .unwrap()
    .contains("language=japanese&languageExact=true"));
    assert_eq!(
        station_path(KYOTO),
        format!("/json/stations/byuuid/{KYOTO}")
    );
}

#[test]
fn categories_and_servers_are_read() {
    let countries = parse_categories(LiveCategoryKind::Country, COUNTRIES.as_bytes()).unwrap();
    let keys: Vec<&str> = countries.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["JP", "GB"], "no empty countries or bad codes");
    assert_eq!(countries[1].station_count, 2210);

    let genres = parse_categories(
        LiveCategoryKind::Genre,
        br#"[{"name": "smooth jazz", "stationcount": 40}]"#,
    )
    .unwrap();
    assert_eq!(genres[0].key, "smooth jazz");
    assert_eq!(genres[0].name, "Smooth Jazz");
    assert!(categories_path(LiveCategoryKind::Language).starts_with("/json/languages?"));

    assert_eq!(
        parse_servers(SERVERS.as_bytes()).unwrap(),
        ["nl1.api.radio-browser.info", "at1.api.radio-browser.info"],
        "each once, and only Radio Browser's own"
    );
}

/// A seed that can't be reached, one that names the servers, and the first
/// named server failing: the next one answers.
fn directory_client(top: Result<Response, NetError>) -> StaticClient {
    let top_path = stations_path(&LiveAsk::Top).unwrap();
    StaticClient::new()
        .route(
            Method::Get,
            "https://de1.api.radio-browser.info/json/servers",
            Err(NetError::Timeout),
        )
        .route(
            Method::Get,
            "https://de2.api.radio-browser.info/json/servers",
            Ok(Response::new(200, SERVERS)),
        )
        .route(
            Method::Get,
            format!("https://nl1.api.radio-browser.info{top_path}"),
            Ok(Response::new(503, "")),
        )
        .route(
            Method::Get,
            format!("https://at1.api.radio-browser.info{top_path}"),
            top,
        )
}

#[test]
fn servers_are_learnt_once_and_a_failing_one_is_skipped() {
    let client = directory_client(Ok(Response::new(
        200,
        stations_json("http://london.example/live"),
    )));
    let directory = Directory::default();
    let cancel = CancelHandle::new();
    let found = directory.stations(&client, &LiveAsk::Top, &cancel).unwrap();
    assert_eq!(found.len(), 2);
    let asked = client.received().len();
    assert_eq!(asked, 4, "de1, de2 (list), nl1 (503), at1");

    // The list is kept, and the failed server now comes last.
    directory.stations(&client, &LiveAsk::Top, &cancel).unwrap();
    let urls: Vec<String> = client.received().iter().map(|r| r.url.clone()).collect();
    assert_eq!(urls.len(), asked + 1);
    assert!(urls[asked].starts_with("https://at1."), "{urls:?}");
}

/// Like `directory_client`, but nl1 is too busy (429) and says when to
/// come back; at1 answers with `top`.
fn busy_client(top: Result<Response, NetError>) -> StaticClient {
    let top_path = stations_path(&LiveAsk::Top).unwrap();
    StaticClient::new()
        .route(
            Method::Get,
            "https://de1.api.radio-browser.info/json/servers",
            Ok(Response::new(200, SERVERS)),
        )
        .route(
            Method::Get,
            format!("https://nl1.api.radio-browser.info{top_path}"),
            Ok(Response {
                status: 429,
                headers: vec![Header {
                    name: "Retry-After".into(),
                    value: "120".into(),
                }],
                body: Vec::new(),
            }),
        )
        .route(
            Method::Get,
            format!("https://at1.api.radio-browser.info{top_path}"),
            top,
        )
}

#[test]
fn a_busy_server_is_skipped_until_its_retry_after() {
    let client = busy_client(Ok(Response::new(
        200,
        stations_json("http://london.example/live"),
    )));
    let directory = Directory::default();
    let cancel = CancelHandle::new();
    assert_eq!(
        directory
            .stations(&client, &LiveAsk::Top, &cancel)
            .unwrap()
            .len(),
        2,
        "the next mirror answered"
    );
    let asked = client.received().len();
    assert_eq!(asked, 3, "de1 (list), nl1 (429), at1");

    // nl1 asked for two minutes' rest: it isn't asked again meanwhile.
    directory.stations(&client, &LiveAsk::Top, &cancel).unwrap();
    let urls: Vec<String> = client.received().iter().map(|r| r.url.clone()).collect();
    assert_eq!(urls.len(), asked + 1, "{urls:?}");
    assert!(urls[asked].starts_with("https://at1."), "{urls:?}");
}

#[test]
fn every_server_busy_says_so_and_asks_none_again() {
    let client = busy_client(Ok(Response::new(429, "")));
    let directory = Directory::default();
    let cancel = CancelHandle::new();
    let busy = SoundscapeError::Server {
        reason: "Radio Browser is busy; try again in a minute".into(),
    };
    assert_eq!(
        directory
            .stations(&client, &LiveAsk::Top, &cancel)
            .unwrap_err(),
        busy
    );
    let asked = client.received().len();
    assert_eq!(
        directory
            .stations(&client, &LiveAsk::Top, &cancel)
            .unwrap_err(),
        busy
    );
    assert_eq!(client.received().len(), asked, "both still resting");
}

#[test]
fn retry_after_is_followed_within_limits() {
    assert_eq!(rest_for(Some("120")).as_secs(), 120);
    assert_eq!(rest_for(Some(" 5 ")).as_secs(), 5);
    assert_eq!(rest_for(Some("86400")), MAX_REST);
    assert_eq!(rest_for(Some("Wed, 21 Oct 2026 07:28:00 GMT")), BUSY_REST);
    assert_eq!(rest_for(None), BUSY_REST);
}

#[test]
fn every_server_failing_is_one_clear_error() {
    let client = directory_client(Err(NetError::Timeout));
    let err = Directory::default()
        .stations(&client, &LiveAsk::Top, &CancelHandle::new())
        .unwrap_err();
    assert_eq!(
        err,
        SoundscapeError::Server {
            reason: "can't reach Radio Browser".into()
        }
    );
}

#[test]
fn saved_and_recent_stations_are_kept_by_uuid() {
    let stations = parse_stations(stations_json("http://london.example/live").as_bytes()).unwrap();
    let (kyoto, london) = (stations[0].clone(), stations[1].clone());
    let mut file = LiveRadioFile::default();
    file.save(kyoto.clone(), 1_000).unwrap();
    file.save(london.clone(), 2_000).unwrap();
    let mut renamed = kyoto.clone();
    renamed.name = "Kyoto Jazz".into();
    file.save(renamed, 3_000).unwrap();
    let view = file.view();
    assert_eq!(
        view.saved.len(),
        2,
        "saving again refreshes, keeps its place"
    );
    assert_eq!(view.saved[0].name, "Kyoto Jazz");

    for n in 0..(MAX_RECENTS as i64 + 5) {
        file.played(london.clone(), n, None);
    }
    file.played(kyoto.clone(), 100, None);
    assert_eq!(file.recent.len(), 2, "each station once");
    assert_eq!(file.recent[0].station.uuid, KYOTO, "newest first");

    // Saving and playing aren't lookups: Kyoto's details were never
    // checked, so they are looked up when tuned.
    assert!(file.is_stale(KYOTO, 3_000));
    file.played(kyoto.clone(), 200_000, Some(200_000));
    assert!(
        !file.is_stale(KYOTO, 200_000 + STALE_MS),
        "a lookup refreshes it"
    );
    assert!(file.is_stale(KYOTO, 200_000 + STALE_MS + 1));
    // Saving again keeps the lookup's time.
    file.save(kyoto.clone(), 300_000).unwrap();
    assert!(!file.is_stale(KYOTO, 200_000 + STALE_MS));
    // Details with another stream address don't inherit that lookup's time.
    let mut moved = kyoto.clone();
    moved.stream_url = "https://kyoto.example/elsewhere".into();
    let mut other = LiveRadioFile::default();
    other.played(kyoto.clone(), 1, Some(200_000));
    other.played(moved, 2, None);
    assert!(
        other.is_stale(KYOTO, 200_001),
        "the moved address was never checked"
    );
    // Saving details with another address doesn't borrow that lookup's time.
    let mut old_address = kyoto.clone();
    old_address.stream_url = "https://kyoto.example/old".into();
    let mut third = LiveRadioFile::default();
    third.played(kyoto.clone(), 1, Some(200_000));
    third.save(old_address, 2).unwrap();
    third.clear_recent();
    assert!(
        third.is_stale(KYOTO, 200_001),
        "the saved address was never checked"
    );
    // A failed stream makes every copy stale.
    file.mark_stale(KYOTO);
    assert!(
        file.is_stale(KYOTO, 200_001),
        "looked up again after a failure"
    );
    let mut stranger = kyoto.clone();
    stranger.uuid = "00000000-0000-0000-0000-000000000000".into();
    file.played(stranger, 5, None);
    assert!(
        file.is_stale("00000000-0000-0000-0000-000000000000", 5),
        "never checked"
    );
    assert!(
        file.is_stale("12345678-0000-0000-0000-000000000000", 0),
        "unknown is looked up"
    );
    assert!(file.unsave(LONDON));
    assert!(!file.is_saved(LONDON));
}

fn core_client(london_now: &str) -> StaticClient {
    StaticClient::new()
        .route(
            Method::Get,
            "https://de1.api.radio-browser.info/json/servers",
            Ok(Response::new(200, SERVERS)),
        )
        .route(
            Method::Get,
            format!(
                "https://nl1.api.radio-browser.info{}",
                stations_path(&LiveAsk::Top).unwrap()
            ),
            Ok(Response::new(
                200,
                stations_json("http://london.example/live"),
            )),
        )
        .route(
            Method::Get,
            format!("https://nl1.api.radio-browser.info{}", station_path(LONDON)),
            Ok(Response::new(200, stations_json(london_now))),
        )
        .route(
            Method::Get,
            format!("https://nl1.api.radio-browser.info{}", station_path(KYOTO)),
            Ok(Response::new(200, "[]")),
        )
}

fn open(dir: &tempfile::TempDir, client: StaticClient) -> Arc<SoundscapeCore> {
    SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(client),
    )
    .unwrap()
}

#[test]
fn tuning_looks_a_station_up_again_and_keeps_it_in_recent() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, core_client("https://london.example/moved"));
    let top = block(core.live_stations(LiveAsk::Top, None)).unwrap();
    let london = top.iter().find(|s| s.uuid == LONDON).unwrap().clone();
    assert_eq!(london.stream_url, "http://london.example/live");

    // Never played: looked up by UUID, so the moved stream is used.
    let tuned = block(core.tune_live_station(london.clone(), None)).unwrap();
    assert_eq!(tuned.stream_url, "https://london.example/moved");
    assert!(tuned.secure);
    assert_eq!(core.live_radio().recent[0].stream_url, tuned.stream_url);

    block(core.save_live_station(tuned.clone())).unwrap();
    assert!(core.is_live_station_saved(LONDON.into()));

    // A station Radio Browser no longer lists.
    let kyoto = top.iter().find(|s| s.uuid == KYOTO).unwrap().clone();
    let err = block(core.tune_live_station(kyoto, None)).unwrap_err();
    assert!(
        matches!(err, SoundscapeError::NotInLibrary { .. }),
        "{err:?}"
    );
    assert_eq!(
        block(core.live_station_failed(KYOTO.into(), None)).unwrap(),
        None
    );

    drop(core);
    let core = open(&dir, StaticClient::new());
    let kept = core.live_radio();
    assert_eq!(kept.saved.len(), 1, "saved stations survive a restart");
    assert_eq!(kept.recent.len(), 1);

    // Fresh: played from what was kept, with no request.
    let again = block(core.tune_live_station(london, None)).unwrap();
    assert_eq!(again.stream_url, "https://london.example/moved");

    block(core.unsave_live_station(LONDON.into())).unwrap();
    let cleared = block(core.clear_recent_live_stations()).unwrap();
    assert!(cleared.saved.is_empty() && cleared.recent.is_empty());
}

#[test]
fn an_unreachable_directory_still_plays_what_was_given() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, core_client("https://london.example/moved"));
    let london = block(core.live_stations(LiveAsk::Top, None))
        .unwrap()
        .into_iter()
        .find(|s| s.uuid == LONDON)
        .unwrap();
    drop(core);
    let core = open(
        &dir,
        StaticClient::new().route(
            Method::Get,
            "https://de1.api.radio-browser.info/json/servers",
            Err(NetError::Timeout),
        ),
    );
    let tuned = block(core.tune_live_station(london.clone(), None)).unwrap();
    assert_eq!(tuned, london);
    assert!(block(core.live_station_failed(LONDON.into(), None)).is_err());
}

const MAPPED: &str = r#"[
  {"stationuuid": "11111111-2222-3333-4444-555555555555", "name": "Kyoto Jazz FM", "url": "https://kyoto.example/s",
   "geo_lat": 35.0116, "geo_long": 135.7681, "clickcount": 812},
  {"stationuuid": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "name": "London Talk", "url": "https://london.example/s",
   "geo_lat": "51.5072", "geo_long": "-0.1276", "clickcount": 90},
  {"stationuuid": "22222222-2222-2222-2222-222222222222", "name": "Edinburgh Folk", "url": "https://edi.example/s",
   "geo_lat": 55.9533, "geo_long": -3.1883, "clickcount": 40},
  {"stationuuid": "33333333-3333-3333-3333-333333333333", "name": "Null Island", "url": "https://zero.example/s",
   "geo_lat": 0, "geo_long": 0},
  {"stationuuid": "44444444-4444-4444-4444-444444444444", "name": "Off The Globe", "url": "https://off.example/s",
   "geo_lat": 123.0, "geo_long": 10.0},
  {"stationuuid": "55555555-5555-5555-5555-555555555555", "name": "Nowhere Known", "url": "https://none.example/s",
   "geo_lat": null}
]"#;

#[test]
fn stations_keep_a_real_location_or_none() {
    let stations = parse_stations(MAPPED.as_bytes()).unwrap();
    assert_eq!(
        stations.len(),
        6,
        "a missing location never drops a station"
    );
    let at = |name: &str| stations.iter().find(|s| s.name == name).unwrap().location;
    let kyoto = at("Kyoto Jazz FM").unwrap();
    assert!((kyoto.latitude - 35.0116).abs() < 1e-9 && (kyoto.longitude - 135.7681).abs() < 1e-9);
    assert!(
        at("London Talk").is_some(),
        "numbers written as text are read"
    );
    assert_eq!(at("Null Island"), None, "0, 0 means no location");
    assert_eq!(at("Off The Globe"), None);
    assert_eq!(at("Nowhere Known"), None);
}

#[test]
fn distances_are_great_circle_kilometres() {
    let london = LiveLocation::new(51.5072, -0.1276).unwrap();
    let edinburgh = LiveLocation::new(55.9533, -3.1883).unwrap();
    let d = london.distance_km(&edinburgh);
    assert!((520.0..545.0).contains(&d), "{d}");
    assert!(london.distance_km(&london) < 1e-6);
}

#[test]
fn map_requests_ask_only_for_placed_stations_around_a_checked_place() {
    let mapped = stations_path(&LiveAsk::Mapped).unwrap();
    assert!(
        mapped.contains("has_geo_info=true") && mapped.contains("limit=300"),
        "{mapped}"
    );
    let near = stations_path(&LiveAsk::Near {
        latitude: 55.95,
        longitude: -3.19,
        radius_km: 50.0,
    })
    .unwrap();
    assert!(
        near.contains("geo_lat=55.9500")
            && near.contains("geo_long=-3.1900")
            && near.contains("geo_distance=50000"),
        "{near}"
    );
    let wide = stations_path(&LiveAsk::Near {
        latitude: 0.0,
        longitude: 0.0,
        radius_km: 1e9,
    })
    .unwrap();
    assert!(
        wide.contains("geo_distance=3000000"),
        "the radius is capped and 0, 0 is a place: {wide}"
    );
    let tight = stations_path(&LiveAsk::Near {
        latitude: 1.0,
        longitude: 1.0,
        radius_km: 0.0,
    })
    .unwrap();
    assert!(tight.contains("geo_distance=5000"), "{tight}");
    assert!(stations_path(&LiveAsk::Near {
        latitude: 91.0,
        longitude: 0.0,
        radius_km: 10.0
    })
    .is_err());
    assert!(stations_path(&LiveAsk::Near {
        latitude: f64::NAN,
        longitude: 0.0,
        radius_km: 10.0
    })
    .is_err());
}

#[test]
fn a_map_keeps_only_stations_it_can_place_inside_its_circle() {
    let stations = parse_stations(MAPPED.as_bytes()).unwrap();
    let names = |found: Vec<LiveStation>| found.into_iter().map(|s| s.name).collect::<Vec<_>>();
    assert_eq!(
        names(for_map(&LiveAsk::Mapped, stations.clone())),
        ["Kyoto Jazz FM", "London Talk", "Edinburgh Folk"]
    );
    // A server that ignored the area still yields only what is inside it.
    let around_edinburgh = LiveAsk::Near {
        latitude: 55.95,
        longitude: -3.19,
        radius_km: 100.0,
    };
    assert_eq!(
        names(for_map(&around_edinburgh, stations.clone())),
        ["Edinburgh Folk"]
    );
    let around_britain = LiveAsk::Near {
        latitude: 54.0,
        longitude: -2.0,
        radius_km: 600.0,
    };
    assert_eq!(
        names(for_map(&around_britain, stations.clone())),
        ["London Talk", "Edinburgh Folk"]
    );
    // Other lists are left as they are, located or not.
    assert_eq!(
        for_map(&LiveAsk::Top, stations.clone()).len(),
        stations.len()
    );
}
