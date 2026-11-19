//! What was playing survives the app closing: the queue, the song and how
//! far into it, brought up to date with the library when it opens again.

use std::sync::Arc;

use forge_net::StaticClient;
use soundscape_core::catalogue::rights_from_licence;
use soundscape_core::playback::{refresh, tidy};
use soundscape_core::queue::queue_start;
use soundscape_core::{
    CatalogueTrack, PlayQueue, QueueTrack, RepeatMode, SavedPlayback, SoundscapeCore, TrackEdit,
};

fn open(dir: &tempfile::TempDir) -> Arc<SoundscapeCore> {
    SoundscapeCore::open_with_http(
        dir.path().join("data").to_str().unwrap(),
        dir.path().join("caches").to_str().unwrap(),
        Arc::new(StaticClient::new()),
    )
    .unwrap()
}

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

fn song(n: u32) -> CatalogueTrack {
    CatalogueTrack {
        id: format!("jamendo:{n}"),
        provider: "Jamendo".into(),
        title: format!("Song {n}"),
        artist: Some("Band".into()),
        album: Some("Album".into()),
        number: Some(n),
        genre: None,
        duration_seconds: Some(200.0),
        artwork_url: None,
        audio_url: format!("https://prod-1.storage.jamendo.com/?trackid={n}&format=mp32"),
        rights: rights_from_licence(None, false, "c", None),
        added_ms: 0,
    }
}

fn saved(queue: PlayQueue, seconds: f64) -> SavedPlayback {
    SavedPlayback {
        queue,
        seconds,
        saved_ms: 1,
    }
}

fn titles(saved: &SavedPlayback) -> Vec<String> {
    saved
        .queue
        .order
        .iter()
        .map(|i| saved.queue.tracks[*i as usize].title.clone())
        .collect()
}

fn queue_of(core: &SoundscapeCore, songs: &[u32], start: u32) -> PlayQueue {
    let tracks: Vec<QueueTrack> =
        core.catalogue_queue_tracks(songs.iter().map(|n| song(*n)).collect());
    queue_start(tracks, start, false, RepeatMode::All, 7)
}

#[test]
fn the_queue_and_time_come_back_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    assert_eq!(core.saved_playback(), None);
    let queue = queue_of(&core, &[1, 2, 3], 1);
    block(core.save_playback(Some(saved(queue.clone(), 42.5)))).unwrap();

    drop(core);
    let core = open(&dir);
    let back = core.saved_playback().unwrap();
    assert_eq!(back.queue, queue);
    assert_eq!(back.seconds, 42.5);
    assert_eq!(back.queue.repeat, RepeatMode::All);

    // Stopping forgets it.
    block(core.save_playback(None)).unwrap();
    drop(core);
    assert_eq!(open(&dir).saved_playback(), None);
}

#[test]
fn songs_come_back_as_the_library_has_them_now() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    block(core.add_catalogue_tracks(vec![song(1), song(2)])).unwrap();
    let queue = queue_of(&core, &[1, 2, 3], 0);
    block(core.save_playback(Some(saved(queue, 10.0)))).unwrap();
    block(core.edit_track(
        "jamendo:1".into(),
        TrackEdit {
            title: Some("Renamed".into()),
            ..Default::default()
        },
    ))
    .unwrap();

    let back = core.saved_playback().unwrap();
    assert_eq!(titles(&back), ["Renamed", "Song 2", "Song 3"]);
    assert_eq!(back.seconds, 10.0);
}

#[test]
fn a_file_that_is_gone_is_left_out_and_playback_moves_on() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let mut tracks = core.catalogue_queue_tracks(vec![song(1), song(2), song(3)]);
    // A file on the phone the library no longer names.
    tracks[1].recording.audio_url = "file:///gone/download-9.mp3".into();
    let queue = queue_start(tracks, 1, false, RepeatMode::Off, 1);
    block(core.save_playback(Some(saved(queue, 30.0)))).unwrap();

    let back = core.saved_playback().unwrap();
    assert_eq!(titles(&back), ["Song 1", "Song 3"]);
    assert_eq!(back.queue.position, 1, "on the next song kept");
    assert_eq!(back.seconds, 0.0, "from its start");
}

#[test]
fn refresh_keeps_the_song_and_the_shuffled_order() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let tracks = core.catalogue_queue_tracks((1..=6).map(song).collect());
    let queue = queue_start(tracks, 2, true, RepeatMode::Off, 99);
    let playing = titles(&saved(queue.clone(), 0.0))[queue.position as usize].clone();
    let back = refresh(saved(queue.clone(), 5.0), |t| {
        (t.title != "Song 6" || playing == "Song 6").then(|| t.clone())
    })
    .unwrap();
    assert_eq!(titles(&back)[back.queue.position as usize], playing);
    assert_eq!(back.seconds, 5.0);
    assert!(back.queue.shuffle);

    assert!(refresh(saved(queue, 0.0), |_| None).is_none());
}

#[test]
fn a_broken_saved_queue_is_made_safe() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let mut queue = queue_of(&core, &[1, 2], 0);
    queue.order = vec![0, 7, 1];
    queue.position = 9;
    let fixed = tidy(saved(queue.clone(), f64::NAN)).unwrap();
    assert_eq!(fixed.queue.order, [0, 1]);
    assert_eq!(fixed.queue.position, 1);
    assert_eq!(fixed.seconds, 0.0);

    queue.order = vec![5];
    assert!(tidy(saved(queue, 1.0)).is_none());
}

#[test]
fn the_crossfade_is_kept_and_has_a_limit() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    assert_eq!(core.crossfade_seconds(), 0, "gapless until set");
    assert_eq!(block(core.set_crossfade_seconds(6)).unwrap(), 6);
    assert_eq!(block(core.set_crossfade_seconds(60)).unwrap(), 12);
    drop(core);
    assert_eq!(open(&dir).crossfade_seconds(), 12);
}
