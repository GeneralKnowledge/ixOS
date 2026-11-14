#if os(iOS)
import PencilKit
import SwiftUI
import UIKit

/// One layer on screen: a transparent `PKCanvasView` showing the layer's
/// drawing. Only the selected layer takes touches and shows the tool
/// picker; the others just show their ink. When a stroke ends the new
/// drawing goes to `onChange`, which hands it to Rust (one undo step).
struct LayerCanvas: UIViewRepresentable {
    /// The layer's drawing as Rust holds it (`PKDrawing.dataRepresentation()`).
    let drawing: Data?
    let isActive: Bool
    let onChange: (Data) -> Void

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> PKCanvasView {
        let canvas = RustUndoCanvas()
        canvas.backgroundColor = .clear
        canvas.isOpaque = false
        // Finger drawing: the iPhone has no Pencil.
        canvas.drawingPolicy = .anyInput
        canvas.isScrollEnabled = false
        canvas.contentInsetAdjustmentBehavior = .never
        canvas.tool = PKInkingTool(.pen, color: .black, width: 4)
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
        coordinator.showTools(isActive, on: canvas)
    }

    static func dismantleUIView(_ canvas: PKCanvasView, coordinator: Coordinator) {
        coordinator.showTools(false, on: canvas)
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
        private let picker = PKToolPicker()
        private var toolsShown = false

        func apply(_ data: Data?, to canvas: PKCanvasView) {
            guard data != shown else { return }
            applying = true
            canvas.drawing = data.flatMap { try? PKDrawing(data: $0) } ?? PKDrawing()
            shown = data
            applying = false
        }

        func showTools(_ show: Bool, on canvas: PKCanvasView) {
            guard show != toolsShown else { return }
            toolsShown = show
            if show {
                picker.addObserver(canvas)
                picker.setVisible(true, forFirstResponder: canvas)
                // The canvas must be in a window to become first responder.
                DispatchQueue.main.async { canvas.becomeFirstResponder() }
            } else {
                picker.setVisible(false, forFirstResponder: canvas)
                picker.removeObserver(canvas)
                canvas.resignFirstResponder()
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
