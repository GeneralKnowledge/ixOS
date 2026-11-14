# Sketchbook

A drawing notebook: sketches drawn with a finger, in layers, with undo and
redo, exported to PNG and PDF. The second of the remaining demo apps
([friction log](../../docs/research/app-friction-log.md)); it runs inside
Device Lab (Apps, then Sketchbook) and on its own.

## Where it is

| Step | What | State |
|---|---|---|
| S1 | Rust core (`rust/`): sketches, layers, one undo history per sketch, export plans | Done, tested on Linux |
| S2 | SwiftUI app: sketch grid, PencilKit canvas per layer, layer panel, PNG/PDF export with a Quick Look preview, hosted in Device Lab | Written; not yet built with xtool or run on a phone |

## How the work is split

- **PencilKit draws.** Each layer is a `PKCanvasView`. When a stroke ends
  (`canvasViewDrawingDidChange`), the app passes
  `PKDrawing.dataRepresentation()` to `SketchbookCore.draw`. Rust stores it
  as a file named by its SHA-256 and never parses it.
- **Rust owns undo.** Strokes and layer changes (add, delete, duplicate,
  move, rename, hide, opacity, paper) are steps in one history per sketch,
  so Undo takes back whichever came last. Each step keeps the whole page
  from before; a page is a few file names, so that is cheap. Quick opacity
  changes to one layer (a slider) merge into one step. The history lives in
  memory and starts empty after a relaunch. The canvas's own undo manager
  is turned off (`LayerCanvas.RustUndoCanvas`), so the tool picker and
  shake to undo can't take back a stroke behind Rust's back.
- **Exports are planned in Rust and rendered by PencilKit.**
  `export_plan(ids, format)` lists each page's visible, non-empty layers
  bottom first with their opacity, and the file path to write (one PNG per
  sketch, or one PDF).
- Drawings nothing needs any more (not the saved sketches, not an undo
  step) are deleted after each change; a crash between writing a drawing and
  saving is cleaned up at the next launch.

## Rust

```bash
cargo test --manifest-path examples/sketchbook/rust/Cargo.toml
```

## Swift

| File | What |
|---|---|
| `Sources/AppHost/SketchbookModel.swift` | The screens' model: asks the core, keeps alerts. Foundation only, tested on Linux and macOS |
| `Sources/AppHost/ContentView.swift` | The sketch grid: new (with a paper menu), rename, duplicate, delete |
| `Sources/AppHost/SketchView.swift` | One sketch: the layers stacked on the paper, Undo and Redo, paper, export |
| `Sources/AppHost/LayerCanvas.swift` | One layer's `PKCanvasView`; hands each finished stroke to Rust |
| `Sources/AppHost/LayersPanel.swift` | Layers top first: select, hide, reorder, opacity, rename, duplicate, clear, delete |
| `Sources/AppHost/SketchRenderer.swift` | Draws export pages and thumbnails from Rust's export plan |
| `Sources/AppHost/PaperBackground.swift` | The paper's lines, squares or dots, on screen and in exports |

Build and install on its own (it uses the free team's sandbox id):

```bash
forge run ios --project examples/sketchbook
forge ui run examples/sketchbook/uitests/smoke.toml   # phone plugged in
```

In Device Lab it is one more entry on the Apps screen; see
[Device Lab's README](../device-lab/README.md).
