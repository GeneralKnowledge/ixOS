//! One song, several sources: offering merges, merging, and separating,
//! against a canned take server, an imported file and a Jamendo track.

use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::library::track_id;
use soundscape_core::{
    jamendo, AudioSource, DuplicateReason, MusicFile, SoundscapeCore, SoundscapeError,
};

const SERVER: &str = "http://100.64.0.7:8642";

const CATALOG: &str = r#"{"version": 1,
 "artists": [{"id": "after-hours", "name": "AFTER HOURS", "artwork": null},
             {"id": "lowbridge", "name": "LOWBRIDGE", "artwork": null}],
 "albums": [{"id": "after-hours", "title": "AFTER HOURS", "artist": "after-hours", "artwork": null}],
 "tracks": [
  {"slug": "ah-one-more-night", "title": "One More Night", "artist": "after-hours", "album": "after-hours",
   "number": 1, "artwork": null, "pick": "r/night-2", "updated": 300,
   "takes": [
     {"id": "r/night-1", "file": "r/night-1.mp3", "label": "seed 1", "mtime": 100, "seconds": 225, "stars": 5},
     {"id": "r/night-2", "file": "r/night-2.mp3", "label": "seed 2", "mtime": 300, "seconds": 230, "stars": 3}]},
  {"slug": "ah-one-more-night-live", "title": "One More Night", "artist": "after-hours", "album": null,
   "number": null, "artwork": null, "pick": null, "updated": 310,
   "takes": [{"id": "r/live-1", "file": "r/live-1.mp3", "label": "live", "mtime": 310, "seconds": 301}]},
  {"slug": "lb-anchor", "title": "Anchor", "artist": "lowbridge", "album": null, "number": null,
   "artwork": null, "pick": null, "updated": 150,
   "takes": [{"id": "r/lb-1", "file": "r/lb-1.mp3", "label": "a", "mtime": 10, "stars": 2}]}
 ]}"#;

const JAMENDO_ANCHOR: &str = r#"{"headers": {"status": "success", "code": 0, "error_message": ""},
 "results": [{"id": "555", "name": "Anchor!", "artist_name": "Lowbridge", "duration": 200,
  "audio": "https://prod-1.storage.jamendo.com/?trackid=555&format=mp32",
  "license_ccurl": "http://creativecommons.org/licenses/by-nc-sa/3.0/", "audiodownload_allowed": true}]}"#;

fn server() -> StaticClient {
    StaticClient::new()
        .route(
            Method::Get,
            format!("{SERVER}/audio/r/lb-1.mp3"),
            Ok(Response::new(200, vec![7u8; 300])),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/catalog"),
            Ok(Response::new(200, CATALOG)),
        )
        .route(
            Method::Post,
            format!("{SERVER}/api/pick"),
            Ok(Response::new(200, r#"{"ok": true}"#)),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=ah-one-more-night"),
            Ok(Response::new(
                200,
                r#"{"slug": "ah-one-more-night", "title": "One More Night", "pick": "r/night-1",
                 "takes": [{"id": "r/night-1", "file": "r/night-1.mp3", "label": "seed 1", "mtime": 100, "stars": 5},
                           {"id": "r/night-2", "file": "r/night-2.mp3", "label": "seed 2", "mtime": 300, "stars": 3}]}"#,
            )),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=lb-anchor"),
            Ok(Response::new(
                200,
                r#"{"slug": "lb-anchor", "title": "Anchor", "pick": "r/lb-1",
                 "takes": [{"id": "r/lb-1", "file": "r/lb-1.mp3", "label": "a", "mtime": 10, "stars": 2}]}"#,
            )),
        )
}

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

fn open(dir: &tempfile::TempDir) -> Arc<SoundscapeCore> {
    SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(server()),
    )
    .unwrap()
}

/// The take server's library, your own file of One More Night, and
/// Jamendo's Anchor. Returns the imported file's id and Jamendo's.
fn ready(dir: &tempfile::TempDir) -> (Arc<SoundscapeCore>, String, String) {
    let core = open(dir);
    block(core.set_listen_server(SERVER.into())).unwrap();
    block(core.refresh_library(false, None)).unwrap();
    let picked = dir.path().join("picked.m4a");
    std::fs::write(&picked, b"my own copy").unwrap();
    let file = MusicFile {
        path: picked.to_string_lossy().into_owned(),
        file_name: "One More Night.m4a".into(),
        title: Some("One more night".into()),
        artist: Some("After Hours".into()),
        album: None,
        number: None,
        genre: None,
        duration_seconds: Some(228.4),
        artwork_path: None,
    };
    let imported = block(core.import_music(vec![file])).unwrap().added[0].clone();
    let anchor = jamendo::parse_tracks(JAMENDO_ANCHOR.as_bytes()).unwrap();
    let jamendo_id = anchor[0].id.clone();
    block(core.add_catalogue_tracks(anchor)).unwrap();
    (core, imported, jamendo_id)
}

#[test]
fn the_same_song_from_several_sources_is_offered_not_merged() {
    let dir = tempfile::tempdir().unwrap();
    let (core, imported, jamendo_id) = ready(&dir);
    let night = track_id("ah-one-more-night");
    let groups = core.duplicate_songs();
    let ids: Vec<Vec<&str>> = groups
        .iter()
        .map(|g| g.tracks.iter().map(|t| t.id.as_str()).collect())
        .collect();
    assert!(
        ids.contains(&vec![night.as_str(), imported.as_str()]),
        "server first, your file after: {ids:?}"
    );
    assert!(
        ids.contains(&vec![track_id("lb-anchor").as_str(), jamendo_id.as_str()]),
        "a length unknown on one side still matches: {ids:?}"
    );
    // The live version is minutes longer: another song.
    assert!(!ids
        .iter()
        .flatten()
        .any(|id| *id == track_id("ah-one-more-night-live")));
    assert!(groups
        .iter()
        .all(|g| g.reason == DuplicateReason::SameDetails));
    // Offering changes nothing.
    assert_eq!(core.library().tracks.len(), 5);
}

#[test]
fn merging_makes_one_song_whose_recordings_come_from_every_source() {
    let dir = tempfile::tempdir().unwrap();
    let (core, imported, _) = ready(&dir);
    let night = track_id("ah-one-more-night");

    block(core.set_favourite(imported.clone(), true)).unwrap();
    let list = block(core.create_playlist("Late".into(), vec![imported.clone()])).unwrap();
    block(core.record_play(imported.clone(), imported.clone())).unwrap();
    block(core.set_offline(vec![imported.clone()], true)).unwrap();

    let detail = block(core.merge_songs(night.clone(), vec![imported.clone()])).unwrap();
    assert_eq!(detail.track.recording_count, 3);
    assert_eq!(detail.track.merged.len(), 1);
    assert_eq!(detail.track.merged[0].source, "Your file");
    // Your own file plays.
    let plays = detail.track.preferred.as_ref().unwrap();
    assert!(matches!(plays.source, AudioSource::LocalFile { .. }));
    assert_eq!(plays.merged_from.as_deref(), Some(imported.as_str()));
    assert!(detail.track.downloaded);
    assert_eq!(detail.recordings.len(), 3);
    let preferred: Vec<_> = detail.recordings.iter().filter(|r| r.preferred).collect();
    assert_eq!(preferred.len(), 1);
    assert_eq!(preferred[0].merged_from.as_deref(), Some(imported.as_str()));

    // The file leaves the lists; what pointed at it points at the song.
    let library = core.library();
    assert_eq!(library.tracks.len(), 4);
    assert!(!library.tracks.iter().any(|t| t.id == imported));
    let collection = core.collection();
    assert_eq!(collection.favourite_tracks[0].id, night);
    assert_eq!(collection.recently_played[0].track.id, night);
    assert_eq!(
        core.playlist(list.playlist.id).unwrap().entries[0].track.id,
        night
    );
    let queue = core.queue_tracks(vec![night.clone()]);
    assert!(queue[0].recording.audio_url.starts_with("file://"));
    assert!(core
        .duplicate_songs()
        .iter()
        .all(|g| !g.tracks.iter().any(|t| t.id == night)));

    // Kept after a restart.
    drop(core);
    let core = open(&dir);
    let again = core.library_track(night.clone()).unwrap();
    assert_eq!(again.recordings.len(), 3);
    assert_eq!(core.library().tracks.len(), 4);

    // Separating brings the file back as it was.
    let library = block(core.separate_song(imported.clone())).unwrap();
    assert_eq!(library.tracks.len(), 5);
    let night_alone = core.library_track(night).unwrap();
    assert_eq!(night_alone.recordings.len(), 2);
    assert!(night_alone.track.merged.is_empty());
    assert!(!night_alone.track.downloaded);
}

#[test]
fn a_take_merged_into_a_catalogue_song_is_still_picked_on_its_server() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _, jamendo_id) = ready(&dir);
    let anchor = track_id("lb-anchor");
    let detail = block(core.merge_songs(jamendo_id.clone(), vec![anchor.clone()])).unwrap();
    assert_eq!(detail.recordings.len(), 2);
    assert_eq!(detail.track.merged[0].source, "Take server");
    let take = detail.recordings.iter().find(|r| r.id == "r/lb-1").unwrap();
    assert_eq!(take.merged_from.as_deref(), Some(anchor.as_str()));
    // Jamendo's stream still plays: no file of your own.
    assert_eq!(
        detail.track.preferred.as_ref().unwrap().id,
        jamendo_id,
        "the kept song's own recording plays"
    );
    let picked = block(core.choose_recording(jamendo_id.clone(), "r/lb-1".into())).unwrap();
    assert_eq!(picked.track.id, jamendo_id);
    // The chosen take is what the song plays now, and stays so.
    let plays = picked.track.preferred.as_ref().unwrap();
    assert_eq!(plays.id, "r/lb-1");
    assert_eq!(plays.merged_from.as_deref(), Some(anchor.as_str()));
    let queued = core.queue_tracks(vec![jamendo_id.clone()]);
    assert_eq!(queued[0].recording.id, "r/lb-1");
    drop(picked);
    let core = {
        drop(core);
        open(&dir)
    };

    // Downloaded under the song it came from, and kept on the next pass.
    block(core.set_offline(vec![jamendo_id.clone()], true)).unwrap();
    block(core.sync_downloads(None)).unwrap();
    let again = block(core.sync_downloads(None)).unwrap();
    let state = &again
        .tracks
        .iter()
        .find(|t| t.track.id == jamendo_id)
        .unwrap()
        .state;
    assert_eq!(*state, soundscape_core::DownloadState::Downloaded);
    let detail = core.library_track(jamendo_id.clone()).unwrap();
    assert!(detail.track.downloaded);
    assert!(detail
        .track
        .preferred
        .unwrap()
        .audio_url
        .starts_with("file://"));

    // One playable take on its own.
    let one = core
        .queue_recording(jamendo_id.clone(), "r/lb-1".into())
        .unwrap();
    assert_eq!(one.recording.label, "a");

    // Separating the take server's song undoes the choice.
    block(core.separate_song(anchor.clone())).unwrap();
    let alone = core.library_track(jamendo_id.clone()).unwrap();
    assert_eq!(alone.track.preferred.unwrap().id, jamendo_id);
}

#[test]
fn songs_kept_apart_are_not_offered_again() {
    let dir = tempfile::tempdir().unwrap();
    let (core, imported, _) = ready(&dir);
    let night = track_id("ah-one-more-night");
    let before = core.duplicate_songs().len();
    block(core.keep_songs_apart(vec![imported.clone(), night.clone()])).unwrap();
    let after = core.duplicate_songs();
    assert_eq!(after.len(), before - 1);
    assert!(!after
        .iter()
        .any(|g| g.tracks.iter().any(|t| t.id == imported)));
}

#[test]
fn merges_that_make_no_sense_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (core, imported, jamendo_id) = ready(&dir);
    let night = track_id("ah-one-more-night");
    assert!(matches!(
        block(core.merge_songs(night.clone(), vec!["file:nothing".into()])),
        Err(SoundscapeError::NotInLibrary { .. })
    ));
    assert!(block(core.merge_songs(night.clone(), vec![])).is_err());
    block(core.merge_songs(night.clone(), vec![imported.clone()])).unwrap();
    // Into a song that is now part of another: refused.
    assert!(matches!(
        block(core.merge_songs(imported.clone(), vec![jamendo_id.clone()])),
        Err(SoundscapeError::NotInLibrary { .. })
    ));
    // Merging the kept song into a third moves its merged song along.
    block(core.merge_songs(jamendo_id.clone(), vec![night.clone()])).unwrap();
    let all = core.library_track(jamendo_id).unwrap();
    assert_eq!(all.recordings.len(), 4);
    assert_eq!(all.track.merged.len(), 2);
}

#[test]
fn an_unknown_length_never_ties_two_different_lengths_together() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let file = |name: &str, seconds: Option<f64>| {
        let path = dir.path().join(name);
        std::fs::write(&path, name.as_bytes()).unwrap();
        MusicFile {
            path: path.to_string_lossy().into_owned(),
            file_name: name.into(),
            title: Some("Wise Up".into()),
            artist: Some("Someone".into()),
            album: None,
            number: None,
            genre: None,
            duration_seconds: seconds,
            artwork_path: None,
        }
    };
    block(core.import_music(vec![
        file("a.mp3", None),
        file("b.mp3", Some(200.0)),
        file("c.mp3", Some(400.0)),
    ]))
    .unwrap();
    assert!(
        core.duplicate_songs().is_empty(),
        "{:?}",
        core.duplicate_songs()
    );

    // With one known length, the unknown one joins it.
    block(core.import_music(vec![file("d.mp3", Some(201.0))])).unwrap();
    let groups = core.duplicate_songs();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].tracks.len(), 2, "200 s and 201 s only");
}

#[test]
fn a_merge_whose_later_saves_failed_is_finished_on_opening() {
    let dir = tempfile::tempdir().unwrap();
    let (core, imported, _) = ready(&dir);
    let night = track_id("ah-one-more-night");
    block(core.set_favourite(imported.clone(), true)).unwrap();
    block(core.set_offline(vec![imported.clone()], true)).unwrap();
    drop(core);

    // As if the library saved the merge and nothing after it did.
    let path = dir.path().join("data").join("library.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let value = json.get_mut("data").expect("a versioned document");
    value["merged"] = serde_json::json!({ imported.clone(): night.clone() });
    std::fs::write(&path, json.to_string()).unwrap();

    let core = open(&dir);
    assert_eq!(core.collection().favourite_tracks[0].id, night);
    assert!(core.offline().tracks.iter().any(|t| t.track.id == night));
}

#[test]
fn choosing_the_kept_songs_own_take_beats_your_merged_file() {
    let dir = tempfile::tempdir().unwrap();
    let (core, imported, _) = ready(&dir);
    let night = track_id("ah-one-more-night");
    let merged = block(core.merge_songs(night.clone(), vec![imported.clone()])).unwrap();
    assert!(matches!(
        merged.track.preferred.unwrap().source,
        AudioSource::LocalFile { .. }
    ));
    let chosen = block(core.choose_recording(night.clone(), "r/night-1".into())).unwrap();
    assert_eq!(chosen.track.preferred.as_ref().unwrap().id, "r/night-1");
    assert_eq!(
        core.queue_tracks(vec![night.clone()])[0].recording.id,
        "r/night-1"
    );
}

#[test]
fn a_saved_queue_keeps_a_song_merged_since() {
    let dir = tempfile::tempdir().unwrap();
    let (core, imported, _) = ready(&dir);
    let night = track_id("ah-one-more-night");
    let tracks = core.queue_tracks(vec![imported.clone()]);
    assert!(tracks[0].recording.audio_url.starts_with("file:"));
    let queue =
        soundscape_core::queue::queue_start(tracks, 0, false, soundscape_core::RepeatMode::Off, 1);
    block(core.save_playback(Some(soundscape_core::SavedPlayback {
        queue,
        seconds: 12.0,
        saved_ms: 1,
    })))
    .unwrap();
    block(core.merge_songs(night.clone(), vec![imported.clone()])).unwrap();
    drop(core);

    let core = open(&dir);
    let saved = core.saved_playback().expect("the queue survives");
    assert_eq!(saved.queue.tracks.len(), 1);
    assert_eq!(saved.queue.tracks[0].track_id, night);
    assert!(saved.queue.tracks[0]
        .recording
        .audio_url
        .starts_with("file:"));
}

#[test]
fn a_catalogue_track_merged_into_your_file_plays_your_file() {
    let dir = tempfile::tempdir().unwrap();
    let (core, imported, jamendo_id) = ready(&dir);
    let anchor = jamendo::parse_tracks(JAMENDO_ANCHOR.as_bytes()).unwrap();
    // Discover still hands over Jamendo's track; once merged into your
    // file, the queue plays the file.
    block(core.merge_songs(imported.clone(), vec![jamendo_id.clone()])).unwrap();
    let queued = core.catalogue_queue_tracks(anchor);
    assert_eq!(queued[0].track_id, imported);
    assert!(queued[0].recording.audio_url.starts_with("file:"));
}
