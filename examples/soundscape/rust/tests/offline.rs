//! Offline: songs kept on the phone, and the person's own music files,
//! against canned take-server answers.

use std::path::Path;
use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::library::{album_id, track_id};
use soundscape_core::offline::{extension_of, plan, OfflineFile};
use soundscape_core::{DownloadState, MusicFile, SoundscapeCore, SoundscapeError, TrackEdit};

const SERVER: &str = "http://100.64.0.7:8642";

const CATALOG: &str = r#"{"version": 1,
 "artists": [{"id": "after-hours", "name": "AFTER HOURS"}],
 "albums": [{"id": "after-hours", "title": "AFTER HOURS", "artist": "after-hours"}],
 "tracks": [
  {"slug": "ah-one-more-night", "title": "One More Night", "artist": "after-hours", "album": "after-hours",
   "number": 1, "pick": "r/night-2", "updated": 300,
   "takes": [
     {"id": "r/night-1", "file": "r/night-1.mp3", "label": "seed 1", "mtime": 100, "stars": 5},
     {"id": "r/night-2", "file": "r/night-2.mp3", "label": "seed 2", "mtime": 300, "stars": 3}]},
  {"slug": "ah-everybody-knows", "title": "Everybody Knows", "artist": "after-hours", "album": "after-hours",
   "number": 2, "pick": null, "updated": 200,
   "takes": [{"id": "r/ek-1", "file": "r/ek-1.wav", "label": "seed 9", "mtime": 200}]}
 ]}"#;

const NIGHT_PICKED_1: &str = r#"{"slug": "ah-one-more-night", "title": "One More Night", "actTitle": "AFTER HOURS",
 "pick": "r/night-1",
 "takes": [{"id": "r/night-1", "file": "r/night-1.mp3", "label": "seed 1", "mtime": 100, "stars": 5},
           {"id": "r/night-2", "file": "r/night-2.mp3", "label": "seed 2", "mtime": 300, "stars": 3}]}"#;

fn night() -> String {
    track_id("ah-one-more-night")
}

fn knows() -> String {
    track_id("ah-everybody-knows")
}

fn audio(file: &str) -> String {
    format!("{SERVER}/audio/{file}")
}

/// A server with the catalogue, both night takes and Everybody Knows.
fn server() -> StaticClient {
    StaticClient::new()
        .route(
            Method::Get,
            format!("{SERVER}/api/catalog"),
            Ok(Response::new(200, CATALOG)),
        )
        .route(
            Method::Get,
            audio("r/night-1.mp3"),
            Ok(Response::new(200, vec![1u8; 1000])),
        )
        .route(
            Method::Get,
            audio("r/night-2.mp3"),
            Ok(Response::new(200, vec![2u8; 2000])),
        )
        .route(
            Method::Get,
            audio("r/ek-1.wav"),
            Ok(Response::new(200, vec![3u8; 500])),
        )
        .route(
            Method::Post,
            format!("{SERVER}/api/pick"),
            Ok(Response::new(200, "{}")),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=ah-one-more-night"),
            Ok(Response::new(200, NIGHT_PICKED_1)),
        )
}

fn open(dir: &tempfile::TempDir, http: StaticClient) -> Arc<SoundscapeCore> {
    SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(http),
    )
    .unwrap()
}

fn ready(http: StaticClient) -> (tempfile::TempDir, Arc<SoundscapeCore>) {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir, http);
    block(core.set_listen_server(SERVER.into())).unwrap();
    block(core.refresh_library(false, None)).unwrap();
    (dir, core)
}

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn downloads(dir: &tempfile::TempDir) -> Vec<String> {
    files_in(&dir.path().join("data/downloads"))
}

#[test]
fn a_downloaded_song_plays_from_the_phone_after_a_restart() {
    let (dir, core) = ready(server());
    assert_eq!(core.download_state(night()), DownloadState::NotDownloaded);

    let chosen = block(core.set_offline(vec![night()], true)).unwrap();
    assert_eq!(chosen.pending, 1);
    assert_eq!(chosen.tracks[0].state, DownloadState::Waiting);

    let done = block(core.sync_downloads(None)).unwrap();
    assert_eq!((done.downloaded, done.pending, done.failed), (1, 0, 0));
    assert_eq!(done.bytes, 2000, "the preferred take, seed 2");
    assert!(!done.syncing);
    assert_eq!(downloads(&dir), vec!["download-1.mp3"]);

    let track = core
        .library()
        .tracks
        .into_iter()
        .find(|t| t.id == night())
        .unwrap();
    assert!(track.downloaded);
    let url = track.preferred.as_ref().unwrap().audio_url.clone();
    assert!(url.starts_with("file://"), "{url}");
    assert_eq!(core.queue_tracks(vec![night()])[0].recording.audio_url, url);
    // the other take still streams
    let detail = core.library_track(night()).unwrap();
    let other = detail
        .recordings
        .iter()
        .find(|r| r.id == "r/night-1")
        .unwrap();
    assert_eq!(other.audio_url, audio("r/night-1.mp3"));

    drop(core);

    // With no server at all, it is still there and still plays locally.
    let again = open(&dir, StaticClient::new());
    assert_eq!(again.download_state(night()), DownloadState::Downloaded);
    let track = again
        .library()
        .tracks
        .into_iter()
        .find(|t| t.id == night())
        .unwrap();
    assert_eq!(track.preferred.unwrap().audio_url, url);
    let summary = block(again.sync_downloads(None)).unwrap();
    assert_eq!(summary.downloaded, 1, "nothing to fetch, nothing lost");
}

#[test]
fn a_failed_download_does_not_stop_the_others_and_is_tried_again() {
    // Everybody Knows answers 500.
    let http = server().route(
        Method::Get,
        audio("r/ek-1.wav"),
        Ok(Response::new(500, "busy")),
    );
    let (dir, core) = ready(http);
    block(core.set_offline(vec![knows(), night()], true)).unwrap();
    let done = block(core.sync_downloads(None)).unwrap();
    assert_eq!((done.downloaded, done.failed), (1, 1));
    assert!(matches!(
        core.download_state(knows()),
        DownloadState::Failed { .. }
    ));
    assert_eq!(core.download_state(night()), DownloadState::Downloaded);
    assert_eq!(downloads(&dir), vec!["download-1.mp3"], "no half file kept");

    // The next pass (here with the server working) gets it.
    drop(core);
    let core = open(&dir, server());
    let done = block(core.sync_downloads(None)).unwrap();
    assert_eq!((done.downloaded, done.failed), (2, 0));
    assert_eq!(downloads(&dir), vec!["download-1.mp3", "download-2.wav"]);
}

#[test]
fn picking_another_take_replaces_the_download() {
    let (dir, core) = ready(server());
    block(core.set_offline(vec![night()], true)).unwrap();
    block(core.sync_downloads(None)).unwrap();
    assert_eq!(downloads(&dir), vec!["download-1.mp3"]);

    block(core.choose_recording(night(), "r/night-1".into())).unwrap();
    // The new pick isn't on the phone yet.
    assert_eq!(core.download_state(night()), DownloadState::Waiting);
    let done = block(core.sync_downloads(None)).unwrap();
    assert_eq!(done.downloaded, 1);
    assert_eq!(done.bytes, 1000, "seed 1 now");
    assert_eq!(downloads(&dir), vec!["download-2.mp3"], "the old take went");
}

#[test]
fn taking_songs_off_the_phone_deletes_their_files() {
    let (dir, core) = ready(server());
    block(core.set_offline(vec![night(), knows()], true)).unwrap();
    block(core.sync_downloads(None)).unwrap();
    assert_eq!(downloads(&dir).len(), 2);

    block(core.set_offline(vec![knows()], false)).unwrap();
    let left = block(core.sync_downloads(None)).unwrap();
    assert_eq!(left.tracks.len(), 1);
    assert_eq!(left.bytes, 2000);
    assert_eq!(downloads(&dir), vec!["download-1.mp3"]);
    assert!(
        !core
            .library()
            .tracks
            .iter()
            .find(|t| t.id == knows())
            .unwrap()
            .downloaded
    );

    let none = block(core.remove_all_downloads()).unwrap();
    assert_eq!(none, Default::default());
    assert!(downloads(&dir).is_empty());
}

#[test]
fn the_plan_fetches_preferred_takes_in_the_order_chosen() {
    let (_dir, core) = ready(server());
    let library = core.library();
    let mut offline = OfflineFile::default();
    offline.set_wanted(&[knows()], true, 2).unwrap();
    offline.set_wanted(&[night()], true, 1).unwrap();
    offline
        .set_wanted(&["take-server:gone".into()], true, 3)
        .unwrap();
    let plan = plan(&offline, &library);
    let order: Vec<&str> = plan.fetch.iter().map(|f| f.recording_id.as_str()).collect();
    assert_eq!(order, vec!["r/night-2", "r/ek-1"]);
    assert_eq!(plan.fetch[1].extension, "wav");
    assert!(plan.remove.is_empty());

    assert_eq!(extension_of("http://x/audio/a.FLAC?x=1"), "flac");
    assert_eq!(extension_of("http://x/audio/a"), "mp3");
    assert_eq!(extension_of("http://x/audio/a.exe"), "mp3");
}

fn music_file(dir: &Path, name: &str, bytes: &[u8]) -> MusicFile {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    MusicFile {
        path: path.to_string_lossy().into_owned(),
        file_name: name.into(),
        title: None,
        artist: None,
        album: None,
        number: None,
        genre: None,
        duration_seconds: None,
        artwork_path: None,
    }
}

#[test]
fn own_files_join_the_library_once_and_stay_after_a_restart() {
    let (dir, core) = ready(server());
    let picked = tempfile::tempdir().unwrap();
    let mut bonus = music_file(picked.path(), "Bonus Track.m4a", b"bonus audio");
    bonus.title = Some("  Bonus  ".into());
    bonus.artist = Some("AFTER HOURS".into());
    bonus.album = Some("AFTER HOURS".into());
    bonus.number = Some(3);
    bonus.duration_seconds = Some(61.5);
    let cover = picked.path().join("cover.png");
    std::fs::write(&cover, b"png").unwrap();
    bonus.artwork_path = Some(cover.to_string_lossy().into_owned());
    let demo = music_file(picked.path(), "my_demo.MP3", b"demo audio");
    let notes = music_file(picked.path(), "notes.txt", b"words");
    let twice = music_file(picked.path(), "copy of demo.mp3", b"demo audio");

    let result = block(core.import_music(vec![bonus, demo, notes, twice])).unwrap();
    assert_eq!(result.added.len(), 2);
    assert_eq!(result.already, 1, "the same audio twice comes in once");
    assert_eq!(result.refused.len(), 1);
    assert!(result.refused[0].starts_with("notes.txt"));

    let library = core.library();
    let bonus = library.tracks.iter().find(|t| t.title == "Bonus").unwrap();
    assert!(bonus.id.starts_with("file:"));
    assert!(bonus.downloaded);
    assert_eq!(bonus.number, Some(3));
    assert_eq!(bonus.duration_seconds, Some(61.5));
    assert!(bonus.artwork_url.as_deref().unwrap().starts_with("file://"));
    let recording = bonus.preferred.as_ref().unwrap();
    assert_eq!(recording.label, "Bonus Track.m4a");
    assert!(recording.audio_url.starts_with("file://"));
    assert!(recording.audio_url.ends_with(".m4a"));
    // it joins the server's album and artist
    let album = library
        .albums
        .iter()
        .find(|a| a.id == album_id(Some("AFTER HOURS"), "AFTER HOURS"))
        .unwrap();
    assert_eq!(album.track_count, 3);
    let demo = library
        .tracks
        .iter()
        .find(|t| t.title == "my demo")
        .unwrap();
    assert_eq!(demo.artist, "Unknown Artist");
    let demo_id = demo.id.clone();
    let bonus_id = bonus.id.clone();

    // Like any track: edits, favourites, playlists; never downloaded again.
    block(core.edit_track(
        demo_id.clone(),
        TrackEdit {
            title: Some("Demo".into()),
            ..Default::default()
        },
    ))
    .unwrap();
    block(core.set_favourite(demo_id.clone(), true)).unwrap();
    block(core.create_playlist("Mine".into(), vec![demo_id.clone(), night()])).unwrap();
    assert_eq!(core.collection().favourite_tracks[0].title, "Demo");
    block(core.set_offline(vec![demo_id.clone()], true)).unwrap();
    let summary = block(core.sync_downloads(None)).unwrap();
    assert_eq!(summary.tracks[0].state, DownloadState::Downloaded);
    assert_eq!(summary.bytes, 0);
    assert!(downloads(&dir).is_empty(), "it is on the phone already");

    // The originals can go; the copies stay, and so does everything.
    drop(picked);
    drop(core);
    let core = open(&dir, StaticClient::new());
    let library = core.library();
    assert!(library.tracks.iter().any(|t| t.title == "Demo"));
    let copies = files_in(&dir.path().join("data/imported"));
    assert_eq!(copies.len(), 2);

    // Removing one deletes its copy and leaves the other.
    block(core.remove_imported(bonus_id.clone())).unwrap();
    assert_eq!(files_in(&dir.path().join("data/imported")).len(), 1);
    assert!(!core.library().tracks.iter().any(|t| t.id == bonus_id));
    assert!(matches!(
        block(core.remove_imported(bonus_id)),
        Err(SoundscapeError::NotInLibrary { .. })
    ));
    let playlist = &core.collection().playlists[0];
    assert_eq!(playlist.track_count, 2);
}
