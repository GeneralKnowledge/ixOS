#if os(iOS)
@preconcurrency import SketchbookFFI
import ForgeAppKit
import SwiftUI

/// The layers of one sketch, top first as in every drawing app (Rust keeps
/// them bottom first). Tap to draw on a layer; the eye hides it; drag to
/// reorder; swipe or long press for the rest. Every change is an undo step.
struct LayersPanel: View {
    @Bindable var model: SketchbookModel
    let sketchID: UInt64
    @Binding var detail: SketchDetail?
    @Binding var selected: UInt64?

    @Environment(\.dismiss) private var dismiss
    @State private var renaming: Layer?
    /// The slider's value while it moves; sent to Rust when it is let go.
    @State private var opacity: Double?

    private var layers: [Layer] { (detail?.layers ?? []).reversed() }
    private var current: Layer? { detail?.layers.first { $0.id == selected } }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(layers) { layer in
                        row(layer)
                    }
                    .onMove(perform: move)
                }
                if let current {
                    Section("Opacity of \(current.name)") {
                        HStack {
                            Slider(
                                value: Binding(
                                    get: { opacity ?? current.opacity },
                                    set: { opacity = $0 }
                                ),
                                in: 0...1
                            ) { editing in
                                if !editing { commitOpacity(current) }
                            }
                            .accessibilityIdentifier("layers.opacity")
                            .accessibilityLabel("Opacity")
                            Text(SketchFormat.percent(opacity ?? current.opacity))
                                .monospacedDigit()
                                .frame(minWidth: 44, alignment: .trailing)
                        }
                    }
                }
            }
            .navigationTitle("Layers")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    EditButton()
                        .accessibilityIdentifier("layers.edit")
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Add Layer", systemImage: "plus") {
                        apply { await model.addLayer(to: sketchID) } select: { $0.layers.last?.id }
                    }
                    .accessibilityIdentifier("layers.add")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("layers.done")
                }
            }
            // Held back while the rename sheet is up: it shows its own
            // refusal, and an alert here can't present over it.
            .forgeAlert(Binding(
                get: { renaming == nil ? model.alert : nil },
                set: { model.alert = $0 }
            )) { action in
                apply { await model.handle(action) }
            }
            .forgeRename("Rename Layer", item: $renaming, name: \.name, id: "layers.renameSheet") { layer, name in
                let changed = try await model.renameLayer(sketchID, layer: layer.id, to: name)
                detail = changed
            }
        }
    }

    private func row(_ layer: Layer) -> some View {
        HStack(spacing: 12) {
            Button {
                apply { await model.setVisible(sketchID, layer: layer.id, !layer.visible) }
            } label: {
                Image(systemName: layer.visible ? "eye" : "eye.slash")
                    .foregroundStyle(layer.visible ? Color.accentColor : .secondary)
                    .frame(width: 28)
            }
            .buttonStyle(.borderless)
            .accessibilityIdentifier("layers.visible")
            .accessibilityLabel(layer.visible ? "Hide \(layer.name)" : "Show \(layer.name)")

            Button {
                selected = layer.id
                opacity = nil
            } label: {
                HStack {
                    VStack(alignment: .leading) {
                        Text(layer.name)
                        Text(layer.drawingPath == nil ? "Empty" : SketchFormat.percent(layer.opacity))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    if layer.id == selected {
                        Image(systemName: "pencil.tip")
                            .foregroundStyle(Color.accentColor)
                            .accessibilityLabel("Drawing on this layer")
                    }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("layers.row")
        }
        .swipeActions {
            Button("Delete", systemImage: "trash", role: .destructive) {
                model.askToDeleteLayer(layer, in: sketchID)
            }
            .accessibilityIdentifier("layers.delete")
        }
        .contextMenu {
            Button("Rename", systemImage: "pencil") { renaming = layer }
            .accessibilityIdentifier("layers.rename")
            Button("Duplicate", systemImage: "plus.square.on.square") {
                apply { await model.duplicateLayer(sketchID, layer: layer.id) }
            }
            .accessibilityIdentifier("layers.duplicate")
            Button("Clear", systemImage: "eraser") {
                apply { await model.clear(sketchID, layer: layer.id) }
            }
            .accessibilityIdentifier("layers.clear")
            Button("Delete", systemImage: "trash", role: .destructive) {
                model.askToDeleteLayer(layer, in: sketchID)
            }
            .accessibilityIdentifier("layers.deleteMenu")
        }
    }

    /// `onMove` counts from the top; Rust counts from the bottom.
    private func move(from source: IndexSet, to destination: Int) {
        guard let from = source.first else { return }
        let top = layers
        let landed = destination > from ? destination - 1 : destination
        let layer = top[from]
        apply { await model.moveLayer(sketchID, layer: layer.id, to: top.count - 1 - landed) }
    }

    private func commitOpacity(_ layer: Layer) {
        guard let value = opacity else { return }
        apply { await model.setOpacity(sketchID, layer: layer.id, value) }
    }

    /// Runs a change and shows the sketch Rust returns; `select` picks the
    /// layer to draw on afterwards (by default, the same one).
    private func apply(
        _ change: @escaping () async -> SketchDetail?,
        select: ((SketchDetail) -> UInt64?)? = nil
    ) {
        Task {
            guard let changed = await change() else { return }
            detail = changed
            opacity = nil
            if let pick = select?(changed) {
                selected = pick
            } else if !changed.layers.contains(where: { $0.id == selected }) {
                selected = changed.layers.last?.id
            }
        }
    }
}
#endif
