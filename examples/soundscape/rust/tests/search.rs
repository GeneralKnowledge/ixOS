//! One search for everything: the library answers at once, and Jamendo and
//! the Internet Archive answer together, each for itself, against canned
//! answers in their shapes.

use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::{archive, jamendo, SoundscapeCore};

const CLIENT: &str = "abc123";

const JAMENDO: &str = r#"{"headers": {"status": "success", "code": 0, "error_message": "", "results_count": 2},
 "results": [
  {"id": "1", "name": "Night Bus", "duration": 183, "artist_name": "Ana Lu", "album_name": "Late",
   "audio": "https://prod-1.storage.jamendo.com/?trackid=1&format=mp32",
   "license_ccurl": "http://creativecommons.org/licenses/by/3.0/", "audiodownload_allowed": true},
  {"id": "2", "name": "Night Train", "duration": 200, "artist_name": "Ana Lu", "album_name": "Late",
   "audio": "https://prod-1.storage.jamendo.com/?trackid=2&format=mp32",
   "license_ccurl": "http://creativecommons.org/licenses/by/3.0/", "audiodownload_allowed": true}
 ]}"#;

const ARCHIVE: &str = r#"{"response": {"numFound": 1, "docs": [
  {"identifier": "night042", "title": "Night Signals", "creator": "Kalte Ohren", "year": "2006"}]}}"#;

fn http(archive_up: bool) -> StaticClient {
    let client = StaticClient::new().route(
        Method::Get,
        jamendo::search_url(CLIENT, "night"),
        Ok(Response::new(200, JAMENDO)),
    );
    client.route(
        Method::Get,
        archive::search_url("night"),
        if archive_up {
            Ok(Response::new(200, ARCHIVE))
        } else {
            Ok(Response::new(502, "bad gateway"))
        },
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

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

#[test]
fn both_catalogues_answer_and_the_library_keeps_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, http(true));
    block(core.set_jamendo_client_id(CLIENT.into())).unwrap();

    let found = block(core.search_catalogues(" night ".into(), None)).unwrap();
    assert_eq!(found.jamendo.len(), 2);
    assert_eq!(found.archive.len(), 1);
    assert_eq!(found.archive[0].identifier, "night042");
    assert_eq!(found.jamendo_problem, None);
    assert_eq!(found.archive_problem, None);

    // A track added to the library shows there, not again under Jamendo.
    let bus = found.jamendo[0].clone();
    block(core.add_catalogue_tracks(vec![bus.clone()])).unwrap();
    let again = block(core.search_catalogues("night".into(), None)).unwrap();
    assert_eq!(again.jamendo.len(), 1);
    assert_eq!(again.jamendo[0].title, "Night Train");
    let library = core.search_library("night".into());
    assert_eq!(library.tracks.len(), 1);
    assert_eq!(library.tracks[0].id, bus.id);
}

#[test]
fn each_catalogue_answers_for_itself() {
    let dir = tempfile::tempdir().unwrap();
    // No client ID: Jamendo isn't asked and says why; the Archive is down.
    let core = open(&dir, http(false));
    let found = block(core.search_catalogues("night".into(), None)).unwrap();
    assert!(found.jamendo.is_empty());
    assert!(found.jamendo_problem.unwrap().contains("client ID"));
    assert!(found.archive.is_empty());
    assert!(found.archive_problem.unwrap().contains("502"));

    // With the ID, Jamendo still answers while the Archive is down.
    block(core.set_jamendo_client_id(CLIENT.into())).unwrap();
    let found = block(core.search_catalogues("night".into(), None)).unwrap();
    assert_eq!(found.jamendo.len(), 2);
    assert!(found.archive_problem.is_some());
}

#[test]
fn an_empty_search_asks_nothing() {
    let dir = tempfile::tempdir().unwrap();
    // A client with no routes fails any request it is given.
    let core = open(&dir, StaticClient::new());
    block(core.set_jamendo_client_id(CLIENT.into())).unwrap();
    let found = block(core.search_catalogues("   ".into(), None)).unwrap();
    assert_eq!(found, Default::default());
}

#[test]
fn a_stopped_search_is_not_a_catalogue_problem() {
    let cancel = forge_net::CancelHandle::new();
    cancel.cancel();
    let result = soundscape_core::search::search_catalogues(
        &http(true),
        Some(CLIENT),
        "night",
        &|_| false,
        &cancel,
    );
    assert!(result.is_err());
}
