#if os(iOS)
import PencilKit
import SwiftUI
import UIKit

/// One layer on screen: a transparent `PKCanvasView` showing the layer's
/// drawing. Only the selected layer takes touches; the others just show
/// their ink. Every layer follows the sketch's one tool picker, so the pen,
/// colour and width stay the same when you change layer. When a stroke ends
/// the new drawing goes to `onChange`, which hands it to Rust (one undo
/// step).
struct LayerCanvas: UIViewRepresentable {
    /// The layer's drawing as Rust holds it (`PKDrawing.dataRepresentation()`).
    let drawing: Data?
    let isActive: Bool
    /// The sketch's tool picker, shared by its layers.
    let picker: PKToolPicker
    let onChange: (Data) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(picker: picker) }

    func makeUIView(context: Context) -> PKCanvasView {
        let canvas = RustUndoCanvas()
        canvas.backgroundColor = .clear
        canvas.isOpaque = false
        // The paper is always warm white. In dark mode PencilKit would show
        // black ink as white, which vanishes on it; exports already render
        // in light colours, so the screen now matches them.
        canvas.overrideUserInterfaceStyle = .light
        // Finger drawing: the iPhone has no Pencil.
        canvas.drawingPolicy = .anyInput
        canvas.isScrollEnabled = false
        canvas.contentInsetAdjustmentBehavior = .never
        // The tool chosen on another layer, then whatever is chosen next.
        canvas.tool = picker.selectedTool
        picker.addObserver(canvas)
        // The picker shows while any of the sketch's layers is first
        // responder, which only the selected one becomes.
        picker.setVisible(true, forFirstResponder: canvas)
        canvas.delegate = context.coordinator
        canvas.accessibilityIdentifier = "sketch.canvas"
        canvas.accessibilityLabel = "Drawing canvas"
        context.coordinator.apply(drawing, to: canvas)
        return canvas
    }

    func updateUIView(_ canvas: PKCanvasView, context: Context) {
        let coordinator = context.coordinator
        coordinator.onChange = onChange
        coordinator.apply(drawing, to: canvas)
        canvas.isUserInteractionEnabled = isActive
        coordinator.setActive(isActive, canvas)
    }

    static func dismantleUIView(_ canvas: PKCanvasView, coordinator: Coordinator) {
        coordinator.picker.setVisible(false, forFirstResponder: canvas)
        coordinator.picker.removeObserver(canvas)
        canvas.resignFirstResponder()
    }

    /// A canvas with no undo manager of its own: undo is Rust's, one
    /// history per sketch across every layer, so the tool picker's and the
    /// shake gesture's undo must not take back strokes behind its back.
    final class RustUndoCanvas: PKCanvasView {
        override var undoManager: UndoManager? { nil }
    }

    final class Coordinator: NSObject, PKCanvasViewDelegate {
        var onChange: (Data) -> Void = { _ in }
        /// The bytes the canvas shows, so the same drawing coming back from
        /// Rust isn't set again (which would reset the canvas mid-stroke).
        private var shown: Data?
        private var applying = false
        let picker: PKToolPicker
        private var active = false

        init(picker: PKToolPicker) {
            self.picker = picker
        }

        func apply(_ data: Data?, to canvas: PKCanvasView) {
            guard data != shown else { return }
            applying = true
            canvas.drawing = data.flatMap { try? PKDrawing(data: $0) } ?? PKDrawing()
            shown = data
            applying = false
        }

        /// The selected layer becomes first responder, which shows the
        /// picker; the layer it replaces doesn't resign, so the picker
        /// stays up while the new one takes over.
        func setActive(_ isActive: Bool, _ canvas: PKCanvasView) {
            guard isActive != active else { return }
            active = isActive
            if isActive {
                // The canvas must be in a window to become first responder.
                DispatchQueue.main.async { canvas.becomeFirstResponder() }
            }
        }

        func canvasViewDrawingDidChange(_ canvas: PKCanvasView) {
            guard !applying else { return }
            let data = canvas.drawing.dataRepresentation()
            shown = data
            onChange(data)
        }
    }
}
#endif
