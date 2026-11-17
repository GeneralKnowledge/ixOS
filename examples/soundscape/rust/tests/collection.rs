//! Favourites, playlists and history: kept by library id, shown against the
//! library as it is now, and kept across a restart.

use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::collection::{CollectionFile, MAX_HISTORY};
use soundscape_core::library::{album_id, artist_id, track_id};
use soundscape_core::{counts_as_played, SoundscapeCore, SoundscapeError};

const SERVER: &str = "http://100.64.0.7:8642";

const CATALOG: &str = r#"{"version": 1,
 "artists": [{"id": "after-hours", "name": "AFTER HOURS", "artwork": null}],
 "albums": [{"id": "after-hours", "title": "AFTER HOURS", "artist": "after-hours", "artwork": "/art/after-hours/album.jpg"}],
 "tracks": [
  {"slug": "one", "title": "One", "artist": "after-hours", "album": "after-hours", "number": 1,
   "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/one", "file": "r/one.mp3", "label": "a", "mtime": 1, "seconds": 100}]},
  {"slug": "two", "title": "Two", "artist": "after-hours", "album": "after-hours", "number": 2,
   "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/two", "file": "r/two.mp3", "label": "a", "mtime": 1, "seconds": 50}]},
  {"slug": "three", "title": "Three", "artist": null, "album": null, "number": null,
   "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/three", "file": "r/three.mp3", "label": "a", "mtime": 1}]}
 ]}"#;

/// The same catalogue without "Two".
const CATALOG_WITHOUT_TWO: &str = r#"{"version": 1,
 "artists": [{"id": "after-hours", "name": "AFTER HOURS", "artwork": null}],
 "albums": [{"id": "after-hours", "title": "AFTER HOURS", "artist": "after-hours", "artwork": null}],
 "tracks": [
  {"slug": "one", "title": "One", "artist": "after-hours", "album": "after-hours", "number": 1,
   "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/one", "file": "r/one.mp3", "label": "a", "mtime": 1, "seconds": 100}]},
  {"slug": "three", "title": "Three", "artist": null, "album": null, "number": null,
   "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/three", "file": "r/three.mp3", "label": "a", "mtime": 1}]}
 ]}"#;

fn server(catalog: &'static str) -> StaticClient {
    StaticClient::new().route(
        Method::Get,
        format!("{SERVER}/api/catalog"),
        Ok(Response::new(200, catalog)),
    )
}

fn open(dir: &tempfile::TempDir, catalog: &'static str) -> Arc<SoundscapeCore> {
    let core = SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(server(catalog)),
    )
    .unwrap();
    block(core.set_listen_server(SERVER.into())).unwrap();
    block(core.refresh_library(false, None)).unwrap();
    core
}

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

fn titles(tracks: impl IntoIterator<Item = soundscape_core::LibraryTrack>) -> Vec<String> {
    tracks.into_iter().map(|t| t.title).collect()
}

#[test]
fn favourites_cover_songs_albums_and_artists_and_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, CATALOG);
    let album = album_id(Some("AFTER HOURS"), "AFTER HOURS");
    block(core.set_favourite(track_id("one"), true)).unwrap();
    block(core.set_favourite(track_id("three"), true)).unwrap();
    block(core.set_favourite(album.clone(), true)).unwrap();
    block(core.set_favourite(artist_id("AFTER HOURS"), true)).unwrap();
    // twice is the same as once
    block(core.set_favourite(track_id("one"), true)).unwrap();

    let c = core.collection();
    let mut songs = titles(c.favourite_tracks.clone());
    songs.sort();
    assert_eq!(songs, ["One", "Three"]);
    assert_eq!(c.favourite_albums.len(), 1);
    assert_eq!(c.favourite_artists.len(), 1);
    assert!(core.is_favourite(album.clone()));

    block(core.set_favourite(track_id("three"), false)).unwrap();
    drop(core);
    let core = open(&dir, CATALOG);
    assert_eq!(titles(core.collection().favourite_tracks), ["One"]);
    assert!(!core.is_favourite(track_id("three")));
}

#[test]
fn playlists_keep_their_order_and_hide_songs_the_library_lost() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, CATALOG);
    let list = block(core.create_playlist(
        "  Late drive ".into(),
        vec![track_id("two"), track_id("one")],
    ))
    .unwrap();
    assert_eq!(list.playlist.name, "Late drive");
    let id = list.playlist.id;
    // the same song can be in twice
    let list = block(core.add_to_playlist(id, vec![track_id("three"), track_id("one")])).unwrap();
    assert_eq!(
        titles(list.entries.iter().map(|e| e.track.clone())),
        ["Two", "One", "Three", "One"]
    );
    assert_eq!(list.playlist.duration_seconds, 50.0 + 100.0 + 100.0);
    assert_eq!(
        list.playlist.artwork_urls,
        [format!("{SERVER}/art/after-hours/album.jpg")],
        "one picture, however many songs share it"
    );

    // move "Three" to the top and drop the second "One"
    let list = block(core.arrange_playlist(id, vec![2, 0, 1])).unwrap();
    assert_eq!(
        titles(list.entries.iter().map(|e| e.track.clone())),
        ["Three", "Two", "One"]
    );

    let renamed = block(core.rename_playlist(id, "Night drive".into())).unwrap();
    assert_eq!(renamed.playlist.name, "Night drive");
    assert!(matches!(
        block(core.rename_playlist(id, "   ".into())),
        Err(SoundscapeError::EmptyName)
    ));

    // "Two" leaves the library: hidden, counted, and back when it returns
    drop(core);
    let core = open(&dir, CATALOG_WITHOUT_TWO);
    let list = core.playlist(id).unwrap();
    assert_eq!(
        titles(list.entries.iter().map(|e| e.track.clone())),
        ["Three", "One"]
    );
    assert_eq!(list.playlist.missing_count, 1);
    // removing "Three" while "Two" is gone keeps "Two", at the end
    block(core.arrange_playlist(id, vec![2])).unwrap();
    drop(core);
    let core = open(&dir, CATALOG);
    let list = core.playlist(id).unwrap();
    assert_eq!(
        titles(list.entries.iter().map(|e| e.track.clone())),
        ["One", "Two"]
    );

    let after = block(core.delete_playlist(id)).unwrap();
    assert!(after.playlists.is_empty());
    assert!(matches!(
        core.playlist(id),
        Err(SoundscapeError::NotInLibrary { .. })
    ));
}

#[test]
fn recently_played_lists_each_song_once_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, CATALOG);
    for slug in ["one", "two", "one", "three"] {
        block(core.record_play(track_id(slug), format!("r/{slug}"))).unwrap();
    }
    let recent = core.collection().recently_played;
    assert_eq!(
        titles(recent.into_iter().map(|r| r.track)),
        ["Three", "One", "Two"]
    );
    let cleared = block(core.clear_history()).unwrap();
    assert!(cleared.recently_played.is_empty());
}

#[test]
fn history_keeps_only_the_newest_plays() {
    let mut file = CollectionFile::default();
    for i in 0..(MAX_HISTORY + 10) {
        file.record_play(&format!("t{i}"), "r", i as i64);
    }
    assert_eq!(file.history.len(), MAX_HISTORY);
    assert_eq!(file.history[0].track_id, "t10");
}

#[test]
fn a_play_counts_after_thirty_seconds_or_half_a_short_song() {
    assert!(!counts_as_played(29.0, Some(200.0)));
    assert!(counts_as_played(30.0, Some(200.0)));
    assert!(counts_as_played(21.0, Some(40.0)));
    assert!(!counts_as_played(19.0, Some(40.0)));
    assert!(counts_as_played(30.0, None));
}
