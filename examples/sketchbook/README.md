# Sketchbook

A drawing notebook: sketches drawn with a finger, in layers, with undo and
redo, exported to PNG and PDF. The second of the remaining demo apps
([friction log](../../docs/research/app-friction-log.md)); it runs inside Device Lab
once its SwiftUI app exists.

## Where it is

| Step | What | State |
|---|---|---|
| S1 | Rust core (`rust/`): sketches, layers, one undo history per sketch, export plans | Done, tested on Linux |
| S2 | SwiftUI app: sketch grid, PencilKit canvas per layer, layer panel, PNG/PDF export with a Quick Look preview, hosted in Device Lab | Next |

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
  memory and starts empty after a relaunch. Turn off the canvas's own undo
  manager.
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
