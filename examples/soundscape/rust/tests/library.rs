//! The music library: tracks apart from their recordings, kept on the phone,
//! with edits and artwork, against canned take-server answers.

use std::sync::Arc;

use forge_net::{Method, Response, StaticClient};
use soundscape_core::library::{album_id, artist_id, track_id, UNKNOWN_ARTIST};
use soundscape_core::{ArtworkTarget, SoundscapeCore, SoundscapeError, TrackEdit};

const SERVER: &str = "http://100.64.0.7:8642";

const CATALOG: &str = r#"{"version": 1,
 "artists": [{"id": "after-hours", "name": "AFTER HOURS", "artwork": "/art/after-hours/artist.jpg"},
             {"id": "lowbridge", "name": "LOWBRIDGE", "artwork": null}],
 "albums": [{"id": "after-hours", "title": "AFTER HOURS", "artist": "after-hours", "artwork": "/art/after-hours/album.jpg"},
            {"id": "lowbridge", "title": "Under the Bridge", "artist": "lowbridge", "artwork": null}],
 "tracks": [
  {"slug": "ah-one-more-night", "title": "One More Night", "artist": "after-hours", "album": "after-hours",
   "number": 1, "artwork": null, "pick": "r/ah-one-more-night-2", "updated": 300,
   "takes": [
     {"id": "r/ah-one-more-night-1", "file": "r/ah-one-more-night-1.mp3", "label": "seed 1", "mtime": 100,
      "seconds": 225, "seed": 1, "model": "turbo", "bpm": 112, "key": "A major", "caption": "boy band",
      "lyrics": "[verse]", "lufs": -11.0, "stars": 5, "note": "great"},
     {"id": "r/ah-one-more-night-2", "file": "r/ah-one-more-night-2.mp3", "label": "seed 2", "mtime": 300,
      "seconds": 230, "seed": 2, "model": "xl-turbo", "lufs": -12.5, "stars": 3, "note": ""}]},
  {"slug": "ah-everybody-knows", "title": "Everybody Knows", "artist": "after-hours", "album": "after-hours",
   "number": 2, "artwork": "/art/after-hours/ah-everybody-knows.png", "pick": null, "updated": 200,
   "takes": [{"id": "r/ek-1", "file": "r/ek-1.mp3", "label": "seed 9", "mtime": 200, "stars": 0}]},
  {"slug": "lb-anchor", "title": "Anchor", "artist": "lowbridge", "album": "lowbridge", "number": 1,
   "artwork": null, "pick": null, "updated": 150,
   "takes": [{"id": "r/lb-1", "file": "r/lb-1.mp3", "label": "a", "mtime": 10, "stars": 2},
             {"id": "r/lb-2", "file": "r/lb-2.mp3", "label": "b", "mtime": 20, "stars": 2}]},
  {"slug": "voice-note", "title": "voice note", "artist": null, "album": null, "number": null,
   "artwork": null, "pick": null, "updated": 50,
   "takes": [{"id": "outputs/voice-note", "file": "outputs/voice-note.wav", "label": "voice-note", "mtime": 50}]}
 ]}"#;

const NIGHT_PICKED_1: &str = r#"{"slug": "ah-one-more-night", "title": "One More Night", "actTitle": "AFTER HOURS",
 "pick": "r/ah-one-more-night-1",
 "takes": [{"id": "r/ah-one-more-night-1", "file": "r/ah-one-more-night-1.mp3", "label": "seed 1", "mtime": 100, "stars": 5},
           {"id": "r/ah-one-more-night-2", "file": "r/ah-one-more-night-2.mp3", "label": "seed 2", "mtime": 300, "stars": 3}]}"#;

fn core_with(http: StaticClient) -> (tempfile::TempDir, Arc<SoundscapeCore>, Arc<StaticClient>) {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(http);
    let core = SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        http.clone(),
    )
    .unwrap();
    block(core.set_listen_server(SERVER.into())).unwrap();
    (dir, core, http)
}

fn reopen(dir: &tempfile::TempDir, http: StaticClient) -> Arc<SoundscapeCore> {
    SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(http),
    )
    .unwrap()
}

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

fn catalog_server() -> StaticClient {
    StaticClient::new().route(
        Method::Get,
        format!("{SERVER}/api/catalog"),
        Ok(Response::new(200, CATALOG)),
    )
}

#[test]
fn the_catalog_becomes_artists_albums_and_tracks_with_recordings() {
    let (_dir, core, _) = core_with(catalog_server());
    assert!(
        core.library().tracks.is_empty(),
        "empty before the first refresh"
    );
    let lib = block(core.refresh_library(false, None)).unwrap();

    let artists: Vec<_> = lib
        .artists
        .iter()
        .map(|a| (a.name.as_str(), a.album_count, a.track_count))
        .collect();
    assert_eq!(artists, [("AFTER HOURS", 1, 2), ("LOWBRIDGE", 1, 1)]);
    assert_eq!(
        lib.artists[0].artwork_url.as_deref(),
        Some("http://100.64.0.7:8642/art/after-hours/artist.jpg")
    );
    let albums: Vec<_> = lib
        .albums
        .iter()
        .map(|a| (a.title.as_str(), a.artist.as_str()))
        .collect();
    assert_eq!(
        albums,
        [
            ("AFTER HOURS", "AFTER HOURS"),
            ("Under the Bridge", "LOWBRIDGE")
        ]
    );

    // one track per song, however many takes it has
    let titles: Vec<_> = lib.tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Anchor", "Everybody Knows", "One More Night", "voice note"]
    );
    let night = lib
        .tracks
        .iter()
        .find(|t| t.title == "One More Night")
        .unwrap();
    assert_eq!(night.id, track_id("ah-one-more-night"));
    assert_eq!(night.recording_count, 2);
    let preferred = night.preferred.as_ref().unwrap();
    assert_eq!(
        preferred.id, "r/ah-one-more-night-2",
        "the server's pick plays"
    );
    assert_eq!(
        preferred.audio_url,
        format!("{SERVER}/audio/r/ah-one-more-night-2.mp3")
    );
    assert_eq!(night.duration_seconds, Some(230.0));
    assert_eq!(
        night.artwork_url.as_deref(),
        Some("http://100.64.0.7:8642/art/after-hours/album.jpg"),
        "a track without its own artwork shows its album's"
    );
    let knows = lib
        .tracks
        .iter()
        .find(|t| t.title == "Everybody Knows")
        .unwrap();
    assert_eq!(
        knows.artwork_url.as_deref(),
        Some("http://100.64.0.7:8642/art/after-hours/ah-everybody-knows.png")
    );
    // no pick: the best rated, then the newer of a tie
    let anchor = lib.tracks.iter().find(|t| t.title == "Anchor").unwrap();
    assert_eq!(anchor.preferred.as_ref().unwrap().id, "r/lb-2");
    assert!(night.picked && !anchor.picked);
    let note = lib.tracks.iter().find(|t| t.title == "voice note").unwrap();
    assert_eq!(
        (note.artist.as_str(), note.album_id.as_deref()),
        (UNKNOWN_ARTIST, None)
    );

    // an album's tracks in order, and the detail with every recording
    let album = album_id(Some("AFTER HOURS"), "AFTER HOURS");
    let order: Vec<_> = core
        .album_tracks(album)
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(order, ["One More Night", "Everybody Knows"]);
    let detail = core.library_track(track_id("ah-one-more-night")).unwrap();
    assert_eq!(detail.recordings.len(), 2);
    assert_eq!(detail.caption.as_deref(), Some("boy band"));
    let gen = detail.recordings[0].generation.as_ref().unwrap();
    assert_eq!((gen.seed, gen.bpm), (Some(1), Some(112.0)));
    assert!(matches!(
        core.library_track("take-server:nope".into()),
        Err(SoundscapeError::NotInLibrary { .. })
    ));

    // the queue plays each track's preferred recording, in the order asked
    let queue = core.queue_tracks(vec![
        track_id("lb-anchor"),
        "gone".into(),
        track_id("ah-one-more-night"),
    ]);
    let ids: Vec<_> = queue.iter().map(|q| q.recording.id.as_str()).collect();
    assert_eq!(ids, ["r/lb-2", "r/ah-one-more-night-2"]);
    assert_eq!(queue[1].artist, "AFTER HOURS");
}

#[test]
fn the_library_is_kept_and_edits_survive_a_refresh() {
    let (dir, core, _) = core_with(catalog_server());
    block(core.refresh_library(false, None)).unwrap();
    let id = track_id("lb-anchor");
    let edited = block(core.edit_track(
        id.clone(),
        TrackEdit {
            title: Some("Anchor (Live)".into()),
            album: Some("Live at the Bridge".into()),
            genre: Some("Folk".into()),
            ..Default::default()
        },
    ))
    .unwrap();
    assert_eq!(edited.track.title, "Anchor (Live)");
    assert!(edited.track.edited);
    // moving it to another album makes that album
    let lib = core.library();
    let live = lib
        .albums
        .iter()
        .find(|a| a.title == "Live at the Bridge")
        .unwrap();
    assert_eq!((live.artist.as_str(), live.track_count), ("LOWBRIDGE", 1));
    assert!(lib.albums.iter().all(|a| a.title != "Under the Bridge"));
    assert_eq!(
        lib.artists
            .iter()
            .find(|a| a.name == "LOWBRIDGE")
            .unwrap()
            .album_count,
        1
    );

    // kept on the phone: a reopened core browses it without the server
    drop(core);
    let core = reopen(&dir, StaticClient::new());
    let lib = core.library();
    assert_eq!(lib.tracks.len(), 4);
    assert!(lib.refreshed_ms.is_some());
    assert_eq!(
        core.library_track(id.clone())
            .unwrap()
            .track
            .genre
            .as_deref(),
        Some("Folk")
    );
    drop(core);

    // a refresh keeps the edit; an empty text goes back to the server's
    let core = reopen(&dir, catalog_server());
    block(core.refresh_library(false, None)).unwrap();
    assert_eq!(
        core.library_track(id.clone()).unwrap().track.title,
        "Anchor (Live)"
    );
    let back = block(core.edit_track(
        id.clone(),
        TrackEdit {
            title: Some(" ".into()),
            album: Some(String::new()),
            genre: Some(String::new()),
            ..Default::default()
        },
    ))
    .unwrap();
    assert_eq!(back.track.title, "Anchor");
    assert!(!back.track.edited);
    assert!(matches!(
        block(core.edit_track("take-server:nope".into(), TrackEdit::default())),
        Err(SoundscapeError::NotInLibrary { .. })
    ));
    assert!(matches!(
        block(core.edit_track(
            id,
            TrackEdit {
                title: Some("x".repeat(201)),
                ..Default::default()
            }
        )),
        Err(SoundscapeError::OutOfRange { .. })
    ));
}

#[test]
fn own_artwork_wins_and_goes_when_cleared() {
    let (dir, core, _) = core_with(catalog_server());
    block(core.refresh_library(false, None)).unwrap();
    let picture = dir.path().join("cover.PNG");
    std::fs::write(&picture, b"\x89PNG fake").unwrap();
    let album = album_id(Some("AFTER HOURS"), "AFTER HOURS");
    let lib = block(core.set_artwork(
        ArtworkTarget::Album { id: album.clone() },
        picture.to_string_lossy().into(),
    ))
    .unwrap();
    let art = lib
        .albums
        .iter()
        .find(|a| a.id == album)
        .unwrap()
        .artwork_url
        .clone()
        .unwrap();
    assert!(art.starts_with("file://") && art.ends_with(".png"), "{art}");
    let night = lib
        .tracks
        .iter()
        .find(|t| t.title == "One More Night")
        .unwrap();
    assert_eq!(
        night.artwork_url.as_deref(),
        Some(art.as_str()),
        "its tracks show it too"
    );
    let stored = dir.path().join("data/artwork");
    assert_eq!(std::fs::read_dir(&stored).unwrap().count(), 1);

    let lib = block(core.clear_artwork(ArtworkTarget::Album { id: album.clone() })).unwrap();
    assert_eq!(
        lib.albums
            .iter()
            .find(|a| a.id == album)
            .unwrap()
            .artwork_url
            .as_deref(),
        Some("http://100.64.0.7:8642/art/after-hours/album.jpg")
    );
    assert_eq!(
        std::fs::read_dir(&stored).unwrap().count(),
        0,
        "the copy is removed"
    );

    let text = dir.path().join("notes.txt");
    std::fs::write(&text, b"no").unwrap();
    assert!(matches!(
        block(core.set_artwork(
            ArtworkTarget::Artist {
                id: artist_id("LOWBRIDGE")
            },
            text.to_string_lossy().into()
        )),
        Err(SoundscapeError::OutOfRange { .. })
    ));
}

#[test]
fn choosing_a_recording_picks_it_on_the_server() {
    let http = catalog_server()
        .route(
            Method::Post,
            format!("{SERVER}/api/pick"),
            Ok(Response::new(200, r#"{"ok": true}"#)),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=ah-one-more-night"),
            Ok(Response::new(200, NIGHT_PICKED_1)),
        );
    let (_dir, core, http) = core_with(http);
    block(core.refresh_library(false, None)).unwrap();
    let id = track_id("ah-one-more-night");
    let detail = block(core.choose_recording(id.clone(), "r/ah-one-more-night-1".into())).unwrap();
    assert_eq!(
        detail.track.preferred.as_ref().unwrap().id,
        "r/ah-one-more-night-1"
    );
    assert!(detail.recordings[0].preferred && !detail.recordings[1].preferred);
    // the length the catalogue gave survives the server's song answer
    assert_eq!(detail.track.duration_seconds, Some(225.0));
    let post = http
        .received()
        .into_iter()
        .find(|r| r.method == Method::Post)
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&post.body).unwrap();
    assert_eq!(body["take"], "r/ah-one-more-night-1");
    assert_eq!(body["pick"], true);

    // one recording on its own, rather than the preferred one
    let other = core
        .queue_recording(id, "r/ah-one-more-night-2".into())
        .unwrap();
    assert_eq!(other.recording.label, "seed 2");
    assert_eq!(other.title, "One More Night");
}

#[test]
fn search_finds_artists_albums_and_tracks() {
    let (_dir, core, _) = core_with(catalog_server());
    block(core.refresh_library(false, None)).unwrap();
    let found = core.search_library("after".into());
    assert_eq!(found.artists.len(), 1);
    assert_eq!(found.albums.len(), 1);
    assert_eq!(found.tracks.len(), 2, "tracks by their artist too");
    let found = core.search_library("ANCHOR".into());
    assert_eq!(
        found
            .tracks
            .iter()
            .map(|t| t.title.as_str())
            .collect::<Vec<_>>(),
        ["Anchor"]
    );
    assert!(core.search_library("  ".into()).tracks.is_empty());
    assert_eq!(
        core.artist_albums(artist_id("LOWBRIDGE"))[0].title,
        "Under the Bridge"
    );
    assert_eq!(core.artist_tracks(artist_id("AFTER HOURS")).len(), 2);
}

const LIBRARY: &str = r#"{"songs": [
  {"slug": "ah-one-more-night", "title": "AFTER HOURS · One More Night", "act": "after-hours",
   "actTitle": "AFTER HOURS", "takes": 1, "updated": 300},
  {"slug": "gone", "title": "Gone", "takes": 1, "updated": 100}
]}"#;

#[test]
fn an_older_server_without_a_catalogue_is_asked_song_by_song() {
    let http = StaticClient::new()
        .route(
            Method::Get,
            format!("{SERVER}/api/catalog"),
            Ok(Response::new(404, "{}")),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/library"),
            Ok(Response::new(200, LIBRARY)),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=ah-one-more-night"),
            Ok(Response::new(200, NIGHT_PICKED_1)),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=gone"),
            Ok(Response::new(404, "")),
        );
    let (_dir, core, _) = core_with(http);
    let lib = block(core.refresh_library(false, None)).unwrap();
    assert_eq!(
        lib.tracks.len(),
        1,
        "a song gone since the list is left out"
    );
    let t = &lib.tracks[0];
    assert_eq!(
        (t.title.as_str(), t.artist.as_str(), t.album.as_deref()),
        ("One More Night", "AFTER HOURS", Some("AFTER HOURS"))
    );
    assert_eq!(t.preferred.as_ref().unwrap().id, "r/ah-one-more-night-1");
}

#[test]
fn each_recording_carries_its_own_lyrics() {
    let (_dir, core, _) = core_with(catalog_server());
    block(core.refresh_library(false, None)).unwrap();
    let detail = core.library_track(track_id("ah-one-more-night")).unwrap();
    let words: Vec<_> = detail
        .recordings
        .iter()
        .map(|r| (r.id.as_str(), r.lyrics.as_deref()))
        .collect();
    assert_eq!(
        words,
        [
            ("r/ah-one-more-night-1", Some("[verse]")),
            ("r/ah-one-more-night-2", None)
        ]
    );
}

#[test]
fn songs_to_pick_have_several_takes_and_no_pick() {
    let (_dir, core, _) = core_with(catalog_server());
    block(core.refresh_library(false, None)).unwrap();
    let ids: Vec<String> = core.tracks_to_pick().into_iter().map(|t| t.id).collect();
    // One More Night is picked; Everybody Knows and the voice note have one take.
    assert_eq!(ids, vec![track_id("lb-anchor")]);
}

#[test]
fn recordings_list_best_newest_or_oldest_first() {
    use soundscape_core::RecordingOrder;
    let (_dir, core, _) = core_with(catalog_server());
    block(core.refresh_library(false, None)).unwrap();
    let id = track_id("ah-one-more-night");
    let labels = |order| -> Vec<String> {
        core.library_track_sorted(id.clone(), order)
            .unwrap()
            .recordings
            .into_iter()
            .map(|r| r.label)
            .collect()
    };
    // seed 2 is picked, so it plays and comes first, though seed 1 has more stars
    assert_eq!(labels(RecordingOrder::Best), vec!["seed 2", "seed 1"]);
    assert_eq!(labels(RecordingOrder::Newest), vec!["seed 2", "seed 1"]);
    assert_eq!(labels(RecordingOrder::Oldest), vec!["seed 1", "seed 2"]);
}

#[test]
fn rating_a_recording_from_its_track_goes_to_the_server() {
    use soundscape_core::TakeMark;
    let http = catalog_server()
        .route(
            Method::Post,
            format!("{SERVER}/api/pick"),
            Ok(Response::new(200, r#"{"ok": true}"#)),
        )
        .route(
            Method::Get,
            format!("{SERVER}/api/song?slug=ah-one-more-night"),
            Ok(Response::new(200, NIGHT_PICKED_1)),
        );
    let (_dir, core, http) = core_with(http);
    block(core.refresh_library(false, None)).unwrap();
    let mark = TakeMark {
        picked: None,
        stars: Some(4),
        note: Some("chorus lands".into()),
    };
    let detail = block(core.mark_recording(
        track_id("ah-one-more-night"),
        "r/ah-one-more-night-2".into(),
        mark.clone(),
    ))
    .unwrap();
    assert_eq!(detail.track.title, "One More Night");
    let post = http
        .received()
        .into_iter()
        .find(|r| r.method == Method::Post)
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&post.body).unwrap();
    assert_eq!(body["take"], "r/ah-one-more-night-2");
    assert_eq!(body["stars"], 4);
    assert_eq!(body["note"], "chorus lands");

    // An id that isn't a take-server track is refused before anything is sent.
    assert!(matches!(
        block(core.mark_recording("file:abc".into(), "x".into(), mark)),
        Err(SoundscapeError::NotInLibrary { .. })
    ));
}
