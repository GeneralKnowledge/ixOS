//! FieldKit's API as the SwiftUI app will use it, on the real runtime.

use std::sync::Arc;

use fieldkit_core::{FieldKitCore, FieldKitError, LocationSample, NoteKind, Position, Recorded};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

fn open(dir: &tempfile::TempDir) -> Result<Arc<FieldKitCore>, FieldKitError> {
    FieldKitCore::new(dir.path().join("data").to_string_lossy().into_owned())
}

/// A walk north from `start_s`: one fix every 5 s, about 11 m apart, climbing
/// 1 m a fix.
fn walk(start_s: i64, count: i64) -> Vec<LocationSample> {
    (0..count)
        .map(|i| LocationSample {
            latitude: 51.5 + 0.0001 * (start_s / 5 + i) as f64,
            longitude: -0.12,
            altitude_m: Some(20.0 + (start_s / 5 + i) as f64),
            horizontal_accuracy_m: 8.0,
            timestamp_ms: (start_s + i * 5) * 1000,
        })
        .collect()
}

#[test]
fn a_recorded_trip_keeps_good_fixes_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    assert!(core.trips().is_empty());

    let trip = block_on(core.start_trip("  Coast path ".into())).unwrap();
    assert_eq!(trip.title, "Coast path");
    assert!(trip.recording);
    assert_eq!(
        block_on(core.start_trip("Another".into())),
        Err(FieldKitError::AlreadyRecording)
    );

    let mut batch = walk(0, 10);
    // A Wi-Fi fix, a repeat and a jump among them.
    let mut wifi = batch[3];
    wifi.horizontal_accuracy_m = 65.0;
    let repeat = batch[5];
    let mut jump = batch[6];
    jump.latitude += 0.05;
    jump.timestamp_ms += 1000;
    batch.extend([wifi, repeat, jump]);
    let recorded = block_on(core.record(batch)).unwrap();
    assert_eq!(
        recorded,
        Recorded {
            kept: 10,
            inaccurate: 1,
            skipped: 2
        }
    );

    let live = core.recording().unwrap();
    assert_eq!(live.point_count, 10);
    assert!((live.distance_m - 100.0).abs() < 1.0, "{}", live.distance_m);
    assert_eq!(live.climb_m, 9.0);

    // The app is ended mid-walk: the trip is still recording on relaunch,
    // and recording carries on from the last kept fix.
    core.shutdown();
    drop(core);
    let core = open(&dir).unwrap();
    assert_eq!(core.recording().map(|t| t.id), Some(trip.id));
    let more = block_on(core.record(walk(50, 5))).unwrap();
    assert_eq!(more.kept, 5);
    let finished = block_on(core.stop_trip()).unwrap();
    assert!(!finished.recording);
    assert!(finished.ended_ms.is_some());
    assert_eq!(finished.point_count, 15);
    assert!(core.recording().is_none());
    assert_eq!(block_on(core.track(trip.id)).unwrap().len(), 15);
    assert_eq!(
        block_on(core.record(walk(200, 1))),
        Err(FieldKitError::NotRecording)
    );
}

#[test]
fn notes_take_the_current_position_and_their_files_are_cleaned_up() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let trip = block_on(core.start_trip("Survey".into())).unwrap();
    block_on(core.record(walk(0, 3))).unwrap();

    let text = block_on(core.add_text_note(trip.id, "Stile is broken".into(), None)).unwrap();
    let photo_source = dir.path().join("IMG_0001.jpg");
    std::fs::write(&photo_source, b"pretend jpeg").unwrap();
    let photo = block_on(core.add_photo_note(
        trip.id,
        photo_source.to_string_lossy().into_owned(),
        "Gate".into(),
        Some(Position {
            latitude: 51.0,
            longitude: -1.0,
        }),
    ))
    .unwrap();
    // The same photo again: one stored file, two notes.
    let again = block_on(core.add_photo_note(
        trip.id,
        photo_source.to_string_lossy().into_owned(),
        String::new(),
        None,
    ))
    .unwrap();

    let detail = block_on(core.trip(trip.id)).unwrap();
    assert_eq!(detail.summary.note_count, 3);
    let notes = detail.notes;
    assert_eq!(notes[0].id, text);
    assert_eq!(
        notes[0].kind,
        NoteKind::Text {
            body: "Stile is broken".into()
        }
    );
    // Taken while recording: placed at the last kept fix.
    let at = notes[0].at.unwrap();
    assert!((at.latitude - 51.5002).abs() < 1e-9);
    assert_eq!(notes[1].id, photo);
    assert_eq!(
        notes[1].at,
        Some(Position {
            latitude: 51.0,
            longitude: -1.0
        })
    );
    let NoteKind::Photo { path, caption } = &notes[1].kind else {
        panic!("not a photo: {:?}", notes[1].kind);
    };
    assert_eq!(caption, "Gate");
    assert_eq!(std::fs::read(path).unwrap(), b"pretend jpeg");
    let stored = std::path::PathBuf::from(path);

    block_on(core.remove_note(photo)).unwrap();
    assert!(stored.exists(), "still used by the other note");
    block_on(core.remove_note(again)).unwrap();
    assert!(!stored.exists());

    assert_eq!(
        block_on(core.add_text_note(trip.id, "   ".into(), None)),
        Err(FieldKitError::EmptyNote)
    );
    assert_eq!(
        block_on(core.add_text_note(999, "Lost".into(), None)),
        Err(FieldKitError::NotFound { id: 999 })
    );
    assert!(matches!(
        block_on(core.add_photo_note(trip.id, "/no/such/file.jpg".into(), String::new(), None)),
        Err(FieldKitError::File { .. } | FieldKitError::Storage { .. })
    ));
}

#[test]
fn deleting_a_trip_removes_its_track_notes_and_files() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let trip = block_on(core.start_trip("Morning".into())).unwrap();
    block_on(core.record(walk(0, 4))).unwrap();
    let memo = dir.path().join("memo.m4a");
    std::fs::write(&memo, b"pretend audio").unwrap();
    block_on(core.add_voice_note(trip.id, memo.to_string_lossy().into_owned(), 4200, None))
        .unwrap();
    let detail = block_on(core.trip(trip.id)).unwrap();
    let NoteKind::Voice { path, duration_ms } = &detail.notes[0].kind else {
        panic!("not a voice note");
    };
    assert_eq!(*duration_ms, 4200);
    let stored = std::path::PathBuf::from(path);
    assert!(stored.exists());

    block_on(core.rename_trip(trip.id, "Morning loop".into())).unwrap();
    assert_eq!(core.trips()[0].title, "Morning loop");
    assert_eq!(
        block_on(core.rename_trip(trip.id, "".into())),
        Err(FieldKitError::EmptyTitle)
    );

    // Deleting the recording trip stops the recording.
    block_on(core.delete_trip(trip.id)).unwrap();
    assert!(core.trips().is_empty());
    assert!(core.recording().is_none());
    assert!(!stored.exists());
    assert_eq!(
        block_on(core.track(trip.id)),
        Err(FieldKitError::NotFound { id: trip.id })
    );
    assert_eq!(
        block_on(core.delete_trip(trip.id)),
        Err(FieldKitError::NotFound { id: trip.id })
    );
    // A new trip can start.
    block_on(core.start_trip("Evening".into())).unwrap();
}

#[test]
fn a_second_core_cannot_open_the_same_data() {
    let dir = tempfile::tempdir().unwrap();
    let _core = open(&dir).unwrap();
    assert!(matches!(open(&dir), Err(FieldKitError::InUse)));
}

#[test]
fn trips_are_listed_newest_first_and_each_change_is_announced() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let first = block_on(core.start_trip("First".into())).unwrap();
    block_on(core.stop_trip()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let second = block_on(core.start_trip("Second".into())).unwrap();
    let ids: Vec<i64> = core.trips().iter().map(|t| t.id).collect();
    assert_eq!(ids, vec![second.id, first.id]);
    assert_eq!(core.revision(), 3);
    // A refused command changes nothing.
    let _ = block_on(core.start_trip("Third".into()));
    assert_eq!(core.revision(), 3);
}
