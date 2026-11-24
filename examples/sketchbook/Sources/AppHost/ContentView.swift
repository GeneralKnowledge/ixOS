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

/// A sketch open in the stack. It grows out of its tile and shrinks back
/// into it (ForgeAppKit's Layered Navigation).
struct SketchRoute: ForgeSurfaceRoute {
    let id: UInt64
    var surfaceID: String? { "sketch:\(id)" }
    var surfaceStyle: ForgeSurfaceStyle { .stacked }
}

/// Screen 1: every sketch as a thumbnail, most recent first. New makes a
/// sketch and opens it; a long press renames, duplicates or deletes.
struct SketchListView: View {
    @Bindable var model: SketchbookModel
    let changes: UInt64
    @State private var path: [SketchRoute] = []
    @State private var renaming: SketchSummary?
    /// Where an opened sketch grows from: its tile.
    @Namespace private var surfaces

    private let columns = [GridItem(.adaptive(minimum: 150), spacing: 16)]

    var body: some View {
        NavigationStack(path: $path.forgeLayered()) {
            ScrollView {
                LazyVGrid(columns: columns, spacing: 16) {
                    ForEach(model.sketches) { sketch in
                        NavigationLink(value: SketchRoute(id: sketch.id)) {
                            SketchTile(sketch: sketch, version: model.thumbnailsSaved)
                        }
                        .buttonStyle(.plain)
                        .forgeSurfaceSource(SketchRoute(id: sketch.id))
                        .accessibilityIdentifier("sketches.tile")
                        .contextMenu { menu(for: sketch) }
                    }
                }
                .padding()
            }
            .overlay { emptyState }
            .navigationTitle("Sketchbook")
            .forgeLayeredDestination(for: SketchRoute.self) { route in
                #if os(iOS)
                SketchView(model: model, sketchID: route.id, changes: changes)
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
            .forgeRename("Rename Sketch", item: $renaming, name: \.title, id: "rename") { sketch, title in
                try await model.rename(sketch.id, to: title)
            }
            // Only while the list is on screen: the sketch screen it pushes
            // shows its own alerts, and this one would show behind it. Held
            // back while the rename sheet is up, which shows its own refusal.
            .forgeAlert(Binding(
                get: { path.isEmpty && renaming == nil ? model.alert : nil },
                set: { model.alert = $0 }
            )) { action in Task { await model.handle(action) } }
            .forgeFeedback(model.feedback)
        }
        .forgeSurfaces(surfaces, path: $path)
    }

    @ViewBuilder private func menu(for sketch: SketchSummary) -> some View {
        Button("Rename", systemImage: "pencil") {
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
                path = [SketchRoute(id: sketch.summary.id)]
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
            ForgeThumbnail(aspectRatio: 3 / 4) {
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
            }
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
