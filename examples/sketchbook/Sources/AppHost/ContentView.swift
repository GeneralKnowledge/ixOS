@preconcurrency import SketchbookFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import ForgeUI
import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

/// The app's root. ForgeKit's `ForgeRoot` shows a spinner while the Rust
/// core opens, the sketches once it has, or a recovery screen if it failed.
public struct ContentView: View {
    @Bindable private var host: ForgeHost<SketchbookCore>

    public init(host: ForgeHost<SketchbookCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            SketchbookScreen(core: core, changes: host.changes)
        }
        .forgeTheme(ForgeTheme(accent: .pink))
    }
}

/// Owns the screens' model for as long as the core is open.
private struct SketchbookScreen: View {
    @State private var model: SketchbookModel
    let changes: UInt64

    init(core: SketchbookCore, changes: UInt64) {
        _model = State(initialValue: SketchbookModel(core: core))
        self.changes = changes
    }

    var body: some View {
        SketchListView(model: model, changes: changes)
            // Rust committed a change, or the app came back from the
            // background: show what Rust holds now.
            .onChange(of: changes) { model.reload() }
    }
}

/// Screen 1: every sketch as a thumbnail, most recent first. New makes a
/// sketch and opens it; a long press renames, duplicates or deletes.
struct SketchListView: View {
    @Bindable var model: SketchbookModel
    let changes: UInt64
    @State private var path: [UInt64] = []
    @State private var renaming: SketchSummary?
    @State private var newTitle = ""

    private let columns = [GridItem(.adaptive(minimum: 150), spacing: 16)]

    var body: some View {
        NavigationStack(path: $path) {
            ScrollView {
                LazyVGrid(columns: columns, spacing: 16) {
                    ForEach(model.sketches) { sketch in
                        NavigationLink(value: sketch.id) {
                            SketchTile(sketch: sketch, version: model.thumbnailsSaved)
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("sketches.tile")
                        .contextMenu { menu(for: sketch) }
                    }
                }
                .padding()
            }
            .overlay { emptyState }
            .navigationTitle("Sketchbook")
            .navigationDestination(for: UInt64.self) { id in
                #if os(iOS)
                SketchView(model: model, sketchID: id, changes: changes)
                #else
                // PencilKit's canvas is iOS only; the macOS build exists
                // for the model tests.
                Text("Drawing needs an iPhone or iPad.")
                #endif
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Menu {
                        ForEach(SketchFormat.papers, id: \.self) { paper in
                            Button(SketchFormat.name(paper)) { create(paper) }
                                .accessibilityIdentifier("sketches.new.\(SketchFormat.name(paper).lowercased())")
                        }
                    } label: {
                        Label("New Sketch", systemImage: "plus")
                    } primaryAction: {
                        create(.plain)
                    }
                    .accessibilityIdentifier("sketches.new")
                }
            }
            .alert("Rename Sketch", isPresented: Binding(
                get: { renaming != nil },
                set: { if !$0 { renaming = nil } }
            )) {
                TextField("Name", text: $newTitle)
                    .accessibilityIdentifier("rename.title")
                Button("Rename") { rename() }
                    .accessibilityIdentifier("rename.save")
                Button("Cancel", role: .cancel) {}
                    .accessibilityIdentifier("rename.cancel")
            }
            // Only while the list is on screen: the sketch screen it pushes
            // shows its own alerts, and this one would show behind it.
            .forgeAlert(Binding(
                get: { path.isEmpty ? model.alert : nil },
                set: { model.alert = $0 }
            )) { action in Task { await model.handle(action) } }
            .forgeFeedback(model.feedback)
        }
    }

    @ViewBuilder private func menu(for sketch: SketchSummary) -> some View {
        Button("Rename", systemImage: "pencil") {
            newTitle = sketch.title
            renaming = sketch
        }
        .accessibilityIdentifier("sketches.rename")
        Button("Duplicate", systemImage: "plus.square.on.square") {
            Task { await model.duplicate(sketch) }
        }
        .accessibilityIdentifier("sketches.duplicate")
        Button("Delete", systemImage: "trash", role: .destructive) {
            model.askToDelete(sketch)
        }
        .accessibilityIdentifier("sketches.delete")
    }

    @ViewBuilder private var emptyState: some View {
        if model.sketches.isEmpty {
            ForgeStateView.empty(
                "No Sketches", systemImage: "scribble.variable",
                description: "Draw with your finger, in layers. Your sketches stay on this phone.",
                action: ForgeStateAction("New Sketch", identifier: "sketches.newFirst") { create(.plain) }
            )
        }
    }

    private func create(_ paper: Paper) {
        Task {
            if let sketch = await model.create(paper: paper) {
                path = [sketch.summary.id]
            }
        }
    }

    private func rename() {
        guard let sketch = renaming else { return }
        let title = newTitle
        Task {
            do {
                try await model.rename(sketch.id, to: title)
            } catch {
                model.report(error)
            }
        }
    }
}

/// A sketch in the grid: its thumbnail on its paper, its name and date.
private struct SketchTile: View {
    let sketch: SketchSummary
    /// Changes when any thumbnail was saved.
    let version: Int

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ZStack {
                PaperBackground(paper: sketch.paper)
                #if canImport(UIKit)
                if let path = sketch.thumbnailPath, let image = UIImage(contentsOfFile: path) {
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFit()
                        .padding(8)
                }
                #endif
            }
            .aspectRatio(3 / 4, contentMode: .fit)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(.quaternary))
            // The file keeps its name when it changes; this redraws it.
            .id("\(sketch.modifiedMs)-\(version)")
            Text(sketch.title)
                .font(.subheadline.weight(.semibold))
                .lineLimit(1)
            Text(SketchFormat.date(sketch.modifiedMs), format: .dateTime.day().month().hour().minute())
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(sketch.title), \(sketch.layerCount) layers")
    }
}
