//! Catalogue sources: Jamendo search, rights kept as separate facts, and
//! Jamendo tracks behaving like the person's own music once added (library,
//! playlists, favourites, restart, downloads), against canned answers.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use forge_net::{HttpClient, NetError, Request};

use forge_net::{Method, Response, StaticClient};
use soundscape_core::catalogue::rights_from_licence;
use soundscape_core::jamendo::{self, search_url};
use soundscape_core::{AudioSource, DownloadState, RecordingKind, SoundscapeCore, SoundscapeError};

const CLIENT: &str = "abc123";

const RESULTS: &str = r#"{"headers": {"status": "success", "code": 0, "error_message": "", "results_count": 3},
 "results": [
  {"id": "1214935", "name": "Wise Up", "duration": 183, "artist_name": "Kellee Maize",
   "album_name": "Owning My Throne", "position": 3, "image": "https://usercontent.jamendo.com/?type=album&id=1&width=300",
   "audio": "https://prod-1.storage.jamendo.com/?trackid=1214935&format=mp32",
   "shareurl": "https://www.jamendo.com/track/1214935",
   "license_ccurl": "http://creativecommons.org/licenses/by-nc-sa/3.0/",
   "audiodownload_allowed": true,
   "musicinfo": {"tags": {"genres": ["hiphop", "electronic"]}}},
  {"id": 77, "name": "Quiet Field", "duration": "240", "artist_name": "Ana Lu", "album_name": "",
   "position": 0, "image": "", "album_image": "https://usercontent.jamendo.com/?type=album&id=2&width=300",
   "audio": "https://prod-1.storage.jamendo.com/?trackid=77&format=mp32",
   "license_ccurl": "https://creativecommons.org/licenses/by-nd/4.0/",
   "audiodownload_allowed": false},
  {"id": "9", "name": "No Stream", "audio": "", "license_ccurl": ""}
 ]}"#;

const REFUSED: &str = r#"{"headers": {"status": "failed", "code": 5,
 "error_message": "Your credential is not authorized.", "results_count": 0}, "results": []}"#;

fn wise() -> String {
    "jamendo:1214935".into()
}

fn quiet() -> String {
    "jamendo:77".into()
}

fn jamendo() -> StaticClient {
    StaticClient::new()
        .route(
            Method::Get,
            search_url(CLIENT, "kellee"),
            Ok(Response::new(200, RESULTS)),
        )
        .route(
            Method::Get,
            search_url(CLIENT, ""),
            Ok(Response::new(200, RESULTS)),
        )
        .route(
            Method::Get,
            search_url("wrong", "kellee"),
            Ok(Response::new(200, REFUSED)),
        )
        .route(
            Method::Get,
            "https://prod-1.storage.jamendo.com/?trackid=1214935&format=mp32",
            Ok(Response::new(200, vec![7u8; 3000])),
        )
        .route(
            Method::Get,
            "https://prod-1.storage.jamendo.com/?trackid=77&format=mp32",
            Ok(Response::new(200, vec![8u8; 3000])),
        )
}

fn open(dir: &tempfile::TempDir) -> Arc<SoundscapeCore> {
    SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(jamendo()),
    )
    .unwrap()
}

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

/// A core with the client ID set and no take server.
fn ready() -> (tempfile::TempDir, Arc<SoundscapeCore>) {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    block(core.set_jamendo_client_id(format!("  {CLIENT} "))).unwrap();
    (dir, core)
}

#[test]
fn licences_become_separate_rights() {
    let nc_sa = rights_from_licence(
        Some("http://creativecommons.org/licenses/by-nc-sa/3.0/"),
        true,
        "credit",
        None,
    );
    assert_eq!(nc_sa.licence, "CC BY-NC-SA 3.0");
    assert!(nc_sa.may_stream && nc_sa.may_download && nc_sa.may_adapt && nc_sa.non_commercial);
    assert_eq!(nc_sa.attribution, "credit, CC BY-NC-SA 3.0");

    let nd = rights_from_licence(
        Some("https://creativecommons.org/licenses/by-nd/4.0/"),
        false,
        "c",
        None,
    );
    assert_eq!(nd.licence, "CC BY-ND 4.0");
    assert!(!nd.may_adapt && !nd.may_download && !nd.non_commercial);

    let zero = rights_from_licence(
        Some("https://creativecommons.org/publicdomain/zero/1.0/"),
        true,
        "c",
        None,
    );
    assert_eq!(zero.licence, "CC0 1.0");
    assert!(zero.may_adapt);

    // Unknown or missing: never adaptable, downloadable only as the
    // provider says.
    for url in [None, Some(""), Some("https://example.com/my-licence")] {
        let unknown = rights_from_licence(url, false, "c", None);
        assert_eq!(unknown.licence, "Licence not stated");
        assert!(unknown.may_stream && !unknown.may_adapt && !unknown.may_download);
    }
}

#[test]
fn searching_jamendo_gives_playable_tracks_with_their_rights() {
    let (_dir, core) = ready();
    assert_eq!(core.jamendo_client_id().as_deref(), Some(CLIENT));
    let found = block(core.search_jamendo("kellee".into(), None)).unwrap();
    assert_eq!(found.len(), 2, "the track without a stream is left out");

    let wise = &found[0];
    assert_eq!(wise.id, "jamendo:1214935");
    assert_eq!(wise.provider, "Jamendo");
    assert_eq!(wise.title, "Wise Up");
    assert_eq!(wise.artist.as_deref(), Some("Kellee Maize"));
    assert_eq!(wise.album.as_deref(), Some("Owning My Throne"));
    assert_eq!(wise.number, Some(3));
    assert_eq!(wise.genre.as_deref(), Some("Hiphop"));
    assert_eq!(wise.duration_seconds, Some(183.0));
    assert!(wise.artwork_url.as_deref().unwrap().starts_with("https://"));
    assert_eq!(
        wise.rights.page_url.as_deref(),
        Some("https://www.jamendo.com/track/1214935")
    );
    assert!(wise.rights.may_download);
    assert_eq!(
        wise.rights.attribution,
        "\u{201c}Wise Up\u{201d} by Kellee Maize, via Jamendo, CC BY-NC-SA 3.0"
    );

    let quiet = &found[1];
    assert_eq!(quiet.id, "jamendo:77");
    assert_eq!(quiet.album, None);
    assert_eq!(quiet.number, None);
    assert_eq!(quiet.duration_seconds, Some(240.0));
    assert!(
        quiet.artwork_url.is_some(),
        "falls back to the album's image"
    );
    assert!(!quiet.rights.may_download && !quiet.rights.may_adapt);

    // An empty search is this week's popular tracks.
    assert_eq!(
        block(core.search_jamendo(" ".into(), None)).unwrap().len(),
        2
    );
    assert!(search_url(CLIENT, "").contains("order=popularity_week"));
    assert!(search_url(CLIENT, "a b&c").contains("search=a%20b%26c"));
}

#[test]
fn jamendo_needs_a_client_id_and_says_why_it_refused() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    assert!(matches!(
        block(core.search_jamendo("kellee".into(), None)),
        Err(SoundscapeError::BadAddress { .. })
    ));
    assert!(block(core.set_jamendo_client_id("not valid!".into())).is_err());

    block(core.set_jamendo_client_id("wrong".into())).unwrap();
    let err = block(core.search_jamendo("kellee".into(), None)).unwrap_err();
    assert!(err.to_string().contains("not authorized"), "{err}");

    assert_eq!(
        block(core.set_jamendo_client_id(String::new())).unwrap(),
        None
    );
    assert_eq!(core.jamendo_client_id(), None);
    assert_eq!(
        jamendo::parse_tracks(b"not json").unwrap_err().to_string(),
        "Jamendo's answer can't be read"
    );
}

#[test]
fn a_jamendo_track_plays_before_it_is_added() {
    let (_dir, core) = ready();
    let found = block(core.search_jamendo("kellee".into(), None)).unwrap();
    assert!(!core.in_library(wise()));
    let queue = core.catalogue_queue_tracks(found);
    assert_eq!(queue.len(), 2);
    assert_eq!(queue[0].track_id, wise());
    assert_eq!(queue[0].title, "Wise Up");
    assert_eq!(queue[0].artist, "Kellee Maize");
    assert_eq!(queue[0].recording.kind, RecordingKind::Catalogue);
    assert_eq!(
        queue[0].recording.source,
        AudioSource::Catalogue {
            provider: "Jamendo".into()
        }
    );
    assert!(queue[0].recording.audio_url.starts_with("https://prod-1"));
    assert!(queue[0].recording.rights.is_some());
}

#[test]
fn an_added_jamendo_track_is_in_the_library_playlists_and_favourites_after_a_restart() {
    let (dir, core) = ready();
    let found = block(core.search_jamendo("kellee".into(), None)).unwrap();
    let library = block(core.add_catalogue_tracks(found.clone())).unwrap();
    assert!(core.in_library(wise()));
    let track = library.tracks.iter().find(|t| t.id == wise()).unwrap();
    assert_eq!(track.album.as_deref(), Some("Owning My Throne"));
    assert_eq!(track.genre.as_deref(), Some("Hiphop"));
    assert!(track.artwork_url.is_some());
    assert!(!track.downloaded);
    assert_eq!(library.artists.len(), 2);
    assert_eq!(library.albums.len(), 1, "Quiet Field has no album");

    // Added twice: once, keeping what was saved first.
    let again = block(core.add_catalogue_tracks(found)).unwrap();
    assert_eq!(again.tracks.len(), 2);

    let playlist = block(core.create_playlist("Found".into(), vec![wise(), quiet()])).unwrap();
    block(core.set_favourite(wise(), true)).unwrap();

    drop(core);
    let core = open(&dir);
    let detail = core.playlist(playlist.playlist.id).unwrap();
    let titles: Vec<&str> = detail
        .entries
        .iter()
        .map(|e| e.track.title.as_str())
        .collect();
    assert_eq!(titles, ["Wise Up", "Quiet Field"]);
    assert_eq!(core.collection().favourite_tracks[0].id, wise());

    // The rights stay with the saved track, without asking Jamendo.
    let detail = core.library_track(wise()).unwrap();
    assert_eq!(detail.recordings.len(), 1);
    let rights = detail.recordings[0].rights.clone().unwrap();
    assert_eq!(rights.licence, "CC BY-NC-SA 3.0");
    assert!(rights.may_download);

    // Edits work as on any track.
    let edited = block(core.edit_track(
        wise(),
        soundscape_core::TrackEdit {
            title: Some("Wise Up (live)".into()),
            ..Default::default()
        },
    ))
    .unwrap();
    assert_eq!(edited.track.title, "Wise Up (live)");

    // Removed: gone from the library, kept in the playlist as missing.
    block(core.remove_imported(wise())).unwrap();
    assert!(!core.in_library(wise()));
    let detail = core.playlist(playlist.playlist.id).unwrap();
    assert_eq!(detail.entries.len(), 1);
}

#[test]
fn a_jamendo_track_downloads_only_where_jamendo_allows_it() {
    let (dir, core) = ready();
    let found = block(core.search_jamendo("kellee".into(), None)).unwrap();
    block(core.add_catalogue_tracks(found.clone())).unwrap();
    block(core.set_offline(vec![wise(), quiet()], true)).unwrap();
    let done = block(core.sync_downloads(None)).unwrap();
    assert_eq!(done.downloaded, 1);
    assert_eq!(core.download_state(wise()), DownloadState::Downloaded);
    assert_eq!(
        core.download_state(quiet()),
        DownloadState::Failed {
            reason: "its source doesn't allow downloading".into()
        }
    );

    // It now plays from the phone, wherever it is played from, and is
    // still the same track after a restart.
    drop(core);
    let core = open(&dir);
    let track = core
        .library()
        .tracks
        .into_iter()
        .find(|t| t.id == wise())
        .unwrap();
    assert!(track.downloaded);
    assert!(track.preferred.unwrap().audio_url.starts_with("file://"));
    let queue = core.catalogue_queue_tracks(found);
    assert!(queue[0].recording.audio_url.starts_with("file://"));
    assert!(queue[1].recording.audio_url.starts_with("https://"));
}

/// Answers "success" with no results the first `empty` times, then the
/// results, as Jamendo did live.
struct Flaky {
    empty: usize,
    asked: AtomicUsize,
}

const EMPTY: &str = r#"{"headers": {"status": "success", "code": 0, "error_message": "", "results_count": 0}, "results": []}"#;

impl HttpClient for Flaky {
    fn send(&self, _request: &Request) -> Result<Response, NetError> {
        let n = self.asked.fetch_add(1, Ordering::SeqCst);
        Ok(Response::new(
            200,
            if n < self.empty { EMPTY } else { RESULTS },
        ))
    }
}

#[test]
fn an_empty_answer_is_asked_again_before_it_counts_as_none() {
    let cancel = forge_net::CancelHandle::new();
    let flaky = Flaky {
        empty: 2,
        asked: AtomicUsize::new(0),
    };
    let found = jamendo::search(&flaky, CLIENT, "jazz", &cancel).unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(flaky.asked.load(Ordering::SeqCst), 3);

    let none = Flaky {
        empty: usize::MAX,
        asked: AtomicUsize::new(0),
    };
    assert!(jamendo::search(&none, CLIENT, "zzzz", &cancel)
        .unwrap()
        .is_empty());
    assert_eq!(none.asked.load(Ordering::SeqCst), jamendo::ATTEMPTS);
}

#[test]
fn a_live_answer_reads() {
    // Trimmed from Jamendo's real answer (2026-11-18): album_image without a
    // track image, a by-nc-nd licence.
    let body = r#"{"headers": {"status": "success", "code": 0, "error_message": "", "warnings": "", "results_count": 1,
      "next": "https://api.jamendo.com/v3.0/tracks?offset=2"},
     "results": [{"id": "2034080", "name": "Music Soothe - Meditation Music", "duration": 216, "artist_id": "551562",
      "artist_name": "StimiBeats", "artist_idstr": "Stimi", "album_name": "Impression - Meditation Music",
      "album_id": "522038", "license_ccurl": "http://creativecommons.org/licenses/by-nc-nd/3.0/", "position": 5,
      "releasedate": "2023-06-01",
      "album_image": "https://usercontent.jamendo.com?type=album&id=522038&width=300&trackid=2034080",
      "audio": "https://prod-1.storage.jamendo.com/?trackid=2034080&format=mp32&from=x",
      "audiodownload": "https://prod-1.storage.jamendo.com/download/track/2034080/mp32/", "prourl": "",
      "shorturl": "https://jamen.do/t/2034080", "shareurl": "https://www.jamendo.com/track/2034080",
      "waveform": "{\"peaks\": [13, 14, 24]}", "audiodownload_allowed": true,
      "musicinfo": {"tags": {"genres": ["newage", "ambient"]}}}]}"#;
    let tracks = jamendo::parse_tracks(body.as_bytes()).unwrap();
    let t = &tracks[0];
    assert_eq!(t.id, "jamendo:2034080");
    assert_eq!(t.genre.as_deref(), Some("Newage"));
    assert!(t.artwork_url.as_deref().unwrap().contains("type=album"));
    assert_eq!(t.rights.licence, "CC BY-NC-ND 3.0");
    assert!(t.rights.non_commercial && !t.rights.may_adapt && t.rights.may_download);

    let refused = r#"{"headers":{"status":"failed","code":5,"error_message":"Jamendo Api Invalid Client Id Error: Your credential is not authorized.","warnings":"","results_count":0},"results":[]}"#;
    let err = jamendo::parse_tracks(refused.as_bytes()).unwrap_err();
    assert!(err.to_string().contains("Invalid Client Id"), "{err}");
}

// More from Jamendo: by the same artist, or in the same genre.

const ONE_TRACK: &str = r#"{"headers": {"status": "success", "code": 0, "error_message": "", "results_count": 1},
 "results": [{"id": "2034080", "name": "Music Soothe", "artist_id": "551562", "artist_name": "StimiBeats",
  "audio": "https://prod-1.storage.jamendo.com/?trackid=2034080&format=mp32",
  "license_ccurl": "http://creativecommons.org/licenses/by-nc-nd/3.0/",
  "musicinfo": {"tags": {"genres": ["newage", "ambient"]}}}]}"#;

const BY_ARTIST: &str = r#"{"headers": {"status": "success", "code": 0, "error_message": "", "results_count": 2},
 "results": [
  {"id": "2034080", "name": "Music Soothe", "artist_name": "StimiBeats",
   "audio": "https://prod-1.storage.jamendo.com/?trackid=2034080&format=mp32", "license_ccurl": ""},
  {"id": "2034081", "name": "Calm Water", "artist_name": "StimiBeats",
   "audio": "https://prod-1.storage.jamendo.com/?trackid=2034081&format=mp32", "license_ccurl": ""}
 ]}"#;

fn related() -> StaticClient {
    use jamendo::{tracks_url, Ask};
    StaticClient::new()
        .route(
            Method::Get,
            tracks_url(CLIENT, &Ask::Track("2034080".into())),
            Ok(Response::new(200, ONE_TRACK)),
        )
        .route(
            Method::Get,
            tracks_url(CLIENT, &Ask::Artist("551562".into())),
            Ok(Response::new(200, BY_ARTIST)),
        )
        .route(
            Method::Get,
            tracks_url(CLIENT, &Ask::Genre("newage".into())),
            Ok(Response::new(200, RESULTS)),
        )
}

#[test]
fn more_by_the_artist_leaves_out_the_song_playing() {
    let cancel = forge_net::CancelHandle::new();
    let more = jamendo::more(
        &related(),
        CLIENT,
        "jamendo:2034080",
        jamendo::JamendoMore::Artist,
        &cancel,
    )
    .unwrap();
    assert_eq!(more.name, "StimiBeats");
    let ids: Vec<_> = more.tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["jamendo:2034081"]);
}

#[test]
fn more_in_the_genre_uses_the_first_genre() {
    let cancel = forge_net::CancelHandle::new();
    let more = jamendo::more(
        &related(),
        CLIENT,
        "jamendo:2034080",
        jamendo::JamendoMore::Genre,
        &cancel,
    )
    .unwrap();
    assert_eq!(more.name, "New Age");
    assert_eq!(more.tracks.len(), 2);
}

#[test]
fn more_needs_a_jamendo_song() {
    let cancel = forge_net::CancelHandle::new();
    for id in [
        "file:abc",
        "jamendo:",
        "jamendo:12&x=1",
        "archive:foo/bar.mp3",
    ] {
        let err = jamendo::more(
            &related(),
            CLIENT,
            id,
            jamendo::JamendoMore::Artist,
            &cancel,
        )
        .unwrap_err();
        assert!(
            matches!(err, SoundscapeError::BadAddress { .. }),
            "{id}: {err}"
        );
    }
}

#[test]
fn artist_and_genre_requests() {
    use jamendo::{tracks_url, Ask};
    let artist = tracks_url(CLIENT, &Ask::Artist("551562".into()));
    assert!(
        artist.contains("artist_id=551562&order=popularity_total&type=single%20albumtrack"),
        "{artist}"
    );
    let genre = tracks_url(CLIENT, &Ask::Genre("jazz".into()));
    assert!(
        genre.contains("tags=jazz&order=popularity_week&type=single%20albumtrack"),
        "{genre}"
    );
    assert_eq!(jamendo::clean_tag(" Jazz ").unwrap(), "jazz");
    assert!(jamendo::clean_tag("jazz&order=x").is_err());
    assert!(jamendo::clean_tag("").is_err());
    let genres = jamendo::genres();
    assert!(genres
        .iter()
        .all(|g| jamendo::clean_tag(&g.tag).as_deref() == Ok(g.tag.as_str())));
    assert_eq!(jamendo::genre_name("hiphop"), "Hip-Hop");
    assert_eq!(jamendo::genre_name("darkwave"), "Darkwave");
}
