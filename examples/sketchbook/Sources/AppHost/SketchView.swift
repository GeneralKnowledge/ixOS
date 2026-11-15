#if os(iOS)
@preconcurrency import SketchbookFFI
import ForgeAppKit
import PencilKit
import SwiftUI

/// Screen 2: one sketch. Its layers are stacked canvases over the paper;
/// only the selected layer takes strokes. Undo and Redo are Rust's (one
/// history for every layer), and their labels say what they take back.
struct SketchView: View {
    @Bindable var model: SketchbookModel
    let sketchID: UInt64
    let changes: UInt64

    @State private var detail: SketchDetail?
    @State private var selected: UInt64?
    @State private var showingLayers = false
    @State private var exported: SketchExport?
    /// Strokes sent to Rust and not answered yet. While any are out, an
    /// answer for an earlier stroke would put the canvas back a stroke.
    @State private var pendingDraws = 0
    /// One tool picker for every layer, so the tool stays when the layer
    /// changes. It starts on a black pen.
    @State private var picker: PKToolPicker = {
        let picker = PKToolPicker()
        picker.selectedTool = PKInkingTool(.pen, color: .black, width: 4)
        return picker
    }()

    var body: some View {
        Group {
            if let detail {
                canvas(detail)
            } else {
                ContentUnavailableView("Sketch Deleted", systemImage: "trash")
            }
        }
        .navigationTitle(detail?.summary.title ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { toolbar }
        .sheet(isPresented: $showingLayers) {
            LayersPanel(model: model, sketchID: sketchID, detail: $detail, selected: $selected)
                .presentationDetents([.medium, .large])
        }
        .sheet(item: $exported) { export in
            ExportSheet(export: export)
        }
        // While a sheet is up, it shows the alerts.
        .forgeAlert(Binding(
            get: { showingLayers || exported != nil ? nil : model.alert },
            set: { model.alert = $0 }
        )) { action in
            Task { if let changed = await model.handle(action) { show(changed) } }
        }
        .sensoryFeedback(.success, trigger: model.saveCount)
        .onAppear { show(model.detail(sketchID)) }
        .onChange(of: changes) {
            if pendingDraws == 0 { show(model.detail(sketchID)) }
        }
        // A second after the last change, redraw the grid's picture.
        .task(id: detail?.summary.modifiedMs) {
            guard let detail, (try? await Task.sleep(for: .seconds(1))) != nil else { return }
            if let png = SketchRenderer.thumbnail(detail) {
                await model.setThumbnail(sketchID, png: png)
            }
        }
    }

    private func canvas(_ detail: SketchDetail) -> some View {
        ZStack {
            PaperBackground(paper: detail.summary.paper)
            ForEach(detail.layers) { layer in
                LayerCanvas(
                    drawing: model.drawing(at: layer.drawingPath),
                    isActive: layer.id == selected && layer.visible,
                    picker: picker
                ) { data in
                    draw(data, on: layer.id)
                }
                .opacity(layer.visible ? layer.opacity : 0)
                .allowsHitTesting(layer.id == selected)
            }
        }
        .ignoresSafeArea(edges: .bottom)
        .overlay(alignment: .top) { hiddenLayerNote(detail) }
    }

    @ViewBuilder private func hiddenLayerNote(_ detail: SketchDetail) -> some View {
        if let layer = detail.layers.first(where: { $0.id == selected }), !layer.visible {
            Label("\(layer.name) is hidden. Show it to draw.", systemImage: "eye.slash")
                .font(.footnote)
                .padding(8)
                .background(.regularMaterial, in: Capsule())
                .padding(.top, 8)
        }
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .topBarTrailing) {
            Button {
                Task { if let changed = await model.undo(sketchID) { show(changed) } }
            } label: {
                Label(detail?.undoName ?? "Undo", systemImage: "arrow.uturn.backward")
            }
            .disabled(detail?.undoName == nil)
            .accessibilityIdentifier("sketch.undo")

            Button {
                Task { if let changed = await model.redo(sketchID) { show(changed) } }
            } label: {
                Label(detail?.redoName ?? "Redo", systemImage: "arrow.uturn.forward")
            }
            .disabled(detail?.redoName == nil)
            .accessibilityIdentifier("sketch.redo")

            Button {
                showingLayers = true
            } label: {
                Label("Layers", systemImage: "square.3.layers.3d")
            }
            .accessibilityIdentifier("sketch.layers")

            Menu {
                Picker("Paper", selection: paper) {
                    ForEach(SketchFormat.papers, id: \.self) { paper in
                        Text(SketchFormat.name(paper)).tag(paper)
                    }
                }
                .accessibilityIdentifier("sketch.paper")
                Section("Save or Share") {
                    Button("PNG Image", systemImage: "photo") { export(.png) }
                        .accessibilityIdentifier("sketch.export.png")
                    Button("PDF Document", systemImage: "doc.richtext") { export(.pdf) }
                        .accessibilityIdentifier("sketch.export.pdf")
                }
            } label: {
                Label("More", systemImage: "ellipsis.circle")
            }
            .accessibilityIdentifier("sketch.more")
        }
    }

    private var paper: Binding<Paper> {
        Binding(
            get: { detail?.summary.paper ?? .plain },
            set: { paper in
                Task { if let changed = await model.setPaper(sketchID, paper) { show(changed) } }
            }
        )
    }

    /// Shows what Rust holds, keeping the selected layer if it still exists
    /// (otherwise the top one).
    private func show(_ changed: SketchDetail?) {
        detail = changed
        guard let layers = changed?.layers else { return }
        if !layers.contains(where: { $0.id == selected }) {
            selected = layers.last?.id
        }
    }

    private func draw(_ data: Data, on layer: UInt64) {
        pendingDraws += 1
        Task {
            let changed = await model.draw(sketchID, layer: layer, drawing: data)
            pendingDraws -= 1
            if pendingDraws == 0 {
                show(changed ?? model.detail(sketchID))
            }
        }
    }

    private func export(_ format: ExportFormat) {
        guard let plan = model.exportPlan([sketchID], format: format) else { return }
        do {
            let url = try SketchRenderer.write(plan)
            exported = SketchExport(
                url: url,
                format: format,
                image: plan.pages.first.map { SketchRenderer.image($0, scale: 1) }
            )
        } catch {
            model.report(error)
        }
    }
}
#endif
