//! Sketchbook: a drawing notebook. Sketches of finger drawings with layers,
//! undo and redo, exported to PNG and PDF.
//!
//! The app's whole Rust side. What a sketch is and what undo does live in
//! [`book`]; this file is the API the SwiftUI app calls, built on the
//! application runtime (`forge_app`):
//!
//! - **PencilKit draws, Sketchbook keeps.** Each layer is a `PKCanvasView`
//!   on screen. When a stroke ends, the app passes the layer's drawing
//!   (`PKDrawing.dataRepresentation()`) to `draw`, which stores it as a file
//!   named by its SHA-256 and records an undo step. Rust never parses a
//!   drawing.
//! - **Undo is Rust's**, not `UndoManager`'s: layer changes (add, delete,
//!   move, hide, opacity, paper) and strokes share one history per sketch,
//!   so Undo takes back whichever came last. The canvas's own undo manager
//!   stays off.
//! - **Commands** run one at a time on the runtime's command thread and
//!   save the sketch list (one JSON document) before they answer.
//! - **Queries** (`sketches`, `sketch`) read a published snapshot and never
//!   wait for a command.
//! - **Export** is planned here and rendered by PencilKit: `export_plan`
//!   says which layers go on which page and where each file goes.

pub mod book;
mod model;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use forge_app::{DataDirectory, Document, Published, Runtime};
use forge_runtime::now_ms;
use forge_storage::FileStore;
use sha2::Digest;

use book::{Book, Edit, History, Page};
pub use model::{
    sketchbook_error_kind, ExportFormat, ExportLayer, ExportPage, ExportPlan, Layer, Paper,
    SketchDetail, SketchSummary, SketchbookError, MAX_LAYERS, MAX_NAME_CHARS, MAX_UNDO_STEPS,
};

uniffi::setup_scaffolding!();

// `forge_log` for the host: Swift's `ForgeLog("Category")` writes to the same
// log as Rust.
forge_runtime::log_export!();

const SAVE_FILE: &str = "sketchbook.json";
/// A drawing bigger than this is refused. A full page of dense strokes is a
/// few megabytes.
const MAX_DRAWING_BYTES: u64 = 32 * 1024 * 1024;
/// A thumbnail bigger than this is refused.
const MAX_THUMBNAIL_BYTES: u64 = 2 * 1024 * 1024;

/// What the command thread owns.
struct State {
    book: Document<Book>,
    /// Undo and redo per sketch, in memory.
    histories: HashMap<u64, History>,
    drawings: FileStore,
    thumbnails: FileStore,
    published: Arc<Published<Vec<SketchDetail>>>,
}

impl State {
    fn publish(&self) {
        self.published.set(details(
            self.book.value(),
            &self.histories,
            &self.drawings,
            &self.thumbnails,
        ));
    }

    fn detail(&self, id: u64) -> Result<SketchDetail, SketchbookError> {
        self.published
            .get()
            .iter()
            .find(|s| s.summary.id == id)
            .cloned()
            .ok_or(SketchbookError::NotFound { id })
    }

    /// Runs a change to the book that undo doesn't cover, saves it, then
    /// publishes and removes drawings nothing needs any more.
    fn change<T>(
        &mut self,
        edit: impl FnOnce(&mut Book) -> Result<T, SketchbookError>,
    ) -> Result<T, SketchbookError> {
        let before = self.in_use();
        let value = self.book.change(edit)?;
        self.histories
            .retain(|id, _| self.book.value().sketch(*id).is_ok());
        self.settle(before);
        Ok(value)
    }

    /// Applies an edit the person can undo, saves it, and records the step.
    fn edit(&mut self, id: u64, edit: Edit) -> Result<SketchDetail, SketchbookError> {
        let now = now_ms();
        let in_use = self.in_use();
        let before = self.book.change(|book| book.apply(id, &edit, now))?;
        self.histories
            .entry(id)
            .or_default()
            .record(&edit, before, now);
        self.settle(in_use);
        self.detail(id)
    }

    /// Undo (`forward` false) or redo. The history changes only if the save
    /// does.
    fn step(&mut self, id: u64, forward: bool) -> Result<SketchDetail, SketchbookError> {
        let now = now_ms();
        let in_use = self.in_use();
        let mut history = self.histories.get(&id).cloned().unwrap_or_default();
        let current = self.book.value().sketch(id)?.page.clone();
        let page: Page = if forward {
            history.redo(current)?
        } else {
            history.undo(current)?
        };
        self.book.change(|book| book.restore(id, page, now))?;
        self.histories.insert(id, history);
        self.settle(in_use);
        self.detail(id)
    }

    fn in_use(&self) -> HashSet<String> {
        book::drawings_in_use(self.book.value(), &self.histories)
    }

    /// Publishes, then deletes the drawings that were in use before the
    /// change and aren't now. Only commands write drawings, and they run
    /// one at a time, so nothing is being written meanwhile.
    fn settle(&mut self, before: HashSet<String>) {
        let after = self.in_use();
        for name in before.difference(&after) {
            self.drawings.remove(name);
        }
        let live: HashSet<u64> = self.book.value().sketches.iter().map(|s| s.id).collect();
        for name in self.thumbnails.names() {
            if thumbnail_owner(&name).is_some_and(|id| !live.contains(&id)) {
                self.thumbnails.remove(&name);
            }
        }
        self.publish();
    }

    /// Keeps drawing bytes as the file `name` (their SHA-256), once.
    fn keep_drawing(&self, name: &str, data: &[u8]) -> Result<(), SketchbookError> {
        if self.drawings.get(name).is_none() {
            self.drawings.write(name, data)?;
        }
        Ok(())
    }
}

/// The app's core. The Swift app opens one per process and shares it.
#[derive(uniffi::Object)]
pub struct SketchbookCore {
    rt: Runtime<State>,
    sketches: Arc<Published<Vec<SketchDetail>>>,
    caches: PathBuf,
}

#[uniffi::export]
impl SketchbookCore {
    /// Opens the sketches saved in `data_directory` (created if missing),
    /// holding the directory so no second core can write to it.
    /// `caches_directory` is for files the app can lose (exports). A saved
    /// file that can't be read is an error (`Corrupt`); `start_fresh` sets
    /// it aside once the person agrees.
    #[uniffi::constructor]
    pub fn new(
        data_directory: String,
        caches_directory: String,
    ) -> Result<Arc<Self>, SketchbookError> {
        forge_runtime::logging::init();
        Self::open(&data_directory, &caches_directory, false)
    }

    /// Opens with an empty sketchbook in place of one that can't be read,
    /// which is kept next to it. Refused for data from a newer version.
    #[uniffi::constructor]
    pub fn start_fresh(
        data_directory: String,
        caches_directory: String,
    ) -> Result<Arc<Self>, SketchbookError> {
        forge_runtime::logging::init();
        Self::open(&data_directory, &caches_directory, true)
    }

    /// Every sketch, most recently changed first. Never waits for a command.
    pub fn sketches(&self) -> Vec<SketchSummary> {
        self.sketches
            .get()
            .iter()
            .map(|s| s.summary.clone())
            .collect()
    }

    /// A sketch with its layers and what undo and redo would do. Never
    /// waits for a command.
    pub fn sketch(&self, id: u64) -> Result<SketchDetail, SketchbookError> {
        self.sketches
            .get()
            .iter()
            .find(|s| s.summary.id == id)
            .cloned()
            .ok_or(SketchbookError::NotFound { id })
    }

    /// A new sketch with one empty layer.
    pub async fn create_sketch(
        &self,
        title: String,
        paper: Paper,
    ) -> Result<SketchDetail, SketchbookError> {
        self.rt
            .command("create-sketch", move |state| {
                let id = state.change(|book| book.create(&title, paper, now_ms()))?;
                state.detail(id)
            })
            .await
    }

    /// A copy of a sketch, called "… Copy". Its undo history starts empty.
    pub async fn duplicate_sketch(&self, id: u64) -> Result<SketchDetail, SketchbookError> {
        self.rt
            .command("duplicate-sketch", move |state| {
                let copy = state.change(|book| book.duplicate(id, now_ms()))?;
                state.detail(copy)
            })
            .await
    }

    pub async fn rename_sketch(&self, id: u64, title: String) -> Result<(), SketchbookError> {
        self.rt
            .command("rename-sketch", move |state| {
                state.change(|book| book.rename(id, &title, now_ms()))
            })
            .await
    }

    /// Deletes a sketch, its history and the drawings only it used.
    pub async fn delete_sketch(&self, id: u64) -> Result<(), SketchbookError> {
        self.rt
            .command("delete-sketch", move |state| {
                state.change(|book| book.delete(id))
            })
            .await
    }

    /// A layer's new drawing, after a stroke ended (or an erase, or a
    /// lasso move). One undo step. The same drawing twice changes nothing.
    pub async fn draw(
        &self,
        sketch_id: u64,
        layer_id: u64,
        drawing: Vec<u8>,
    ) -> Result<SketchDetail, SketchbookError> {
        self.rt
            .command("draw", move |state| {
                let sketch = state.book.value().sketch(sketch_id)?;
                let name = hex(&sha2::Sha256::digest(&drawing));
                let unchanged = sketch
                    .page
                    .layers
                    .iter()
                    .any(|l| l.id == layer_id && l.drawing.as_deref() == Some(name.as_str()));
                if unchanged {
                    return state.detail(sketch_id);
                }
                state.keep_drawing(&name, &drawing)?;
                let result = state.edit(
                    sketch_id,
                    Edit::Draw {
                        layer: layer_id,
                        drawing: Some(name.clone()),
                    },
                );
                if result.is_err() && !state.in_use().contains(&name) {
                    state.drawings.remove(&name);
                }
                result
            })
            .await
    }

    /// Empties a layer. One undo step.
    pub async fn clear_layer(
        &self,
        sketch_id: u64,
        layer_id: u64,
    ) -> Result<SketchDetail, SketchbookError> {
        self.edit(
            "clear-layer",
            sketch_id,
            Edit::Draw {
                layer: layer_id,
                drawing: None,
            },
        )
        .await
    }

    /// Adds an empty layer on top, named "Layer N" when `name` is `None`.
    pub async fn add_layer(
        &self,
        sketch_id: u64,
        name: Option<String>,
    ) -> Result<SketchDetail, SketchbookError> {
        self.edit("add-layer", sketch_id, Edit::AddLayer { name })
            .await
    }

    pub async fn delete_layer(
        &self,
        sketch_id: u64,
        layer_id: u64,
    ) -> Result<SketchDetail, SketchbookError> {
        self.edit(
            "delete-layer",
            sketch_id,
            Edit::RemoveLayer { layer: layer_id },
        )
        .await
    }

    /// Puts a copy of a layer just above it.
    pub async fn duplicate_layer(
        &self,
        sketch_id: u64,
        layer_id: u64,
    ) -> Result<SketchDetail, SketchbookError> {
        self.edit(
            "duplicate-layer",
            sketch_id,
            Edit::DuplicateLayer { layer: layer_id },
        )
        .await
    }

    /// Moves a layer to `to`, counted from the bottom (0).
    pub async fn move_layer(
        &self,
        sketch_id: u64,
        layer_id: u64,
        to: u32,
    ) -> Result<SketchDetail, SketchbookError> {
        self.edit(
            "move-layer",
            sketch_id,
            Edit::MoveLayer {
                layer: layer_id,
                to,
            },
        )
        .await
    }

    pub async fn rename_layer(
        &self,
        sketch_id: u64,
        layer_id: u64,
        name: String,
    ) -> Result<SketchDetail, SketchbookError> {
        self.edit(
            "rename-layer",
            sketch_id,
            Edit::RenameLayer {
                layer: layer_id,
                name,
            },
        )
        .await
    }

    pub async fn set_layer_visible(
        &self,
        sketch_id: u64,
        layer_id: u64,
        visible: bool,
    ) -> Result<SketchDetail, SketchbookError> {
        self.edit(
            "set-layer-visible",
            sketch_id,
            Edit::SetVisible {
                layer: layer_id,
                visible,
            },
        )
        .await
    }

    /// From 0 to 1. Call it as a slider moves: changes to one layer less
    /// than two seconds apart are one undo step.
    pub async fn set_layer_opacity(
        &self,
        sketch_id: u64,
        layer_id: u64,
        opacity: f64,
    ) -> Result<SketchDetail, SketchbookError> {
        self.edit(
            "set-layer-opacity",
            sketch_id,
            Edit::SetOpacity {
                layer: layer_id,
                opacity,
            },
        )
        .await
    }

    pub async fn set_paper(
        &self,
        sketch_id: u64,
        paper: Paper,
    ) -> Result<SketchDetail, SketchbookError> {
        self.edit("set-paper", sketch_id, Edit::SetPaper { paper })
            .await
    }

    pub async fn undo(&self, sketch_id: u64) -> Result<SketchDetail, SketchbookError> {
        self.rt
            .command("undo", move |state| state.step(sketch_id, false))
            .await
    }

    pub async fn redo(&self, sketch_id: u64) -> Result<SketchDetail, SketchbookError> {
        self.rt
            .command("redo", move |state| state.step(sketch_id, true))
            .await
    }

    /// The list's picture of a sketch: a PNG the app rendered after a
    /// change. Not an undo step.
    pub async fn set_thumbnail(&self, sketch_id: u64, png: Vec<u8>) -> Result<(), SketchbookError> {
        self.rt
            .command("set-thumbnail", move |state| {
                state.book.value().sketch(sketch_id)?;
                state.thumbnails.write(&thumbnail_name(sketch_id), &png)?;
                state.publish();
                Ok(())
            })
            .await
    }

    /// What to render to export `sketch_ids`, in that order, and where to
    /// write it: PNG is a file per sketch, PDF one file with a page per
    /// sketch. Hidden and empty layers are left out. Never waits for a
    /// command.
    pub fn export_plan(
        &self,
        sketch_ids: Vec<u64>,
        format: ExportFormat,
    ) -> Result<ExportPlan, SketchbookError> {
        if sketch_ids.is_empty() {
            return Err(SketchbookError::NothingToExport);
        }
        let pages = sketch_ids
            .iter()
            .map(|id| self.sketch(*id).map(|s| export_page(&s)))
            .collect::<Result<Vec<_>, _>>()?;
        let folder = self.caches.join("exports");
        std::fs::create_dir_all(&folder).map_err(|err| SketchbookError::Storage {
            reason: format!("create the export folder: {err}"),
        })?;
        let titles: Vec<&str> = match format {
            ExportFormat::Png => pages.iter().map(|p| p.title.as_str()).collect(),
            ExportFormat::Pdf if pages.len() == 1 => vec![pages[0].title.as_str()],
            ExportFormat::Pdf => vec!["Sketches"],
        };
        let paths = unique_names(&titles)
            .into_iter()
            .map(|name| {
                folder
                    .join(format!("{name}.{}", format.extension()))
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        Ok(ExportPlan {
            format,
            pages,
            paths,
        })
    }
}

impl SketchbookCore {
    fn open(
        data_directory: &str,
        caches_directory: &str,
        fresh: bool,
    ) -> Result<Arc<Self>, SketchbookError> {
        let directory = DataDirectory::open(data_directory)?;
        let path = directory.path().join(SAVE_FILE);
        let book = if fresh {
            let (book, kept) = Document::<Book>::start_fresh(&path, book::VERSION, None, now_ms())?;
            if let Some(kept) = kept {
                forge_runtime::logging::warn!(
                    "unreadable sketches set aside as {}",
                    kept.display()
                );
            }
            book
        } else {
            Document::<Book>::open(&path, book::VERSION)?
        };
        let drawings =
            FileStore::new(directory.path().join("drawings")).with_max_bytes(MAX_DRAWING_BYTES);
        let thumbnails =
            FileStore::new(directory.path().join("thumbnails")).with_max_bytes(MAX_THUMBNAIL_BYTES);
        // A crash between writing a drawing and saving the sketch leaves a
        // file nothing names; at launch, before any command, it can go.
        let saved = book.value().drawings();
        drawings.sweep(|name| saved.contains(name));
        thumbnails.sweep_temporaries();
        let state = State {
            book,
            histories: HashMap::new(),
            drawings,
            thumbnails,
            published: Arc::new(Published::new(Vec::new())),
        };
        state.publish();
        let sketches = Arc::clone(&state.published);
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("sketchbook", directory, state),
            sketches,
            caches: PathBuf::from(caches_directory),
        }))
    }

    async fn edit(
        &self,
        name: &'static str,
        sketch_id: u64,
        edit: Edit,
    ) -> Result<SketchDetail, SketchbookError> {
        self.rt
            .command(name, move |state| state.edit(sketch_id, edit))
            .await
    }
}

/// Every sketch for the screen, most recently changed first.
fn details(
    book: &Book,
    histories: &HashMap<u64, History>,
    drawings: &FileStore,
    thumbnails: &FileStore,
) -> Vec<SketchDetail> {
    let path = |store: &FileStore, name: &str| {
        store
            .get(name)
            .map(|p: PathBuf| p.to_string_lossy().into_owned())
    };
    let mut list: Vec<SketchDetail> = book
        .sketches
        .iter()
        .map(|sketch| {
            let history = histories.get(&sketch.id);
            SketchDetail {
                summary: SketchSummary {
                    id: sketch.id,
                    title: sketch.title.clone(),
                    created_ms: sketch.created_ms,
                    modified_ms: sketch.modified_ms,
                    paper: sketch.page.paper,
                    layer_count: sketch.page.layers.len() as u32,
                    thumbnail_path: path(thumbnails, &thumbnail_name(sketch.id)),
                },
                layers: sketch
                    .page
                    .layers
                    .iter()
                    .map(|l| Layer {
                        id: l.id,
                        name: l.name.clone(),
                        visible: l.visible,
                        opacity: l.opacity,
                        drawing_path: l.drawing.as_deref().and_then(|d| path(drawings, d)),
                    })
                    .collect(),
                undo_name: history.and_then(History::undo_name),
                redo_name: history.and_then(History::redo_name),
            }
        })
        .collect();
    list.sort_by(|a, b| {
        b.summary
            .modified_ms
            .cmp(&a.summary.modified_ms)
            .then(b.summary.id.cmp(&a.summary.id))
    });
    list
}

fn export_page(sketch: &SketchDetail) -> ExportPage {
    ExportPage {
        sketch_id: sketch.summary.id,
        title: sketch.summary.title.clone(),
        paper: sketch.summary.paper,
        layers: sketch
            .layers
            .iter()
            .filter(|l| l.visible && l.opacity > 0.0)
            .filter_map(|l| {
                Some(ExportLayer {
                    drawing_path: l.drawing_path.clone()?,
                    opacity: l.opacity,
                })
            })
            .collect(),
    }
}

fn thumbnail_name(sketch_id: u64) -> String {
    format!("{sketch_id}.png")
}

fn thumbnail_owner(name: &str) -> Option<u64> {
    Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.parse().ok())
}

/// File names from titles: letters, digits, spaces and dashes, with " 2",
/// " 3" added to repeats.
fn unique_names(titles: &[&str]) -> Vec<String> {
    let mut seen: HashMap<String, u32> = HashMap::new();
    titles
        .iter()
        .map(|title| {
            let cleaned: String = title
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == ' ' || c == '-' {
                        c
                    } else {
                        ' '
                    }
                })
                .collect();
            let mut name = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
            if name.is_empty() {
                name = "Sketch".into();
            }
            let count = seen.entry(name.to_lowercase()).or_insert(0);
            *count += 1;
            if *count > 1 {
                format!("{name} {count}")
            } else {
                name
            }
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

forge_app::export_runtime!(SketchbookCore, rt);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_names_are_clean_and_unique() {
        assert_eq!(
            unique_names(&["Harbour/Night", "harbour night", "", "Harbour Night"]),
            vec![
                "Harbour Night",
                "harbour night 2",
                "Sketch",
                "Harbour Night 3"
            ]
        );
    }

    #[test]
    fn thumbnails_name_their_sketch() {
        assert_eq!(thumbnail_owner(&thumbnail_name(42)), Some(42));
        assert_eq!(thumbnail_owner("notes.txt"), None);
    }
}
