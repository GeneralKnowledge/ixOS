//! The Internet Archive as a source: search finds items, an item becomes
//! tracks in their best MP3, and those tracks behave like any other once
//! added, against canned answers in the Archive's shapes.

use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::archive::{metadata_url, parse_item, parse_search, search_url};
use soundscape_core::{AudioSource, DownloadState, SoundscapeCore, SoundscapeError};

const SEARCH: &str = r#"{"responseHeader": {"status": 0},
 "response": {"numFound": 3, "start": 0, "docs": [
  {"identifier": "ccr042", "title": "Night Signals", "creator": ["Kalte Ohren", "guest"], "year": 2006,
   "licenseurl": "http://creativecommons.org/licenses/by-nc-sa/2.5/"},
  {"identifier": "gd1977-05-08", "title": "Grateful Dead Live at Barton Hall", "creator": "Grateful Dead",
   "year": "1977"},
  {"identifier": "bad id/../x", "title": "Not an identifier"}
 ]}}"#;

const ITEM: &str = r#"{"created": 1, "dir": "/1/items/ccr042", "files": [
  {"name": "02 Lantern.flac", "source": "original", "format": "Flac", "length": "03:03", "track": "02"},
  {"name": "02 Lantern.mp3", "source": "derivative", "format": "VBR MP3", "original": "02 Lantern.flac",
   "length": "183.4", "title": "Lantern", "track": "02", "creator": "Kalte Ohren", "album": "Night Signals",
   "sha1": "abc"},
  {"name": "02 Lantern_64kb.mp3", "source": "derivative", "format": "64Kbps MP3", "original": "02 Lantern.flac"},
  {"name": "02 Lantern.ogg", "source": "derivative", "format": "Ogg Vorbis", "original": "02 Lantern.flac"},
  {"name": "01 Static Field.mp3", "source": "original", "format": "MP3", "length": "1:02:03", "track": "1/3"},
  {"name": "bonus/hidden track.mp3", "source": "original", "format": "VBR MP3"},
  {"name": "../escape.mp3", "source": "original", "format": "MP3"},
  {"name": "cover.png", "source": "original", "format": "PNG"}
 ],
 "metadata": {"identifier": "ccr042", "title": "Night Signals", "creator": "Kalte Ohren",
   "licenseurl": "http://creativecommons.org/licenses/by-nc-sa/2.5/"}}"#;

fn lantern() -> String {
    "archive:ccr042/02 Lantern.mp3".into()
}

fn archive() -> StaticClient {
    StaticClient::new()
        .route(
            Method::Get,
            search_url("kalte"),
            Ok(Response::new(200, SEARCH)),
        )
        .route(
            Method::Get,
            metadata_url("ccr042"),
            Ok(Response::new(200, ITEM)),
        )
        .route(
            Method::Get,
            metadata_url("gone"),
            Ok(Response::new(200, "{}")),
        )
        .route(
            Method::Get,
            "https://archive.org/download/ccr042/02%20Lantern.mp3",
            Ok(Response::new(200, vec![5u8; 4000])),
        )
}

fn open(dir: &tempfile::TempDir) -> Arc<SoundscapeCore> {
    SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(archive()),
    )
    .unwrap()
}

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

#[test]
fn searching_finds_items_with_their_licence() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let items = block(core.search_archive("kalte".into(), None)).unwrap();
    assert_eq!(items.len(), 2, "a bad identifier is left out");
    assert_eq!(items[0].identifier, "ccr042");
    assert_eq!(items[0].title, "Night Signals");
    assert_eq!(items[0].creator.as_deref(), Some("Kalte Ohren"));
    assert_eq!(items[0].year.as_deref(), Some("2006"));
    assert_eq!(items[0].licence, "CC BY-NC-SA 2.5");
    assert_eq!(
        items[0].artwork_url,
        "https://archive.org/services/img/ccr042"
    );
    assert_eq!(items[1].licence, "Licence not stated");

    let url = search_url("live (1977) \"dead\"");
    assert!(url.contains("mediatype%3A%28audio%29"), "{url}");
    assert!(
        !url.contains("%281977%29"),
        "typed brackets can't change the query: {url}"
    );
    assert!(search_url("").contains("netlabels"));
    assert!(parse_search(br#"{"error": "bad query"}"#).is_err());
}

#[test]
fn an_item_becomes_tracks_in_their_best_mp3_in_order() {
    let tracks = parse_item("ccr042", ITEM.as_bytes()).unwrap();
    let names: Vec<&str> = tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(names, ["Static Field", "Lantern", "hidden track"]);

    let lantern = &tracks[1];
    assert_eq!(lantern.id, "archive:ccr042/02 Lantern.mp3");
    assert_eq!(lantern.provider, "Internet Archive");
    assert_eq!(
        lantern.audio_url,
        "https://archive.org/download/ccr042/02%20Lantern.mp3"
    );
    assert_eq!(lantern.number, Some(2));
    assert_eq!(lantern.duration_seconds, Some(183.4));
    assert_eq!(lantern.artist.as_deref(), Some("Kalte Ohren"));
    assert_eq!(lantern.album.as_deref(), Some("Night Signals"));
    assert_eq!(lantern.rights.licence, "CC BY-NC-SA 2.5");
    assert!(lantern.rights.may_download);
    assert_eq!(
        lantern.rights.page_url.as_deref(),
        Some("https://archive.org/details/ccr042")
    );

    let field = &tracks[0];
    assert_eq!(field.number, Some(1));
    assert_eq!(field.duration_seconds, Some(3723.0));
    assert_eq!(
        field.artist.as_deref(),
        Some("Kalte Ohren"),
        "the item's creator"
    );

    assert!(tracks[2]
        .audio_url
        .ends_with("/ccr042/bonus/hidden%20track.mp3"));
    assert!(tracks.iter().all(|t| !t.audio_url.contains("..")));
}

#[test]
fn a_missing_dark_or_restricted_item() {
    assert!(parse_item("gone", b"{}").unwrap().is_empty());
    let dark = r#"{"is_dark": true, "metadata": {"title": "x"}, "files": [{"name": "a.mp3", "format": "MP3"}]}"#;
    assert!(parse_item("x", dark.as_bytes()).unwrap().is_empty());
    let restricted = r#"{"metadata": {"title": "Lent", "access-restricted-item": "true"},
      "files": [{"name": "a.mp3", "format": "MP3"}]}"#;
    let tracks = parse_item("lent", restricted.as_bytes()).unwrap();
    assert!(!tracks[0].rights.may_download);
    assert_eq!(tracks[0].rights.licence, "Licence not stated");

    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    assert!(block(core.archive_item_tracks("gone".into(), None))
        .unwrap()
        .is_empty());
    assert!(matches!(
        block(core.archive_item_tracks("../etc".into(), None)),
        Err(SoundscapeError::NotInLibrary { .. })
    ));
}

#[test]
fn an_archive_track_joins_the_library_and_downloads() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let tracks = block(core.archive_item_tracks("ccr042".into(), None)).unwrap();
    let library = block(core.add_catalogue_tracks(tracks)).unwrap();
    assert_eq!(library.albums.len(), 1);
    assert_eq!(library.albums[0].track_count, 3);
    let track = library.tracks.iter().find(|t| t.id == lantern()).unwrap();
    assert_eq!(
        track.preferred.as_ref().unwrap().source,
        AudioSource::Catalogue {
            provider: "Internet Archive".into()
        }
    );

    let playlist = block(core.create_playlist("Archive".into(), vec![lantern()])).unwrap();
    block(core.set_offline(vec![lantern()], true)).unwrap();
    block(core.sync_downloads(None)).unwrap();
    assert_eq!(core.download_state(lantern()), DownloadState::Downloaded);

    drop(core);
    let core = open(&dir);
    let detail = core.playlist(playlist.playlist.id).unwrap();
    assert_eq!(detail.entries[0].track.title, "Lantern");
    assert!(detail.entries[0]
        .track
        .preferred
        .as_ref()
        .unwrap()
        .audio_url
        .starts_with("file://"));
}
