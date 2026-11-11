//! The app's API as the SwiftUI app uses it, on the real runtime.

use std::sync::Arc;

use checklist_core::{ChecklistCore, ChecklistError, Summary};
use forge_app::{AppPhase, Cancellation, ErrorKind, RuntimeEvent};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

fn open(dir: &tempfile::TempDir) -> Result<Arc<ChecklistCore>, ChecklistError> {
    ChecklistCore::new(
        dir.path().join("data").to_string_lossy().into_owned(),
        dir.path().join("caches").to_string_lossy().into_owned(),
    )
}

#[test]
fn commands_save_then_announce_and_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let events = core.events().unwrap();
    let a = block_on(core.add("Buy milk".into())).unwrap();
    block_on(core.add("Call the vet".into())).unwrap();
    block_on(core.toggle(a)).unwrap();
    assert_eq!(
        block_on(events.next()),
        Some(RuntimeEvent::Changed { revision: 1 })
    );
    assert_eq!(core.revision(), 3);
    assert_eq!(core.summary(), Summary { total: 2, done: 1 });

    // A failed command changes nothing and announces nothing.
    assert_eq!(
        block_on(core.toggle(42)),
        Err(ChecklistError::NotFound { id: 42 })
    );
    assert_eq!(core.revision(), 3);

    core.shutdown();
    drop(core);
    let reopened = open(&dir).unwrap();
    assert_eq!(reopened.items().len(), 2);
    assert!(reopened.items()[0].done);
}

#[test]
fn a_second_core_cannot_open_the_same_checklist() {
    let dir = tempfile::tempdir().unwrap();
    let _first = open(&dir).unwrap();
    assert_eq!(open(&dir).err(), Some(ChecklistError::InUse));
}

#[test]
fn search_stops_for_the_background_and_for_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    block_on(core.add("Buy oat milk".into())).unwrap();
    block_on(core.add("Call the vet".into())).unwrap();
    core.set_phase(AppPhase::Active);
    let found = block_on(core.search("milk".into(), None)).unwrap();
    assert_eq!(found.len(), 1);

    let cancellation = Cancellation::new();
    cancellation.cancel();
    assert_eq!(
        block_on(core.search("milk".into(), Some(cancellation))),
        Err(ChecklistError::Stopped {
            kind: ErrorKind::Cancelled
        })
    );

    core.set_phase(AppPhase::Background);
    assert_eq!(
        block_on(core.search("milk".into(), None)),
        Err(ChecklistError::Stopped {
            kind: ErrorKind::Interrupted
        })
    );
    // Export is not on-screen work: it still runs in the background.
    let path = block_on(core.export_text(None)).unwrap();
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "[ ] Buy oat milk\n[ ] Call the vet\n"
    );
}

#[test]
fn after_shutdown_commands_are_refused_and_the_file_is_released() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    block_on(core.add("a".into())).unwrap();
    core.shutdown();
    assert_eq!(
        block_on(core.add("b".into())),
        Err(ChecklistError::Stopped {
            kind: ErrorKind::ShutDown
        })
    );
    assert!(core.events().is_none());
    drop(core);
    assert_eq!(open(&dir).unwrap().items().len(), 1);
}

#[test]
fn error_kinds() {
    use checklist_core::checklist_error_kind as kind;
    assert_eq!(kind(ChecklistError::EmptyTitle), ErrorKind::Invalid);
    assert_eq!(kind(ChecklistError::InUse), ErrorKind::Unavailable);
    assert_eq!(kind(ChecklistError::NewerData), ErrorKind::NewerData);
    assert_eq!(
        kind(ChecklistError::Stopped {
            kind: ErrorKind::Interrupted
        }),
        ErrorKind::Interrupted
    );
}

/// Every `fixtures/checklist-v<N>.json` (a file as version N of the app
/// saved it) opens with today's code and saves again, and there is one for
/// the current version. Old fixtures are never edited.
#[test]
fn every_saved_version_still_opens() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut versions = Vec::new();
    for entry in std::fs::read_dir(&fixtures).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(version) = name
            .strip_prefix("checklist-v")
            .and_then(|rest| rest.strip_suffix(".json"))
        else {
            continue;
        };
        versions.push(version.parse::<u32>().unwrap());
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        std::fs::copy(entry.path(), dir.path().join("data/checklist.json")).unwrap();
        let core = open(&dir).unwrap_or_else(|e| panic!("{name} does not open: {e:?}"));
        assert!(!core.items().is_empty(), "{name} lost its items");
        let before = core.items();
        block_on(core.add("After the upgrade".into())).unwrap();
        drop(core);
        let reopened = open(&dir).unwrap();
        assert_eq!(reopened.items()[..before.len()], before[..], "{name}");
    }
    assert!(
        versions.contains(&1),
        "add fixtures/checklist-v<N>.json for the current save version"
    );
}
