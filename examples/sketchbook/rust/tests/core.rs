//! Sketchbook's API as the SwiftUI app will use it, on the real runtime.

use std::sync::Arc;

use sketchbook_core::{ExportFormat, Paper, SketchbookCore, SketchbookError};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

fn open(dir: &tempfile::TempDir) -> Result<Arc<SketchbookCore>, SketchbookError> {
    SketchbookCore::new(
        dir.path().join("data").to_string_lossy().into_owned(),
        dir.path().join("caches").to_string_lossy().into_owned(),
    )
}

fn files(dir: &tempfile::TempDir, folder: &str) -> usize {
    std::fs::read_dir(dir.path().join("data").join(folder))
        .map(|d| d.count())
        .unwrap_or(0)
}

#[test]
fn strokes_and_layer_changes_share_one_undo_history() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let sketch = block_on(core.create_sketch(" Harbour ".into(), Paper::Grid)).unwrap();
    assert_eq!(sketch.summary.title, "Harbour");
    assert_eq!(sketch.layers.len(), 1);
    assert_eq!(sketch.undo_name, None);
    let id = sketch.summary.id;
    let base = sketch.layers[0].id;

    let drawn = block_on(core.draw(id, base, b"stroke one".to_vec())).unwrap();
    let first = drawn.layers[0].drawing_path.clone().unwrap();
    assert_eq!(std::fs::read(&first).unwrap(), b"stroke one");
    block_on(core.draw(id, base, b"stroke two".to_vec())).unwrap();
    let added = block_on(core.add_layer(id, None)).unwrap();
    assert_eq!(added.layers[1].name, "Layer 2");
    assert_eq!(added.undo_name.as_deref(), Some("Undo Add Layer"));

    // Undo takes back the layer, then the second stroke.
    let back = block_on(core.undo(id)).unwrap();
    assert_eq!(back.layers.len(), 1);
    let back = block_on(core.undo(id)).unwrap();
    assert_eq!(back.layers[0].drawing_path.as_deref(), Some(first.as_str()));
    assert_eq!(back.redo_name.as_deref(), Some("Redo Drawing"));
    let again = block_on(core.redo(id)).unwrap();
    assert_eq!(
        std::fs::read(again.layers[0].drawing_path.as_ref().unwrap()).unwrap(),
        b"stroke two"
    );
    // The queries see what the commands did, without waiting.
    assert_eq!(core.sketch(id).unwrap(), again);
    assert_eq!(core.sketches()[0].id, id);
}

#[test]
fn sketches_survive_a_restart_and_history_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let sketch = block_on(core.create_sketch("Harbour".into(), Paper::Plain)).unwrap();
    let id = sketch.summary.id;
    let layer = sketch.layers[0].id;
    block_on(core.draw(id, layer, b"boats".to_vec())).unwrap();
    block_on(core.set_layer_opacity(id, layer, 0.5)).unwrap();
    block_on(core.set_thumbnail(id, b"png".to_vec())).unwrap();
    core.shutdown();
    drop(core);

    let core = open(&dir).unwrap();
    let sketch = core.sketch(id).unwrap();
    assert_eq!(sketch.layers[0].opacity, 0.5);
    assert!(sketch.layers[0].drawing_path.is_some());
    assert!(sketch.summary.thumbnail_path.is_some());
    assert_eq!(sketch.undo_name, None);
    assert_eq!(block_on(core.undo(id)), Err(SketchbookError::NothingToUndo));
}

#[test]
fn drawings_go_when_nothing_needs_them() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let sketch = block_on(core.create_sketch("A".into(), Paper::Plain)).unwrap();
    let (id, layer) = (sketch.summary.id, sketch.layers[0].id);
    block_on(core.draw(id, layer, b"one".to_vec())).unwrap();
    block_on(core.draw(id, layer, b"two".to_vec())).unwrap();
    // "one" is still needed by undo.
    assert_eq!(files(&dir, "drawings"), 2);

    // A copy shares the drawing file.
    let copy = block_on(core.duplicate_sketch(id)).unwrap();
    assert_eq!(copy.summary.title, "A Copy");
    assert_eq!(files(&dir, "drawings"), 2);

    block_on(core.set_thumbnail(id, b"png".to_vec())).unwrap();
    block_on(core.delete_sketch(id)).unwrap();
    // "one" went with the history; "two" stays for the copy.
    assert_eq!(files(&dir, "drawings"), 1);
    assert_eq!(files(&dir, "thumbnails"), 0);

    block_on(core.delete_sketch(copy.summary.id)).unwrap();
    assert_eq!(files(&dir, "drawings"), 0);
}

#[test]
fn a_failed_change_leaves_everything_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let sketch = block_on(core.create_sketch("A".into(), Paper::Plain)).unwrap();
    let (id, layer) = (sketch.summary.id, sketch.layers[0].id);
    let revision = core.revision();
    assert_eq!(
        block_on(core.delete_layer(id, layer)),
        Err(SketchbookError::LastLayer)
    );
    assert_eq!(
        block_on(core.set_layer_opacity(id, layer, 1.5)),
        Err(SketchbookError::BadOpacity)
    );
    assert_eq!(
        block_on(core.draw(999, layer, b"x".to_vec())),
        Err(SketchbookError::NotFound { id: 999 })
    );
    assert_eq!(
        block_on(core.create_sketch("   ".into(), Paper::Plain)),
        Err(SketchbookError::EmptyName)
    );
    assert_eq!(core.revision(), revision);
    assert_eq!(core.sketch(id).unwrap(), sketch);
    assert_eq!(files(&dir, "drawings"), 0);
}

#[test]
fn export_plans_skip_hidden_and_empty_layers() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir).unwrap();
    let a = block_on(core.create_sketch("Harbour".into(), Paper::Lined)).unwrap();
    let id = a.summary.id;
    let base = a.layers[0].id;
    block_on(core.draw(id, base, b"sea".to_vec())).unwrap();
    let with_top = block_on(core.add_layer(id, Some("Boats".into()))).unwrap();
    let top = with_top.layers[1].id;
    block_on(core.draw(id, top, b"boats".to_vec())).unwrap();
    block_on(core.add_layer(id, None)).unwrap(); // empty
    block_on(core.set_layer_visible(id, top, false)).unwrap();
    let b = block_on(core.create_sketch("Harbour".into(), Paper::Plain)).unwrap();

    let plan = core
        .export_plan(vec![id, b.summary.id], ExportFormat::Png)
        .unwrap();
    assert_eq!(plan.pages[0].layers.len(), 1);
    assert_eq!(plan.pages[0].paper, Paper::Lined);
    assert!(plan.pages[1].layers.is_empty());
    assert!(plan.paths[0].ends_with("exports/Harbour.png"));
    assert!(plan.paths[1].ends_with("exports/Harbour 2.png"));

    let pdf = core
        .export_plan(vec![id, b.summary.id], ExportFormat::Pdf)
        .unwrap();
    assert_eq!(pdf.paths.len(), 1);
    assert!(pdf.paths[0].ends_with("Sketches.pdf"));
    assert_eq!(
        core.export_plan(vec![], ExportFormat::Pdf),
        Err(SketchbookError::NothingToExport)
    );
}

#[test]
fn an_unreadable_file_waits_for_the_person() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::write(dir.path().join("data/sketchbook.json"), b"{not json").unwrap();
    assert!(matches!(open(&dir), Err(SketchbookError::Corrupt { .. })));

    let core = SketchbookCore::start_fresh(
        dir.path().join("data").to_string_lossy().into_owned(),
        dir.path().join("caches").to_string_lossy().into_owned(),
    )
    .unwrap();
    assert!(core.sketches().is_empty());
    let kept = std::fs::read_dir(dir.path().join("data"))
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains("unreadable"));
    assert!(kept, "the old file is kept");
}
