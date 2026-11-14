//! What a sketch is and what each change does to it, with no files and no
//! threads, so every rule is tested on Linux.
//!
//! - [`Book`] is everything Sketchbook saves: one JSON document.
//! - A sketch's [`Page`] is its paper and its layers. Every change the
//!   person can undo is an [`Edit`] applied to a page.
//! - [`History`] is undo and redo for one sketch: each step keeps the whole
//!   page from before. A page is small (drawings are file names), so this
//!   is simpler and safer than inverting each edit.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::model::{clean_name, Paper, SketchbookError, MAX_LAYERS, MAX_UNDO_STEPS};

/// The saved document's version. Raise it, and add a migration, on any
/// change an older file can't be read as.
pub const VERSION: u32 = 1;

/// Two opacity changes to the same layer this close together are one undo
/// step: a slider sends many.
const COALESCE_MS: i64 = 2_000;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Book {
    /// The next id for a sketch or layer. Ids are never reused, so an undo
    /// step that brings a layer back brings back its id too.
    #[serde(default = "first_id")]
    pub next_id: u64,
    #[serde(default)]
    pub sketches: Vec<Sketch>,
}

fn first_id() -> u64 {
    1
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Sketch {
    pub id: u64,
    pub title: String,
    pub created_ms: i64,
    pub modified_ms: i64,
    pub page: Page,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Page {
    #[serde(default)]
    pub paper: Paper,
    /// Bottom first.
    pub layers: Vec<SavedLayer>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedLayer {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub opacity: f64,
    /// The drawing's file in the drawings store (its SHA-256), if any.
    #[serde(default)]
    pub drawing: Option<String>,
}

/// A change to a page that undo can take back.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// A new drawing for a layer (`None` clears it).
    Draw {
        layer: u64,
        drawing: Option<String>,
    },
    AddLayer {
        name: Option<String>,
    },
    RemoveLayer {
        layer: u64,
    },
    /// Moves a layer to `to`, counted from the bottom.
    MoveLayer {
        layer: u64,
        to: u32,
    },
    RenameLayer {
        layer: u64,
        name: String,
    },
    SetVisible {
        layer: u64,
        visible: bool,
    },
    SetOpacity {
        layer: u64,
        opacity: f64,
    },
    DuplicateLayer {
        layer: u64,
    },
    SetPaper {
        paper: Paper,
    },
}

impl Edit {
    /// The step's name, for "Undo …" and "Redo …".
    fn name(&self) -> &'static str {
        match self {
            Edit::Draw {
                drawing: Some(_), ..
            } => "Drawing",
            Edit::Draw { drawing: None, .. } => "Clear Layer",
            Edit::AddLayer { .. } => "Add Layer",
            Edit::RemoveLayer { .. } => "Delete Layer",
            Edit::MoveLayer { .. } => "Move Layer",
            Edit::RenameLayer { .. } => "Rename Layer",
            Edit::SetVisible { visible: true, .. } => "Show Layer",
            Edit::SetVisible { visible: false, .. } => "Hide Layer",
            Edit::SetOpacity { .. } => "Layer Opacity",
            Edit::DuplicateLayer { .. } => "Duplicate Layer",
            Edit::SetPaper { .. } => "Paper",
        }
    }

    /// Edits that merge with the one before when they follow quickly.
    fn coalesces(&self) -> Option<u64> {
        match self {
            Edit::SetOpacity { layer, .. } => Some(*layer),
            _ => None,
        }
    }
}

impl Book {
    pub fn sketch(&self, id: u64) -> Result<&Sketch, SketchbookError> {
        self.sketches
            .iter()
            .find(|s| s.id == id)
            .ok_or(SketchbookError::NotFound { id })
    }

    pub fn sketch_mut(&mut self, id: u64) -> Result<&mut Sketch, SketchbookError> {
        self.sketches
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(SketchbookError::NotFound { id })
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    /// Adds a sketch with one empty layer; returns its id.
    pub fn create(
        &mut self,
        title: &str,
        paper: Paper,
        now_ms: i64,
    ) -> Result<u64, SketchbookError> {
        let title = clean_name(title)?;
        let id = self.take_id();
        let layer = self.take_id();
        self.sketches.push(Sketch {
            id,
            title,
            created_ms: now_ms,
            modified_ms: now_ms,
            page: Page {
                paper,
                layers: vec![new_layer(layer, "Layer 1".into())],
            },
        });
        Ok(id)
    }

    /// Copies a sketch (the drawings are shared files); returns the copy's id.
    pub fn duplicate(&mut self, id: u64, now_ms: i64) -> Result<u64, SketchbookError> {
        let original = self.sketch(id)?.clone();
        let copy = self.take_id();
        let mut page = original.page;
        for layer in &mut page.layers {
            layer.id = self.take_id();
        }
        let title = copy_title(&original.title);
        self.sketches.push(Sketch {
            id: copy,
            title,
            created_ms: now_ms,
            modified_ms: now_ms,
            page,
        });
        Ok(copy)
    }

    pub fn rename(&mut self, id: u64, title: &str, now_ms: i64) -> Result<(), SketchbookError> {
        let title = clean_name(title)?;
        let sketch = self.sketch_mut(id)?;
        sketch.title = title;
        sketch.modified_ms = now_ms;
        Ok(())
    }

    pub fn delete(&mut self, id: u64) -> Result<(), SketchbookError> {
        let before = self.sketches.len();
        self.sketches.retain(|s| s.id != id);
        if self.sketches.len() == before {
            return Err(SketchbookError::NotFound { id });
        }
        Ok(())
    }

    /// Applies `edit` to a sketch's page; returns the page from before, for
    /// the undo history.
    pub fn apply(&mut self, id: u64, edit: &Edit, now_ms: i64) -> Result<Page, SketchbookError> {
        let before = self.sketch(id)?.page.clone();
        let mut page = before.clone();
        let new_id = self.next_id.max(1);
        let used_id = apply(&mut page, edit, new_id)?;
        if used_id {
            self.next_id = new_id + 1;
        }
        let sketch = self.sketch_mut(id)?;
        sketch.page = page;
        sketch.modified_ms = now_ms;
        Ok(before)
    }

    /// Puts back a page from the undo history; returns the page it replaced.
    pub fn restore(&mut self, id: u64, page: Page, now_ms: i64) -> Result<Page, SketchbookError> {
        let sketch = self.sketch_mut(id)?;
        sketch.modified_ms = now_ms;
        Ok(std::mem::replace(&mut sketch.page, page))
    }

    /// Every drawing file a sketch uses.
    pub fn drawings(&self) -> HashSet<String> {
        self.sketches
            .iter()
            .flat_map(|s| page_drawings(&s.page))
            .collect()
    }
}

fn new_layer(id: u64, name: String) -> SavedLayer {
    SavedLayer {
        id,
        name,
        visible: true,
        opacity: 1.0,
        drawing: None,
    }
}

/// "Harbour" → "Harbour Copy", "Harbour Copy" → "Harbour Copy 2", within
/// the length limit.
fn copy_title(title: &str) -> String {
    let next = match title.rsplit_once(" Copy") {
        Some((base, "")) => format!("{base} Copy 2"),
        Some((base, n)) => match n.trim().parse::<u32>() {
            Ok(n) => format!("{base} Copy {}", n + 1),
            Err(_) => format!("{title} Copy"),
        },
        None => format!("{title} Copy"),
    };
    if clean_name(&next).is_ok() {
        next
    } else {
        title.to_string()
    }
}

fn page_drawings(page: &Page) -> impl Iterator<Item = String> + '_ {
    page.layers.iter().filter_map(|l| l.drawing.clone())
}

fn layer_index(page: &Page, layer: u64) -> Result<usize, SketchbookError> {
    page.layers
        .iter()
        .position(|l| l.id == layer)
        .ok_or(SketchbookError::NotFound { id: layer })
}

/// A name no layer of `page` has yet: "Layer 2", "Layer 3", ...
fn next_layer_name(page: &Page) -> String {
    (1..)
        .map(|n| format!("Layer {n}"))
        .find(|name| page.layers.iter().all(|l| &l.name != name))
        .expect("an unused name")
}

/// Applies `edit` to `page`. A new layer gets `new_id`; returns whether it
/// was used.
fn apply(page: &mut Page, edit: &Edit, new_id: u64) -> Result<bool, SketchbookError> {
    let room = || {
        if page.layers.len() >= MAX_LAYERS as usize {
            Err(SketchbookError::TooManyLayers { max: MAX_LAYERS })
        } else {
            Ok(())
        }
    };
    match edit {
        Edit::Draw { layer, drawing } => {
            let i = layer_index(page, *layer)?;
            page.layers[i].drawing = drawing.clone();
        }
        Edit::AddLayer { name } => {
            room()?;
            let name = match name {
                Some(name) => clean_name(name)?,
                None => next_layer_name(page),
            };
            page.layers.push(new_layer(new_id, name));
            return Ok(true);
        }
        Edit::RemoveLayer { layer } => {
            let i = layer_index(page, *layer)?;
            if page.layers.len() == 1 {
                return Err(SketchbookError::LastLayer);
            }
            page.layers.remove(i);
        }
        Edit::MoveLayer { layer, to } => {
            let i = layer_index(page, *layer)?;
            let to = (*to as usize).min(page.layers.len() - 1);
            let moved = page.layers.remove(i);
            page.layers.insert(to, moved);
        }
        Edit::RenameLayer { layer, name } => {
            let i = layer_index(page, *layer)?;
            page.layers[i].name = clean_name(name)?;
        }
        Edit::SetVisible { layer, visible } => {
            let i = layer_index(page, *layer)?;
            page.layers[i].visible = *visible;
        }
        Edit::SetOpacity { layer, opacity } => {
            if !(0.0..=1.0).contains(opacity) {
                return Err(SketchbookError::BadOpacity);
            }
            let i = layer_index(page, *layer)?;
            page.layers[i].opacity = *opacity;
        }
        Edit::DuplicateLayer { layer } => {
            let i = layer_index(page, *layer)?;
            room()?;
            let mut copy = page.layers[i].clone();
            copy.id = new_id;
            copy.name = copy_title(&copy.name);
            page.layers.insert(i + 1, copy);
            return Ok(true);
        }
        Edit::SetPaper { paper } => page.paper = *paper,
    }
    Ok(false)
}

/// One undo or redo step: the page as it was on the other side of it.
#[derive(Clone, Debug)]
struct Step {
    name: &'static str,
    page: Page,
    /// The layer whose quick opacity changes merge into this step.
    coalesce: Option<u64>,
    at_ms: i64,
}

/// Undo and redo for one sketch. Kept in memory: a relaunch starts with an
/// empty history, as Notes and Freeform do.
#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl History {
    /// Remembers `before`, the page as it was before `edit`. Clears redo.
    pub fn record(&mut self, edit: &Edit, before: Page, now_ms: i64) {
        self.redo.clear();
        let coalesce = edit.coalesces();
        if let Some(top) = self.undo.last_mut() {
            if coalesce.is_some() && top.coalesce == coalesce && now_ms - top.at_ms <= COALESCE_MS {
                // Keep the older page: one undo takes back the whole drag.
                top.at_ms = now_ms;
                return;
            }
        }
        self.undo.push(Step {
            name: edit.name(),
            page: before,
            coalesce,
            at_ms: now_ms,
        });
        if self.undo.len() > MAX_UNDO_STEPS as usize {
            self.undo.remove(0);
        }
    }

    /// The page to go back to, given the current one (kept for redo).
    pub fn undo(&mut self, current: Page) -> Result<Page, SketchbookError> {
        let step = self.undo.pop().ok_or(SketchbookError::NothingToUndo)?;
        self.redo.push(Step {
            page: current,
            coalesce: None,
            ..step.clone()
        });
        Ok(step.page)
    }

    pub fn redo(&mut self, current: Page) -> Result<Page, SketchbookError> {
        let step = self.redo.pop().ok_or(SketchbookError::NothingToRedo)?;
        self.undo.push(Step {
            page: current,
            coalesce: None,
            ..step.clone()
        });
        Ok(step.page)
    }

    pub fn undo_name(&self) -> Option<String> {
        self.undo.last().map(|s| format!("Undo {}", s.name))
    }

    pub fn redo_name(&self) -> Option<String> {
        self.redo.last().map(|s| format!("Redo {}", s.name))
    }

    fn drawings(&self) -> impl Iterator<Item = String> + '_ {
        self.undo
            .iter()
            .chain(&self.redo)
            .flat_map(|s| page_drawings(&s.page))
    }
}

/// Every drawing the saved book or any undo history still needs.
pub fn drawings_in_use(book: &Book, histories: &HashMap<u64, History>) -> HashSet<String> {
    let mut names = book.drawings();
    names.extend(histories.values().flat_map(|h| h.drawings()));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book_with_sketch() -> (Book, u64) {
        let mut book = Book::default();
        let id = book.create("Harbour", Paper::Plain, 0).unwrap();
        (book, id)
    }

    fn layers(book: &Book, id: u64) -> Vec<String> {
        book.sketch(id)
            .unwrap()
            .page
            .layers
            .iter()
            .map(|l| l.name.clone())
            .collect()
    }

    #[test]
    fn layers_are_added_named_moved_and_kept_within_limits() {
        let (mut book, id) = book_with_sketch();
        for _ in 1..MAX_LAYERS {
            book.apply(id, &Edit::AddLayer { name: None }, 1).unwrap();
        }
        assert_eq!(layers(&book, id).len(), MAX_LAYERS as usize);
        assert_eq!(layers(&book, id)[2], "Layer 3");
        assert_eq!(
            book.apply(id, &Edit::AddLayer { name: None }, 1),
            Err(SketchbookError::TooManyLayers { max: MAX_LAYERS })
        );
        let top = book.sketch(id).unwrap().page.layers[MAX_LAYERS as usize - 1].id;
        book.apply(id, &Edit::MoveLayer { layer: top, to: 0 }, 2)
            .unwrap();
        assert_eq!(layers(&book, id)[0], "Layer 8");
        // Past the top means the top.
        book.apply(id, &Edit::MoveLayer { layer: top, to: 99 }, 2)
            .unwrap();
        assert_eq!(layers(&book, id).last().unwrap(), "Layer 8");
    }

    #[test]
    fn the_last_layer_stays() {
        let (mut book, id) = book_with_sketch();
        let only = book.sketch(id).unwrap().page.layers[0].id;
        assert_eq!(
            book.apply(id, &Edit::RemoveLayer { layer: only }, 1),
            Err(SketchbookError::LastLayer)
        );
    }

    #[test]
    fn undo_and_redo_walk_the_pages() {
        let (mut book, id) = book_with_sketch();
        let mut history = History::default();
        let layer = book.sketch(id).unwrap().page.layers[0].id;
        let draw = |name: &str| Edit::Draw {
            layer,
            drawing: Some(name.into()),
        };

        for (i, name) in ["a", "b"].into_iter().enumerate() {
            let edit = draw(name);
            let before = book.apply(id, &edit, i as i64).unwrap();
            history.record(&edit, before, i as i64);
        }
        assert_eq!(history.undo_name().as_deref(), Some("Undo Drawing"));
        assert_eq!(history.redo_name(), None);

        let current = book.sketch(id).unwrap().page.clone();
        let back = history.undo(current).unwrap();
        assert_eq!(back.layers[0].drawing.as_deref(), Some("a"));
        book.restore(id, back, 5).unwrap();
        assert_eq!(history.redo_name().as_deref(), Some("Redo Drawing"));

        let current = book.sketch(id).unwrap().page.clone();
        let forward = history.redo(current).unwrap();
        assert_eq!(forward.layers[0].drawing.as_deref(), Some("b"));

        // A new edit after an undo drops what redo had.
        let current = book.sketch(id).unwrap().page.clone();
        let back = history.undo(current).unwrap();
        book.restore(id, back, 6).unwrap();
        let edit = draw("c");
        let before = book.apply(id, &edit, 7).unwrap();
        history.record(&edit, before, 7);
        assert_eq!(history.redo_name(), None);
        assert!(history.drawings().any(|d| d == "a"));
        assert!(!history.drawings().any(|d| d == "b"));
    }

    #[test]
    fn a_quick_run_of_opacity_changes_is_one_step() {
        let (mut book, id) = book_with_sketch();
        let mut history = History::default();
        let layer = book.sketch(id).unwrap().page.layers[0].id;
        for (i, opacity) in [0.9, 0.7, 0.5].into_iter().enumerate() {
            let edit = Edit::SetOpacity { layer, opacity };
            let before = book.apply(id, &edit, i as i64 * 100).unwrap();
            history.record(&edit, before, i as i64 * 100);
        }
        let current = book.sketch(id).unwrap().page.clone();
        let back = history.undo(current).unwrap();
        assert_eq!(back.layers[0].opacity, 1.0);
        assert_eq!(history.undo_name(), None);

        // A later change is a step of its own.
        let edit = Edit::SetOpacity {
            layer,
            opacity: 0.4,
        };
        let before = book.apply(id, &edit, 10_000).unwrap();
        history.record(&edit, before, 10_000);
        let edit = Edit::SetOpacity {
            layer,
            opacity: 0.3,
        };
        let before = book.apply(id, &edit, 20_000).unwrap();
        history.record(&edit, before, 20_000);
        let current = book.sketch(id).unwrap().page.clone();
        assert_eq!(history.undo(current).unwrap().layers[0].opacity, 0.4);
    }

    #[test]
    fn history_is_capped() {
        let (mut book, id) = book_with_sketch();
        let mut history = History::default();
        let layer = book.sketch(id).unwrap().page.layers[0].id;
        for i in 0..(MAX_UNDO_STEPS + 20) {
            let edit = Edit::SetVisible {
                layer,
                visible: i % 2 == 0,
            };
            let before = book.apply(id, &edit, i as i64).unwrap();
            history.record(&edit, before, i as i64);
        }
        let mut steps = 0;
        let mut page = book.sketch(id).unwrap().page.clone();
        while let Ok(back) = history.undo(page) {
            page = back;
            steps += 1;
        }
        assert_eq!(steps, MAX_UNDO_STEPS);
    }

    #[test]
    fn duplicates_get_new_ids_and_copy_names() {
        let (mut book, id) = book_with_sketch();
        let copy = book.duplicate(id, 1).unwrap();
        let again = book.duplicate(copy, 2).unwrap();
        assert_eq!(book.sketch(copy).unwrap().title, "Harbour Copy");
        assert_eq!(book.sketch(again).unwrap().title, "Harbour Copy 2");
        let ids: HashSet<u64> = book
            .sketches
            .iter()
            .flat_map(|s| s.page.layers.iter().map(|l| l.id))
            .chain(book.sketches.iter().map(|s| s.id))
            .collect();
        assert_eq!(ids.len(), 6, "every sketch and layer id is unique");
    }
}
