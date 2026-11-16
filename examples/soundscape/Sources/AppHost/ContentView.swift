@preconcurrency import SoundscapeFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import ForgeUI
import SwiftUI

/// The app's root. ForgeKit's `ForgeRoot` shows a spinner while the Rust
/// core opens, the projects once it has, or a recovery screen if it failed.
public struct ContentView: View {
    @Bindable private var host: ForgeHost<SoundscapeCore>

    public init(host: ForgeHost<SoundscapeCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            SoundscapeScreen(core: core, changes: host.changes)
        }
        .forgeTheme(.standard)
    }
}

/// Owns the screens' model for as long as the core is open: the projects,
/// and the Listen tab.
private struct SoundscapeScreen: View {
    @State private var model: SoundscapeModel
    let changes: UInt64

    init(core: SoundscapeCore, changes: UInt64) {
        _model = State(initialValue: SoundscapeModel(core: core))
        self.changes = changes
    }

    var body: some View {
        TabView {
            ProjectListView(model: model, changes: changes)
                .onChange(of: changes) { model.reload() }
                .tabItem { Label("Projects", systemImage: "waveform") }
                .accessibilityIdentifier("tab.projects")
            listen
                .tabItem { Label("Library", systemImage: "music.note.list") }
                .accessibilityIdentifier("tab.listen")
        }
    }

    /// Your music library, from the take server on another computer, with
    /// the player (ListenView.swift, LibraryView.swift).
    @ViewBuilder private var listen: some View {
        #if os(iOS)
        ListenView(core: model.core)
        #else
        Text("Listening needs an iPhone.")
        #endif
    }
}

/// Screen 1: every project, most recent first. New asks for a name, tempo
/// and length, then opens the mixer.
struct ProjectListView: View {
    @Bindable var model: SoundscapeModel
    let changes: UInt64
    @State private var path: [UInt64] = []
    @State private var creating = false
    @State private var renaming: ProjectSummary?
    @State private var newTitle = ""

    var body: some View {
        NavigationStack(path: $path) {
            List {
                ForEach(model.projects) { project in
                    NavigationLink(value: project.id) {
                        ProjectRow(project: project)
                    }
                    .accessibilityIdentifier("projects.row")
                    .swipeActions {
                        Button("Delete", systemImage: "trash", role: .destructive) {
                            model.askToDelete(project)
                        }
                        .accessibilityIdentifier("projects.delete")
                    }
                    .contextMenu {
                        Button("Rename", systemImage: "pencil") {
                            newTitle = project.title
                            renaming = project
                        }
                        .accessibilityIdentifier("projects.rename")
                        Button("Delete", systemImage: "trash", role: .destructive) {
                            model.askToDelete(project)
                        }
                        .accessibilityIdentifier("projects.deleteMenu")
                    }
                }
            }
            .overlay { emptyState }
            .navigationTitle("Soundscape")
            .navigationDestination(for: UInt64.self) { id in
                #if os(iOS)
                ProjectView(model: model, projectID: id, changes: changes)
                #else
                // AVAudioSession is iOS only; the macOS build exists for
                // the model tests.
                Text("Recording needs an iPhone.")
                #endif
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button("New Project", systemImage: "plus") { creating = true }
                        .accessibilityIdentifier("projects.new")
                }
            }
            .sheet(isPresented: $creating) {
                NewProjectSheet(model: model) { project in
                    creating = false
                    path = [project.summary.id]
                }
            }
            .alert("Rename Project", isPresented: Binding(
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
            .forgeAlert(Binding(
                // The project screen it pushes shows its own alerts, and this
                // one would show behind it.
                get: { creating || !path.isEmpty ? nil : model.alert },
                set: { model.alert = $0 }
            )) { action in Task { await model.handle(action) } }
            .sensoryFeedback(.success, trigger: model.saveCount)
        }
    }

    @ViewBuilder private var emptyState: some View {
        if model.projects.isEmpty {
            ForgeStateView.empty(
                "No Projects", systemImage: "waveform",
                description: "Record a loop, layer more on top, shape each one and mix it down to share.",
                action: ForgeStateAction("New Project", identifier: "projects.newFirst") { creating = true }
            )
        }
    }

    private func rename() {
        guard let project = renaming else { return }
        let title = newTitle
        Task {
            do {
                try await model.rename(project.id, to: title)
            } catch {
                model.report(error)
            }
        }
    }
}

private struct ProjectRow: View {
    let project: ProjectSummary

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(project.title)
                .font(.headline)
            Text("\(SoundFormat.timing(tempo: project.tempoBpm, bars: project.bars)) · \(project.trackCount) \(project.trackCount == 1 ? "track" : "tracks")")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
    }
}

/// Name, tempo and length for a new project (or new timing for one).
struct NewProjectSheet: View {
    let model: SoundscapeModel
    let created: (ProjectDetail) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var title = SoundFormat.defaultTitle()
    @State private var tempo = 100.0
    @State private var bars = 2
    @State private var problem: String?
    @State private var saving = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $title)
                        .accessibilityIdentifier("newProject.title")
                }
                TimingFields(tempo: $tempo, bars: $bars)
                if let problem {
                    Section {
                        Text(problem).foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle("New Project")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("newProject.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Create") { create() }
                        .disabled(saving)
                        .accessibilityIdentifier("newProject.create")
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    private func create() {
        saving = true
        Task {
            defer { saving = false }
            do {
                created(try await model.create(title: title, tempo: tempo, bars: bars))
            } catch let error as SoundscapeError {
                problem = SoundscapeModel.describe(error)
            } catch {
                problem = error.localizedDescription
            }
        }
    }
}

/// Tempo and length, for a new project and for changing one.
struct TimingFields: View {
    @Binding var tempo: Double
    @Binding var bars: Int

    var body: some View {
        Section("Timing") {
            Stepper(value: $tempo, in: 40...240, step: 1) {
                LabeledContent("Tempo", value: "\(Int(tempo)) BPM")
            }
            .accessibilityIdentifier("timing.tempo")
            Picker("Length", selection: $bars) {
                ForEach([1, 2, 4, 8, 16], id: \.self) { count in
                    Text("\(count) \(count == 1 ? "bar" : "bars")").tag(count)
                }
            }
            .accessibilityIdentifier("timing.bars")
        }
    }
}
