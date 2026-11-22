//! Everyday Polish (docs/design.md, S3.5): saved stations, recent
//! searches, Undo for playlists, the library and the queue, and saving the
//! queue as a playlist.

use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::catalogue::{rights_from_licence, CatalogueTrack};
use soundscape_core::library::track_id;
use soundscape_core::queue::{queue_put_back, queue_remove, queue_start};
use soundscape_core::{Feeling, RepeatMode, SoundscapeCore, StationSeed, TrackEdit};

const SERVER: &str = "http://100.64.0.7:8642";

const CATALOG: &str = r#"{"version": 1,
 "artists": [{"id": "a", "name": "A", "artwork": null}, {"id": "b", "name": "B", "artwork": null}],
 "albums": [],
 "tracks": [
  {"slug": "one", "title": "One", "artist": "a", "album": null, "number": null, "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/one", "file": "r/one.mp3", "label": "a", "mtime": 1, "seconds": 100}]},
  {"slug": "two", "title": "Two", "artist": "b", "album": null, "number": null, "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/two", "file": "r/two.mp3", "label": "a", "mtime": 1, "seconds": 100}]},
  {"slug": "three", "title": "Three", "artist": "a", "album": null, "number": null, "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/three", "file": "r/three.mp3", "label": "a", "mtime": 1, "seconds": 100}]},
  {"slug": "four", "title": "Four", "artist": "b", "album": null, "number": null, "artwork": null, "pick": null, "updated": 1,
   "takes": [{"id": "r/four", "file": "r/four.mp3", "label": "a", "mtime": 1, "seconds": 100}]}
 ]}"#;

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

fn open(dir: &tempfile::TempDir) -> Arc<SoundscapeCore> {
    let http = StaticClient::new().route(
        Method::Get,
        format!("{SERVER}/api/catalog"),
        Ok(Response::new(200, CATALOG)),
    );
    let core = SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(http),
    )
    .unwrap();
    block(core.set_listen_server(SERVER.into())).unwrap();
    block(core.refresh_library(false, None)).unwrap();
    for (slug, genre) in [
        ("one", "Jazz"),
        ("two", "jazz"),
        ("three", "Jazz"),
        ("four", "Rock"),
    ] {
        let edit = TrackEdit {
            title: None,
            artist: None,
            album: None,
            number: None,
            genre: Some(genre.into()),
        };
        block(core.edit_track(track_id(slug), edit)).unwrap();
    }
    core
}

fn jamendo(n: u32) -> CatalogueTrack {
    CatalogueTrack {
        id: format!("jamendo:{n}"),
        provider: "Jamendo".into(),
        title: format!("Song {n}"),
        artist: Some("Someone".into()),
        album: None,
        number: None,
        genre: Some("Jazz".into()),
        duration_seconds: Some(120.0),
        artwork_url: None,
        audio_url: format!("https://example.com/{n}.mp3"),
        rights: rights_from_licence(
            Some("http://creativecommons.org/licenses/by/3.0/"),
            true,
            "Song by Someone",
            None,
        ),
        added_ms: 0,
    }
}

#[test]
fn saved_stations_come_first_once_and_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let rock = StationSeed::Genre {
        name: "Rock".into(),
    };
    let song = StationSeed::Track {
        id: track_id("one"),
    };
    block(core.set_station_saved(rock.clone(), "Rock Station".into(), true)).unwrap();
    block(core.set_station_saved(song.clone(), "One Station".into(), true)).unwrap();
    let stations = core.suggested_stations();
    assert_eq!(stations[0].seed, song, "newest saved first");
    assert_eq!(stations[1].seed, rock);
    assert!(stations[0].saved && stations[1].saved);
    assert_eq!(
        stations.iter().filter(|s| s.seed == rock).count(),
        1,
        "a saved station isn't offered twice"
    );
    assert!(core.is_station_saved(rock.clone()));
    drop(core);
    let core = open(&dir);
    block(core.set_station_saved(rock.clone(), String::new(), false)).unwrap();
    assert!(!core.is_station_saved(rock.clone()));
    let stations = core.suggested_stations();
    assert_eq!(stations[0].seed, song);
    assert!(stations.iter().any(|s| s.seed == rock && !s.saved));
}

#[test]
fn recent_searches_keep_the_newest_ten_once_each() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    for n in 0..12 {
        block(core.note_search(format!("query {n}"))).unwrap();
    }
    block(core.note_search("  ".into())).unwrap();
    let found = block(core.note_search("QUERY 5".into())).unwrap();
    assert_eq!(found.recent_searches.len(), 10);
    assert_eq!(found.recent_searches[0], "QUERY 5");
    assert_eq!(found.recent_searches[1], "query 11");
    assert_eq!(
        found
            .recent_searches
            .iter()
            .filter(|q| q.to_lowercase() == "query 5")
            .count(),
        1
    );
    assert!(block(core.clear_recent_searches())
        .unwrap()
        .recent_searches
        .is_empty());
}

#[test]
fn a_deleted_playlist_and_removed_songs_come_back_with_undo() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let ids = vec![track_id("one"), track_id("two"), track_id("three")];
    let list = block(core.create_playlist("Mix".into(), ids.clone())).unwrap();
    let id = list.playlist.id;
    // Remove one, then put the songs back as they were.
    block(core.arrange_playlist(id, vec![0, 2])).unwrap();
    let back = block(core.set_playlist_tracks(id, ids.clone())).unwrap();
    let got: Vec<String> = back.entries.iter().map(|e| e.track.id.clone()).collect();
    assert_eq!(got, ids);
    // Delete, then restore under the same id.
    block(core.delete_playlist(id)).unwrap();
    assert!(core.playlist(id).is_err());
    let collection = block(core.restore_playlist(id)).unwrap();
    assert_eq!(collection.playlists.len(), 1);
    assert_eq!(core.playlist(id).unwrap().entries.len(), 3);
    assert!(block(core.restore_playlist(id)).is_err(), "only once");
}

#[test]
fn a_removed_catalogue_song_comes_back_with_its_edits() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    block(core.add_catalogue_tracks(vec![jamendo(1), jamendo(2)])).unwrap();
    let edit = TrackEdit {
        title: Some("Renamed".into()),
        artist: None,
        album: None,
        number: None,
        genre: None,
    };
    block(core.edit_track("jamendo:1".into(), edit)).unwrap();
    block(core.remove_imported("jamendo:1".into())).unwrap();
    assert!(!core.in_library("jamendo:1".into()));
    // Only the song removed last can come back.
    assert!(block(core.restore_removed("jamendo:2".into())).is_err());
    block(core.restore_removed("jamendo:1".into())).unwrap();
    assert!(core.in_library("jamendo:1".into()));
    assert_eq!(
        core.library_track("jamendo:1".into()).unwrap().track.title,
        "Renamed"
    );
}

#[test]
fn the_queue_saves_as_a_playlist_adding_catalogue_songs_once() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let mut queued = core.catalogue_queue_tracks(vec![jamendo(7)]);
    let mut tracks = core.queue_tracks(vec![track_id("one")]);
    tracks.append(&mut queued);
    tracks.push(tracks[0].clone());
    let list = block(core.save_queue_as_playlist("Radio Find".into(), tracks)).unwrap();
    let got: Vec<String> = list.entries.iter().map(|e| e.track.id.clone()).collect();
    assert_eq!(got, [track_id("one"), "jamendo:7".to_string()]);
    assert!(core.in_library("jamendo:7".into()));
    // A radio find joins the library on its own too (to favourite it).
    let found = core.catalogue_queue_tracks(vec![jamendo(8)]);
    block(core.add_queued_to_library(found)).unwrap();
    assert!(core.in_library("jamendo:8".into()));
}

#[test]
fn a_song_taken_out_of_the_queue_goes_back_where_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let ids = core.library().tracks.into_iter().map(|t| t.id).collect();
    let tracks = core.queue_tracks(ids);
    let queue = queue_start(tracks, 0, false, RepeatMode::Off, 1);
    let index = queue.order[2];
    let removed = queue_remove(queue.clone(), 2);
    assert_eq!(removed.order.len(), queue.order.len() - 1);
    let back = queue_put_back(removed.clone(), index, 2);
    assert_eq!(back.order, queue.order);
    // Moved on past it meanwhile: it comes back right after the current song.
    let mut moved = removed;
    moved.position = 2;
    let back = queue_put_back(moved, index, 2);
    assert_eq!(back.order[3], index);
}

#[test]
fn an_artists_top_songs_lead_with_plays_then_favourites() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let artist = core
        .library()
        .tracks
        .into_iter()
        .find(|t| t.id == track_id("one"))
        .and_then(|t| t.artist_id)
        .unwrap();
    // Artist "a" has One and Three.
    let titles = |core: &SoundscapeCore| -> Vec<String> {
        core.artist_top_tracks(artist.clone(), 5)
            .into_iter()
            .map(|t| t.title)
            .collect()
    };
    assert_eq!(titles(&core), ["One", "Three"]);
    block(core.set_favourite(track_id("three"), true)).unwrap();
    assert_eq!(titles(&core), ["Three", "One"]);
    block(core.record_play(track_id("one"), "r/one".into())).unwrap();
    assert_eq!(titles(&core), ["One", "Three"]);
    block(core.set_feeling(track_id("one"), Some(Feeling::Dislike))).unwrap();
    assert_eq!(titles(&core), ["Three"]);
    assert_eq!(core.artist_top_tracks(artist, 0).len(), 0);
}
