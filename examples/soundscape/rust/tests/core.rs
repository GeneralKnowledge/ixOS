//! Soundscape's API as the SwiftUI app will use it, on the real runtime.

use std::path::Path;
use std::sync::Arc;

use soundscape_core::{Echo, SoundscapeCore, SoundscapeError, TrackSettings};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

fn open(dir: &tempfile::TempDir) -> Result<Arc<SoundscapeCore>, SoundscapeError> {
    SoundscapeCore::new(
        dir.path().join("data").to_string_lossy().into_owned(),
        dir.path().join("caches").to_string_lossy().into_owned(),
    )
}

/// A mono 48 kHz float recording of `seconds` of a 220 Hz tone at `level`,
/// as AVAudioFile would write it.
fn recording(dir: &Path, name: &str, seconds: f64, level: f32) -> String {
    let path = dir.join(name);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for i in 0..(48_000.0 * seconds) as usize {
        let t = i as f32 / 48_000.0;
        w.write_sample(level * (2.0 * std::f32::consts::PI * 220.0 * t).sin())
            .unwrap();
    }
    w.finalize().unwrap();
    path.to_string_lossy().into_owned()
}

fn recordings(dir: &tempfile::TempDir) -> usize {
    std::fs::read_dir(dir.path().join("data/recordings"))
        .map(|d| d.count())
        .unwrap_or(0)
}

#[test]
fn a_project_records_mixes_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let project = block_on(core.create_project("Night bus".into(), 120.0, 2)).unwrap();
    let id = project.summary.id;
    assert_eq!(project.loop_seconds, 4.0);
    assert_eq!(core.loop_frames(id, 48_000).unwrap(), 192_000);
    let drums = project.tracks[0].id;

    let source = recording(dir.path(), "take.wav", 4.0, 0.5);
    let with_clip = block_on(core.set_clip(id, drums, source.clone())).unwrap();
    let clip = with_clip.tracks[0].clip.clone().unwrap();
    assert_eq!((clip.sample_rate, clip.channels), (48_000, 1));
    assert_eq!(clip.duration_ms, 4_000);
    assert!(
        clip.path.ends_with(".wav"),
        "AVAudioFile wants an extension"
    );
    std::fs::remove_file(&source).unwrap();
    assert!(
        Path::new(&clip.path).exists(),
        "Soundscape keeps its own copy"
    );

    let bass = block_on(core.add_track(id, Some("Bass".into())))
        .unwrap()
        .tracks[1]
        .id;
    let settings = TrackSettings {
        gain_db: -6.0,
        pan: 0.5,
        tone: 0.4,
        echo: Some(Echo {
            beats: 0.5,
            feedback: 0.3,
            mix: 0.2,
        }),
        ..TrackSettings::default()
    };
    let detail = block_on(core.set_track_settings(id, bass, settings)).unwrap();
    let mix = detail.tracks[1].mix;
    assert_eq!(mix.delay_seconds, Some(0.25));
    assert!(mix.low_pass_hz.is_some());

    let peaks = block_on(core.waveform(id, drums, 100, None)).unwrap();
    assert_eq!(peaks.len(), 100);
    assert!(peaks.iter().all(|p| (*p - 0.5).abs() < 0.02));
    assert!(block_on(core.waveform(id, bass, 100, None))
        .unwrap()
        .is_empty());

    let mixed = block_on(core.mixdown(id, 2, None)).unwrap();
    assert_eq!(mixed.duration_ms, 8_000);
    assert!(!mixed.limited);
    assert!((mixed.peak_db - 20.0 * 0.5f64.log10()).abs() < 0.1);
    let reader = hound::WavReader::open(&mixed.path).unwrap();
    assert_eq!(reader.spec().channels, 2);
    assert_eq!(reader.duration(), 8 * 44_100);

    core.shutdown();
    drop(core);
    let core = open(&dir).unwrap();
    let again = core.project(id).unwrap();
    assert_eq!(again.tracks[1].settings, settings);
    assert!(again.tracks[0].clip.is_some());
    assert_eq!(core.projects()[0].track_count, 2);
}

#[test]
fn recordings_go_when_nothing_uses_them() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let p = block_on(core.create_project("A".into(), 90.0, 1)).unwrap();
    let (id, track) = (p.summary.id, p.tracks[0].id);
    let first = recording(dir.path(), "1.wav", 0.5, 0.2);
    let second = recording(dir.path(), "2.wav", 0.5, 0.3);
    block_on(core.set_clip(id, track, first)).unwrap();
    block_on(core.set_clip(id, track, second.clone())).unwrap();
    assert_eq!(recordings(&dir), 1, "the replaced take went");

    let other = block_on(core.create_project("B".into(), 90.0, 1)).unwrap();
    block_on(core.set_clip(other.summary.id, other.tracks[0].id, second)).unwrap();
    assert_eq!(recordings(&dir), 1, "the same take is kept once");
    block_on(core.delete_project(id)).unwrap();
    assert_eq!(recordings(&dir), 1, "B still uses it");
    block_on(core.clear_clip(other.summary.id, other.tracks[0].id)).unwrap();
    assert_eq!(recordings(&dir), 0);
}

#[test]
fn bad_input_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let p = block_on(core.create_project("A".into(), 90.0, 1)).unwrap();
    let (id, track) = (p.summary.id, p.tracks[0].id);
    let revision = core.revision();

    let not_audio = dir.path().join("notes.txt");
    std::fs::write(&not_audio, b"hello").unwrap();
    assert!(matches!(
        block_on(core.set_clip(id, track, not_audio.to_string_lossy().into_owned())),
        Err(SoundscapeError::Audio { .. })
    ));
    let take = recording(dir.path(), "t.wav", 0.2, 0.2);
    assert_eq!(
        block_on(core.set_clip(id, 999, take)),
        Err(SoundscapeError::NotFound { id: 999 })
    );
    assert_eq!(recordings(&dir), 0, "a refused take isn't kept");
    assert!(matches!(
        block_on(core.set_timing(id, 300.0, 1)),
        Err(SoundscapeError::OutOfRange { .. })
    ));
    assert_eq!(
        block_on(core.mixdown(id, 1, None)),
        Err(SoundscapeError::Silent)
    );
    assert!(block_on(core.mixdown(id, 0, None)).is_err());
    assert_eq!(core.revision(), revision);
    assert_eq!(core.project(id).unwrap(), p);
}
