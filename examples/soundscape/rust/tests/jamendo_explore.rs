//! Exploring Jamendo: Discover's shelves, artists to discover, an artist's
//! page with singles grouped into functional albums by their cover, and
//! "Because You Like", against canned answers.

use std::collections::HashMap;
use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::jamendo::{tracks_url, Ask, SHELVES};
use soundscape_core::jamendo_explore::{
    artist_url, artists_by_id_url, artists_url, loose_covers, main_genre, releases, songs,
    JamendoArtistList, ReleaseKind,
};
use soundscape_core::{JamendoShelf, SoundscapeCore};

const CLIENT: &str = "abc123";

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

/// One `/tracks` item.
fn item(id: u32, name: &str, album: (&str, &str), cover: &str, released: &str) -> String {
    format!(
        r#"{{"id": "{id}", "name": "{name}", "duration": 200, "artist_id": "42",
  "artist_name": "Lune", "album_id": "{}", "album_name": "{}", "position": {id},
  "releasedate": "{released}", "image": "{cover}",
  "audio": "https://prod-1.storage.jamendo.com/?trackid={id}&format=mp32",
  "license_ccurl": "http://creativecommons.org/licenses/by/4.0/",
  "audiodownload_allowed": true, "musicinfo": {{"tags": {{"genres": ["ambient"]}}}}}}"#,
        album.0, album.1
    )
}

fn answer(items: &[String]) -> String {
    format!(
        r#"{{"headers": {{"status": "success", "code": 0}}, "results": [{}]}}"#,
        items.join(",")
    )
}

const COVER_A1: &str = "https://usercontent.jamendo.com/?type=album&id=501&width=300";
const COVER_A2: &str = "https://usercontent.jamendo.com/?type=album&id=502&width=300";
const COVER_B: &str = "https://usercontent.jamendo.com/?type=album&id=503&width=300";
const COVER_ALBUM: &str = "https://usercontent.jamendo.com/?type=album&id=900&width=300";

/// Lune: a real album of two songs, two singles with the same picture at
/// different addresses, and one single on its own.
fn lune() -> String {
    answer(&[
        item(1, "Tide", ("900", "Low Water"), COVER_ALBUM, "2021-03-01"),
        item(2, "Shore", ("900", "Low Water"), COVER_ALBUM, "2021-03-01"),
        item(
            3,
            "First Light",
            ("501", "First Light"),
            COVER_A1,
            "2023-01-10",
        ),
        item(
            4,
            "Second Light",
            ("502", "Second Light"),
            COVER_A2,
            "2023-06-01",
        ),
        item(5, "Alone", ("503", "Alone"), COVER_B, "2024-02-02"),
    ])
}

const ARTIST: &str = r#"{"headers": {"status": "success", "code": 0}, "results": [
 {"id": "42", "name": "Lune", "image": "https://usercontent.jamendo.com/?type=artist&id=42", "joindate": "2019-05-03"}]}"#;

const ARTISTS: &str = r#"{"headers": {"status": "success", "code": 0}, "results": [
 {"id": "7", "name": "No Picture", "image": ""},
 {"id": "42", "name": "Lune", "image": "https://usercontent.jamendo.com/?type=artist&id=42"},
 {"id": "x", "name": "Bad Id"}]}"#;

fn client() -> StaticClient {
    let same = vec![1u8; 500];
    StaticClient::new()
        .route(
            Method::Get,
            tracks_url(CLIENT, &Ask::Releases("42".into())),
            Ok(Response::new(200, lune())),
        )
        .route(
            Method::Get,
            artist_url(CLIENT, "42"),
            Ok(Response::new(200, ARTIST)),
        )
        .route(
            Method::Get,
            artists_url(CLIENT, JamendoArtistList::Popular),
            Ok(Response::new(200, ARTISTS)),
        )
        .route(
            Method::Get,
            tracks_url(CLIENT, &Ask::Shelf(JamendoShelf::Picks)),
            Ok(Response::new(200, lune())),
        )
        .route(Method::Get, COVER_A1, Ok(Response::new(200, same.clone())))
        .route(Method::Get, COVER_A2, Ok(Response::new(200, same)))
        .route(Method::Get, COVER_B, Ok(Response::new(200, vec![2u8; 500])))
}

fn open(dir: &tempfile::TempDir, http: Arc<StaticClient>) -> Arc<SoundscapeCore> {
    let core = SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        http,
    )
    .unwrap();
    block(core.set_jamendo_client_id(CLIENT.into())).unwrap();
    core
}

#[test]
fn shelves_ask_jamendo_for_more_than_this_weeks_most_popular() {
    let url = |shelf| tracks_url(CLIENT, &Ask::Shelf(shelf));
    assert!(url(JamendoShelf::Picks).contains("featured=1"));
    assert!(url(JamendoShelf::NewReleases).contains("order=releasedate_desc"));
    assert!(url(JamendoShelf::Rising).contains("order=buzzrate"));
    assert!(url(JamendoShelf::Instrumental).contains("vocalinstrumental=instrumental"));
    assert!(url(JamendoShelf::PopularEver).contains("order=popularity_total"));
    // Singles too, on every shelf.
    assert!(SHELVES
        .iter()
        .all(|s| url(*s).contains("type=single%20albumtrack")));
    let releases = tracks_url(CLIENT, &Ask::Releases("42".into()));
    assert!(releases.contains("artist_id=42") && releases.contains("limit=200"));

    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, Arc::new(client()));
    assert_eq!(core.jamendo_shelves().len(), 6);
    let picks = block(core.jamendo_shelf(JamendoShelf::Picks, None)).unwrap();
    assert_eq!(picks.len(), 5);
    assert_eq!(
        soundscape_core::jamendo_shelf_title(JamendoShelf::Picks),
        "Jamendo's Picks"
    );
}

#[test]
fn artists_with_pictures_come_first_and_bad_ids_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, Arc::new(client()));
    let artists = block(core.jamendo_artists(JamendoArtistList::Popular, None)).unwrap();
    let names: Vec<_> = artists.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Lune", "No Picture"]);
    assert!(artists[1].image_url.is_none());
}

#[test]
fn singles_with_the_same_cover_become_one_album_and_a_lone_song_is_a_single() {
    let http = Arc::new(client());
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, Arc::clone(&http));
    let page = block(core.jamendo_artist("42".into(), None)).unwrap();
    assert_eq!(page.artist.name, "Lune");
    assert_eq!(page.artist.joined.as_deref(), Some("2019-05-03"));

    let shown: Vec<_> = page
        .releases
        .iter()
        .map(|r| (r.title.as_str(), r.kind, r.grouped, r.tracks.len()))
        .collect();
    assert_eq!(
        shown,
        [
            // Newest first.
            ("Alone", ReleaseKind::Single, false, 1),
            ("First Light", ReleaseKind::Ep, true, 2),
            ("Low Water", ReleaseKind::Ep, false, 2),
        ]
    );
    let grouped = &page.releases[1];
    assert!(grouped.id.starts_with("cover:"));
    assert_eq!(grouped.released.as_deref(), Some("2023-01-10"));
    // Oldest single first, as they came out.
    assert_eq!(grouped.tracks[0].title, "First Light");
    assert_eq!(grouped.duration_seconds, 400.0);
    assert_eq!(page.releases[0].id, "single:jamendo:5");
    assert_eq!(page.releases[2].id, "album:900");

    // The album's own cover is never fetched, and covers are fetched once.
    let covers = |http: &StaticClient| {
        http.received()
            .iter()
            .filter(|r| r.url.contains("usercontent"))
            .count()
    };
    assert_eq!(covers(&http), 3);
    block(core.jamendo_artist("42".into(), None)).unwrap();
    assert_eq!(covers(&http), 3);
}

#[test]
fn a_cover_that_cant_be_fetched_leaves_its_song_a_single() {
    let items: Vec<serde_json::Value> = serde_json::from_str::<serde_json::Value>(&lune()).unwrap()
        ["results"]
        .as_array()
        .unwrap()
        .clone();
    let songs = songs(&items);
    assert_eq!(loose_covers(&songs), [COVER_A1, COVER_A2, COVER_B]);
    let releases = releases(songs, &HashMap::new());
    let singles = releases
        .iter()
        .filter(|r| r.kind == ReleaseKind::Single)
        .count();
    assert_eq!(singles, 3);
}

#[test]
fn seven_songs_or_half_an_hour_make_an_album() {
    let long: Vec<String> = (1..=7)
        .map(|n| {
            item(
                n,
                &format!("Part {n}"),
                ("77", "Seven"),
                COVER_ALBUM,
                "2020-01-01",
            )
        })
        .collect();
    let items: Vec<serde_json::Value> = serde_json::from_str::<serde_json::Value>(&answer(&long))
        .unwrap()["results"]
        .as_array()
        .unwrap()
        .clone();
    let out = releases(songs(&items), &HashMap::new());
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].kind, ReleaseKind::Album);
    assert_eq!(out[0].title, "Seven");
}

#[test]
fn bad_artist_ids_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, Arc::new(client()));
    assert!(block(core.jamendo_artist("42&x=1".into(), None)).is_err());
}

#[test]
fn because_you_like_starts_from_the_jamendo_song_you_play_and_leaves_out_artists_you_know() {
    let other = answer(&[
        item(1, "Tide", ("900", "Low Water"), COVER_ALBUM, "2021-03-01"),
        item(60, "Elsewhere", ("", ""), COVER_B, "2022-01-01").replace("Lune", "Kai"),
        item(61, "Lune Again", ("", ""), COVER_B, "2022-01-01"),
    ]);
    let http = client()
        .route(
            Method::Get,
            tracks_url(CLIENT, &Ask::Track("1".into())),
            Ok(Response::new(200, lune())),
        )
        .route(
            Method::Get,
            tracks_url(CLIENT, &Ask::Genre("ambient".into())),
            Ok(Response::new(200, other)),
        );
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, Arc::new(http));
    assert_eq!(block(core.jamendo_because(None)).unwrap(), None);

    let picks = block(core.jamendo_shelf(JamendoShelf::Picks, None)).unwrap();
    block(core.add_catalogue_tracks(picks[..1].to_vec())).unwrap();
    // In the library but never played: nothing to start from yet.
    assert_eq!(block(core.jamendo_because(None)).unwrap(), None);
    let recording = core.library().tracks[0]
        .preferred
        .as_ref()
        .unwrap()
        .id
        .clone();
    block(core.record_play("jamendo:1".into(), recording)).unwrap();

    let because = block(core.jamendo_because(None)).unwrap().unwrap();
    assert_eq!(because.title, "Because You Like Lune");
    assert_eq!(because.from_track_id, "jamendo:1");
    let titles: Vec<_> = because.tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["Elsewhere"]);
}

#[test]
fn songs_sharing_an_address_whose_cover_cant_be_fetched_stay_singles() {
    let two = answer(&[
        item(8, "One", ("", ""), COVER_B, "2022-01-01"),
        item(9, "Two", ("", ""), COVER_B, "2022-02-01"),
    ]);
    let items: Vec<serde_json::Value> = serde_json::from_str::<serde_json::Value>(&two).unwrap()
        ["results"]
        .as_array()
        .unwrap()
        .clone();
    let out = releases(songs(&items), &HashMap::new());
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|r| r.kind == ReleaseKind::Single));
}

#[test]
fn a_songs_artist_is_found_by_its_jamendo_id() {
    let http = client().route(
        Method::Get,
        tracks_url(CLIENT, &Ask::Track("3".into())),
        Ok(Response::new(200, lune())),
    );
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, Arc::new(http));
    let id = block(core.jamendo_artist_id_of_track("jamendo:3".into(), None)).unwrap();
    assert_eq!(id, "42");
    assert!(block(core.jamendo_artist_id_of_track("archive:x".into(), None)).is_err());
}

/// A `/tracks` item by another artist, in `genre`.
fn by(id: u32, artist: (&str, &str), genre: &str) -> String {
    format!(
        r#"{{"id": "{id}", "name": "Song {id}", "duration": 200, "artist_id": "{}",
  "artist_name": "{}", "album_id": "", "album_name": "", "image": "",
  "audio": "https://prod-1.storage.jamendo.com/?trackid={id}&format=mp32",
  "musicinfo": {{"tags": {{"genres": ["{genre}"]}}}}}}"#,
        artist.0, artist.1
    )
}

#[test]
fn an_artists_main_genre_is_the_one_most_of_their_songs_carry() {
    let items: Vec<serde_json::Value> = [
        by(1, ("42", "Lune"), "ambient"),
        by(2, ("42", "Lune"), "jazz"),
        by(3, ("42", "Lune"), "jazz"),
    ]
    .iter()
    .map(|i| serde_json::from_str(i).unwrap())
    .collect();
    assert_eq!(main_genre(&items).as_deref(), Some("jazz"));
    assert_eq!(
        main_genre(&items[..2]).as_deref(),
        Some("ambient"),
        "a tie goes to the first seen"
    );
    assert_eq!(main_genre(&[]), None);
}

#[test]
fn similar_artists_are_others_popular_in_the_artists_main_genre() {
    let popular = answer(&[
        by(10, ("42", "Lune"), "ambient"),
        by(11, ("7", "Drift"), "ambient"),
        by(12, ("8", "Fjord"), "ambient"),
        by(13, ("7", "Drift"), "ambient"),
    ]);
    let pictures = r#"{"headers": {"status": "success"}, "results": [
        {"id": "8", "name": "Fjord", "image": "https://usercontent.jamendo.com/?type=artist&id=8"}]}"#;
    let http = client()
        .route(
            Method::Get,
            tracks_url(CLIENT, &Ask::Genre("ambient".into())),
            Ok(Response::new(200, popular)),
        )
        .route(
            Method::Get,
            artists_by_id_url(CLIENT, &["7".into(), "8".into()]),
            Ok(Response::new(200, pictures)),
        );
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, Arc::new(http));
    let similar = block(core.jamendo_similar_artists("42".into(), None)).unwrap();
    assert_eq!(similar.genre, "Ambient");
    let names: Vec<&str> = similar.artists.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        ["Drift", "Fjord"],
        "each once, never the artist themselves"
    );
    assert_eq!(
        similar.artists[0].image_url, None,
        "Jamendo gave Drift no picture"
    );
    assert!(similar.artists[1].image_url.is_some());
}

#[test]
fn similar_artists_keep_their_names_when_pictures_cant_be_had() {
    let popular = answer(&[by(11, ("7", "Drift"), "ambient")]);
    let http = client().route(
        Method::Get,
        tracks_url(CLIENT, &Ask::Genre("ambient".into())),
        Ok(Response::new(200, popular)),
    );
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, Arc::new(http));
    let similar = block(core.jamendo_similar_artists("42".into(), None)).unwrap();
    assert_eq!(similar.artists.len(), 1);
    assert_eq!(similar.artists[0].name, "Drift");
    assert!(block(core.jamendo_similar_artists("4 2".into(), None)).is_err());
}
