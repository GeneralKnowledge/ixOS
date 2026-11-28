//! The app's API as the SwiftUI app uses it, on the real runtime.

use std::sync::Arc;

use checklist_core::{checklist_error_kind, ChecklistCore, ChecklistError, Summary};
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

#[test]
fn the_checklist_goes_out_and_comes_back_as_a_spreadsheet() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let milk = block_on(core.add("Buy oat milk".into())).unwrap();
    block_on(core.add("Call the vet".into())).unwrap();
    block_on(core.toggle(milk)).unwrap();
    let path = block_on(core.export_csv(None)).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text.trim_start_matches('\u{feff}'),
        "Item,Done,Due\nBuy oat milk,Yes,\nCall the vet,No,\n"
    );

    // A spreadsheet someone edited: other column order, an extra column, a
    // blank title and a row without the Done column.
    let edited = dir.path().join("edited.csv");
    std::fs::write(
        &edited,
        "done,Notes,item\nyes,,Pack the tent\n,,\nno,x,  \n,,Book the ferry\n",
    )
    .unwrap();
    let summary = block_on(core.import_csv(edited.to_string_lossy().into_owned())).unwrap();
    assert_eq!(summary.added, 2, "{summary:?}");
    assert_eq!(summary.skipped.len(), 1, "{summary:?}");
    assert!(summary.skipped[0].contains("needs a title"), "{summary:?}");
    // Lines of the file, counting the header: the blank title is line 4.
    assert!(summary.skipped[0].starts_with("Line 4:"), "{summary:?}");
    let items = core.items();
    assert_eq!(items.len(), 4);
    assert_eq!(
        (items[2].title.as_str(), items[2].done),
        ("Pack the tent", true)
    );
    assert_eq!(
        (items[3].title.as_str(), items[3].done),
        ("Book the ferry", false)
    );

    // The same file again adds nothing new, and says so.
    let again = block_on(core.import_csv(edited.to_string_lossy().into_owned())).unwrap();
    assert_eq!(again.added, 0, "{again:?}");
    assert_eq!(
        again.skipped,
        vec![
            "Line 4: an item needs a title".to_string(),
            "2 items were already on the list".to_string()
        ]
    );
    assert_eq!(core.items().len(), 4);

    let junk = dir.path().join("junk.csv");
    std::fs::write(&junk, "").unwrap();
    let err = block_on(core.import_csv(junk.to_string_lossy().into_owned())).unwrap_err();
    assert!(matches!(err, ChecklistError::Storage { .. }), "{err:?}");
}

/// A cell with a double space or a tab is saved tidied ("Buy milk"), and
/// importing the file again still finds it on the list (defect hunt
/// finding 6).
#[test]
fn reimporting_finds_items_that_were_tidied_on_the_way_in() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let file = dir.path().join("spaced.csv");
    std::fs::write(&file, "Item,Done\nBuy  milk,No\nCall\tthe vet,No\n").unwrap();
    let first = block_on(core.import_csv(file.to_string_lossy().into_owned())).unwrap();
    assert_eq!(first.added, 2, "{first:?}");
    let titles: Vec<String> = core.items().iter().map(|i| i.title.clone()).collect();
    assert_eq!(titles, vec!["Buy milk", "Call the vet"]);

    let again = block_on(core.import_csv(file.to_string_lossy().into_owned())).unwrap();
    assert_eq!(again.added, 0, "{again:?}");
    assert_eq!(
        again.skipped,
        vec!["2 items were already on the list".to_string()]
    );
    assert_eq!(core.items().len(), 2);
}

/// Due days are saved with the item, survive a relaunch, sort the agenda,
/// and go out to and come back from a spreadsheet; a row whose day isn't
/// one is skipped and says why.
#[test]
fn due_days_are_kept_shown_in_the_agenda_and_exported() {
    let dir = tempfile::tempdir().unwrap();
    {
        let core = open(&dir).unwrap();
        let vet = block_on(core.add("Call the vet".into())).unwrap();
        let tent = block_on(core.add("Pack the tent".into())).unwrap();
        block_on(core.add("Someday".into())).unwrap();
        block_on(core.set_due(vet, Some("2026-11-21".into()))).unwrap();
        block_on(core.set_due(tent, Some("2026-11-24".into()))).unwrap();
        let err = block_on(core.set_due(tent, Some("2026-11-31".into()))).unwrap_err();
        assert!(matches!(err, ChecklistError::InvalidDay { .. }), "{err:?}");
        assert_eq!(checklist_error_kind(err), ErrorKind::Invalid);
        core.shutdown();
    }
    let core = open(&dir).unwrap();
    let agenda = core.agenda("2026-11-22".into());
    assert_eq!(agenda.overdue.len(), 1);
    assert_eq!(agenda.overdue[0].title, "Call the vet");
    assert!(agenda.today.is_empty());
    assert_eq!(agenda.upcoming[0].due.as_deref(), Some("2026-11-24"));
    assert_eq!(core.agenda("2026-11-24".into()).today.len(), 1);

    let path = block_on(core.export_csv(None)).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text.trim_start_matches('\u{feff}'),
        "Item,Done,Due\nCall the vet,No,2026-11-21\nPack the tent,No,2026-11-24\nSomeday,No,\n"
    );

    let file = dir.path().join("dated.csv");
    std::fs::write(
        &file,
        "Item,Due\nBuy stamps,2026-12-01\nPost the parcel,next week\nWater plants,\n",
    )
    .unwrap();
    let summary = block_on(core.import_csv(file.to_string_lossy().into_owned())).unwrap();
    assert_eq!(summary.added, 2, "{summary:?}");
    assert_eq!(summary.skipped.len(), 1, "{summary:?}");
    assert!(
        summary.skipped[0].starts_with("Line 3:") && summary.skipped[0].contains("not a day"),
        "{summary:?}"
    );
    let items = core.items();
    assert_eq!(items[3].due.as_deref(), Some("2026-12-01"));
    assert_eq!(items[4].due, None);

    block_on(core.set_due(items[3].id, None)).unwrap();
    assert_eq!(core.items()[3].due, None);
}
