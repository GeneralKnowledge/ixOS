//! Opening Soundscape's core on the real runtime.

use std::sync::Arc;

use soundscape_core::{SoundscapeCore, SoundscapeError};

fn open(dir: &tempfile::TempDir) -> Result<Arc<SoundscapeCore>, SoundscapeError> {
    SoundscapeCore::new(
        dir.path().join("data").to_string_lossy().into_owned(),
        dir.path().join("caches").to_string_lossy().into_owned(),
    )
}

#[test]
fn one_core_at_a_time_holds_the_data() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    assert!(matches!(open(&dir), Err(SoundscapeError::InUse)));
    drop(core);
    let again = open(&dir).unwrap();
    assert!(again.library().tracks.is_empty());
}

#[test]
fn old_studio_files_are_left_alone() {
    // Studio (loop projects) was removed; a phone that used it still has
    // its project list and recordings, which stay untouched.
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir_all(data.join("recordings")).unwrap();
    std::fs::write(data.join("soundscape.json"), "not what it expects").unwrap();
    std::fs::write(data.join("recordings").join("a.wav"), b"RIFF").unwrap();
    let core = open(&dir).unwrap();
    assert!(core.library().tracks.is_empty());
    drop(core);
    assert_eq!(
        std::fs::read_to_string(data.join("soundscape.json")).unwrap(),
        "not what it expects"
    );
    assert!(data.join("recordings").join("a.wav").exists());
}
