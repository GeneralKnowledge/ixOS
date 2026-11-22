//! The fixed test catalogue: its files are made the same way every time,
//! in formats iOS reads, and its tracks go through the library, the
//! duplicate finder and downloads like any catalogue's.

use std::sync::Arc;

use forge_net::StaticClient;
use soundscape_core::test_catalogue::{self, Format, SPECS};
use soundscape_core::{listen, DownloadState, SoundscapeCore};

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    forge_runtime::block_on(f)
}

fn open(data: &std::path::Path) -> Arc<SoundscapeCore> {
    SoundscapeCore::open_with_http(
        data.to_str().unwrap(),
        data.join("caches").to_str().unwrap(),
        Arc::new(StaticClient::new()),
    )
    .unwrap()
}

fn path_of(url: &str) -> std::path::PathBuf {
    listen::decode(url.strip_prefix("file://").expect("a file URL")).into()
}

#[test]
fn every_track_has_its_files_and_covers_one_case() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir.path().join("my data"));
    let tracks = block(core.test_catalogue(None)).unwrap();
    assert_eq!(tracks.len(), SPECS.len());

    for (test, spec) in tracks.iter().zip(SPECS) {
        let track = &test.track;
        assert!(track.id.starts_with("test:"));
        assert_eq!(track.provider, "Test Catalogue");
        assert_eq!(track.rights.licence, "CC0 1.0");
        assert!(!test.checks.is_empty());
        // The space in "my data" is encoded, and the URL leads to the file.
        assert!(track.audio_url.contains("my%20data"), "{}", track.audio_url);
        let audio = std::fs::read(path_of(&track.audio_url)).unwrap();
        let samples = (spec.seconds * spec.rate) as usize;
        match spec.format {
            Format::Wav => {
                assert_eq!(&audio[..4], b"RIFF");
                assert_eq!(audio.len(), 44 + samples * 2);
            }
            Format::Aiff => {
                assert_eq!(&audio[..4], b"FORM");
                assert_eq!(&audio[8..12], b"AIFF");
                assert_eq!(audio.len(), 54 + samples * 2);
            }
            Format::Caf => {
                assert_eq!(&audio[..4], b"caff");
                assert_eq!(audio.len(), 68 + samples * 2);
            }
        }
        match &track.artwork_url {
            Some(url) => {
                let png = std::fs::read(path_of(url)).unwrap();
                assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
                assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
            }
            None => assert_eq!(spec.slug, "no-artwork"),
        }
    }

    let by = |slug: &str| {
        &tracks
            .iter()
            .find(|t| t.track.id == format!("test:{slug}"))
            .unwrap()
            .track
    };
    assert!(by("short-chime").duration_seconds.unwrap() < 5.0);
    assert!(by("long-drone").duration_seconds.unwrap() >= 600.0);
    assert_eq!(by("untagged").artist, None);
    assert_eq!(by("untagged").genre, None);
    assert!(!by("stream-only").rights.may_download);
    let formats: std::collections::HashSet<_> =
        SPECS.iter().map(|s| s.format.extension()).collect();
    assert_eq!(formats.len(), 3);
}

#[test]
fn files_come_out_the_same_every_time() {
    let spec = |slug| SPECS.iter().find(|s| s.slug == slug).unwrap();
    let slow = test_catalogue::samples(spec("theme-slow"));
    // The copy is the same recording in another format.
    assert_eq!(slow, test_catalogue::samples(spec("theme-copy")));
    assert_ne!(
        slow.len(),
        test_catalogue::samples(spec("theme-fast")).len()
    );
    assert_eq!(
        test_catalogue::png(16, [1, 2, 3]),
        test_catalogue::png(16, [1, 2, 3])
    );
    // Never silent, never clipped.
    for s in SPECS.iter().filter(|s| s.seconds < 100) {
        let samples = test_catalogue::samples(s);
        let loudest = samples.iter().map(|v| v.unsigned_abs()).max().unwrap();
        assert!(
            loudest > 1000 && loudest < i16::MAX as u16,
            "{}: {loudest}",
            s.slug
        );
    }

    let dir = tempfile::tempdir().unwrap();
    test_catalogue::ensure(dir.path()).unwrap();
    let first = std::fs::read(dir.path().join("theme-fast.aiff")).unwrap();
    std::fs::remove_file(dir.path().join("theme-fast.aiff")).unwrap();
    test_catalogue::ensure(dir.path()).unwrap();
    assert_eq!(
        first,
        std::fs::read(dir.path().join("theme-fast.aiff")).unwrap()
    );
    // Nothing half-written is left behind.
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".part")));
}

#[test]
fn added_tracks_follow_their_files_when_the_data_folder_moves() {
    let dir = tempfile::tempdir().unwrap();
    let before = dir.path().join("before");
    let core = open(&before);
    let tracks: Vec<_> = block(core.test_catalogue(None))
        .unwrap()
        .into_iter()
        .map(|t| t.track)
        .collect();
    block(core.add_catalogue_tracks(tracks)).unwrap();
    drop(core);

    // A reinstall gives the app a new data folder, with everything in it.
    let after = dir.path().join("after");
    std::fs::rename(&before, &after).unwrap();
    let core = open(&after);
    let library = core.library();
    assert_eq!(library.tracks.len(), SPECS.len());
    for track in &library.tracks {
        let url = &track.preferred.as_ref().unwrap().audio_url;
        assert!(url.contains("/after/"), "{url}");
        assert!(path_of(url).exists());
        if let Some(art) = &track.artwork_url {
            assert!(art.contains("/after/"), "{art}");
        }
    }
    let detail = core.library_track("test:long-drone".into()).unwrap();
    assert!(detail.recordings[0].audio_url.contains("/after/"));

    // Only the catalogue's own file names are followed.
    assert_eq!(
        test_catalogue::relocate("file:///elsewhere/secret.wav", &after),
        None
    );
    assert_eq!(
        test_catalogue::relocate("https://example.com/short-chime.wav", &after),
        None
    );
}

#[test]
fn the_same_recording_twice_is_offered_as_one_song_and_two_recordings_of_a_work_are_not() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(dir.path());
    let tracks: Vec<_> = block(core.test_catalogue(None))
        .unwrap()
        .into_iter()
        .map(|t| t.track)
        .collect();
    block(core.add_catalogue_tracks(tracks)).unwrap();

    let offered = core.duplicate_songs();
    assert_eq!(offered.len(), 1, "{offered:?}");
    let mut ids: Vec<_> = offered[0].tracks.iter().map(|t| t.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["test:theme-copy", "test:theme-slow"]);
}

#[test]
fn downloads_copy_the_files_except_the_stream_only_track() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(dir.path());
    let tracks: Vec<_> = block(core.test_catalogue(None))
        .unwrap()
        .into_iter()
        .map(|t| t.track)
        .filter(|t| t.id == "test:short-chime" || t.id == "test:stream-only")
        .collect();
    block(core.add_catalogue_tracks(tracks)).unwrap();
    block(core.set_offline(
        vec!["test:short-chime".into(), "test:stream-only".into()],
        true,
    ))
    .unwrap();
    let summary = block(core.sync_downloads(None)).unwrap();

    let state = |id: &str| {
        summary
            .tracks
            .iter()
            .find(|t| t.track.id == id)
            .map(|t| t.state.clone())
    };
    assert_eq!(state("test:short-chime"), Some(DownloadState::Downloaded));
    assert_ne!(state("test:stream-only"), Some(DownloadState::Downloaded));

    let library = core.library();
    let chime = library
        .tracks
        .iter()
        .find(|t| t.id == "test:short-chime")
        .unwrap();
    assert!(chime.downloaded);
    let url = &chime.preferred.as_ref().unwrap().audio_url;
    assert!(url.contains("/downloads/"), "{url}");
    assert_eq!(
        std::fs::read(path_of(url)).unwrap().len(),
        44 + 4 * 16_000 * 2
    );
}

#[test]
fn decode_undoes_encode() {
    for text in [
        "/var/Application Support/a b.wav",
        "plain",
        "100%",
        "%zz",
        "café",
    ] {
        let encoded = listen::encode(text, true);
        assert_eq!(listen::decode(&encoded), text);
    }
    assert_eq!(listen::decode("100%"), "100%");
}

#[test]
fn the_track_without_artwork_has_none_in_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(dir.path());
    let tracks: Vec<_> = block(core.test_catalogue(None))
        .unwrap()
        .into_iter()
        .map(|t| t.track)
        .collect();
    block(core.add_catalogue_tracks(tracks)).unwrap();
    let library = core.library();
    let bare = library
        .tracks
        .iter()
        .find(|t| t.id == "test:no-artwork")
        .unwrap();
    assert_eq!(bare.artwork_url, None);
}

#[test]
fn a_saved_queue_of_test_tracks_not_in_the_library_follows_the_data_folder() {
    let dir = tempfile::tempdir().unwrap();
    let before = dir.path().join("before");
    let core = open(&before);
    let tracks: Vec<_> = block(core.test_catalogue(None))
        .unwrap()
        .into_iter()
        .map(|t| t.track)
        .filter(|t| t.id == "test:short-chime")
        .collect();
    let queued = core.catalogue_queue_tracks(tracks);
    assert!(queued[0].recording.audio_url.starts_with("file:"));
    let queue =
        soundscape_core::queue::queue_start(queued, 0, false, soundscape_core::RepeatMode::Off, 1);
    block(core.save_playback(Some(soundscape_core::SavedPlayback {
        queue,
        seconds: 1.0,
        saved_ms: 1,
    })))
    .unwrap();
    drop(core);

    let after = dir.path().join("after");
    std::fs::rename(&before, &after).unwrap();
    let core = open(&after);
    let saved = core.saved_playback().expect("the queue survives");
    assert_eq!(saved.queue.tracks.len(), 1);
    let url = &saved.queue.tracks[0].recording.audio_url;
    assert!(url.contains("/after/"), "{url}");
    assert!(path_of(url).exists());
}
