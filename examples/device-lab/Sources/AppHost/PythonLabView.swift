@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI

/// Python Lab: a project of Python files, the open one in the editor (saved
/// as you type), and Run running it on the phone with the embedded CPython
/// (python-lab-workspace-plan.md, PL1b and PL1c).
struct PythonLabView: View {
    @Bindable var model: PythonLabModel
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        List {
            if model.hasWorkspace {
                projectSection
            }
            Section {
                PythonCodeEditor(
                    text: $model.code,
                    isEditable: !(model.hasWorkspace && model.openFile == nil),
                    accessibilityLabel: model.openFile.map { "Python file \($0)" } ?? "Python script")
                    .frame(minHeight: 320)
                    .listRowInsets(EdgeInsets())
                Menu {
                    ForEach(model.samples) { sample in
                        Button(sample.title) { model.load(sample) }
                            .accessibilityIdentifier("pythonLab.sample.\(sample.id)")
                    }
                } label: {
                    Label(model.hasWorkspace ? "New File from Example" : "Examples", systemImage: "doc.text")
                }
                .accessibilityIdentifier("pythonLab.examples")
                if model.isRunning {
                    Button("Stop", systemImage: "stop.fill", role: .destructive) { model.stop() }
                        .accessibilityIdentifier("pythonLab.stop")
                } else {
                    Button(runTitle, systemImage: "play.fill") {
                        Task { await model.run() }
                    }
                    .disabled(!model.hasPython || (model.hasWorkspace && model.fileToRun == nil))
                    .accessibilityIdentifier("pythonLab.run")
                }
            } header: {
                Text(model.openFile ?? "Script")
            } footer: {
                VStack(alignment: .leading, spacing: 4) {
                    saveLine
                    Text("Runs on this iPhone with CPython. If the last line is an expression, its value is shown. A script is stopped after \(model.timeLimit.components.seconds) s.")
                }
            }
            if let error = model.error {
                Section {
                    Text(error).font(.footnote).forgeForeground(.danger)
                        .accessibilityIdentifier("pythonLab.error")
                }
            }
            Section {
                if model.isRunning {
                    ProgressView("Running…")
                        .accessibilityIdentifier("pythonLab.running")
                } else if model.succeeded != nil {
                    Text(model.output)
                        .font(.system(.footnote, design: .monospaced))
                        .textSelection(.enabled)
                        .accessibilityIdentifier("pythonLab.output")
                } else {
                    Text("Tap Run to see the output here.")
                        .foregroundStyle(.secondary)
                }
            } header: {
                Text(model.lastRunFile.map { "Output of \($0)" } ?? "Output")
            } footer: {
                if let seconds = model.lastSeconds, !model.isRunning {
                    if model.succeeded == false {
                        Text("Ended with an error after \(Format.number(seconds, digits: 3)) s")
                            .forgeForeground(.danger)
                    } else {
                        Text("Finished in \(Format.number(seconds, digits: 3)) s")
                    }
                }
            }
            if model.hasWorkspace {
                artifactsSection
            }
            Section {
                if let version = model.version {
                    LabeledContent("Python", value: version)
                        .accessibilityIdentifier("pythonLab.version")
                }
                if let unavailable = model.unavailable {
                    Text(unavailable).font(.footnote).forgeForeground(.danger)
                        .accessibilityIdentifier("pythonLab.unavailable")
                }
            } header: {
                Text("Interpreter")
            }
        }
        .groupedList()
        .navigationTitle("Python Lab")
        .inlineNavigationTitle()
        .task {
            model.reloadProjects()
            model.loadArtifacts()
            await model.prepare()
        }
        .onDisappear { model.flush() }
        .onChange(of: scenePhase) { _, phase in
            if phase != .active { model.flush() }
        }
    }

    private var runTitle: String {
        guard model.hasWorkspace, let file = model.fileToRun else { return "Run" }
        return "Run \(file)"
    }

    /// Whether what's on screen is saved.
    @ViewBuilder private var saveLine: some View {
        if model.hasWorkspace, model.openFile != nil {
            switch model.saveState {
            case .saved:
                Text("Saved").accessibilityIdentifier("pythonLab.saved")
            case .unsaved:
                Text("Saving…").accessibilityIdentifier("pythonLab.saving")
            case .failed(let reason):
                Text("Not saved: \(reason)").forgeForeground(.danger)
                    .accessibilityIdentifier("pythonLab.notSaved")
            }
        }
    }

    /// The open project and its files.
    private var projectSection: some View {
        Section {
            NavigationLink {
                PythonProjectsView(model: model)
            } label: {
                LabeledContent("Project", value: model.project)
            }
            .accessibilityIdentifier("pythonLab.projects")
            NavigationLink {
                PythonFilesView(model: model)
            } label: {
                LabeledContent("Files", value: "\(model.entries.filter { !$0.isFolder }.count)")
            }
            .accessibilityIdentifier("pythonLab.files")
            if let runFile = model.runFile {
                LabeledContent("Run starts", value: runFile)
                    .accessibilityIdentifier("pythonLab.runFile")
            }
        } header: {
            Text("Project")
        } footer: {
            Text("Projects are folders in the Files app: On My iPhone, Device Lab, Python Lab. Bonsai Harness works in the same folders.")
        }
    }

    /// What the last run made or changed, then a link to all the project's
    /// images.
    @ViewBuilder private var artifactsSection: some View {
        Section {
            if model.madeByLastRun.isEmpty {
                Text(model.succeeded == nil ? "Images a script saves, like plot.png, show here." : "The last run made no images.")
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("pythonLab.artifacts.none")
            }
            ForEach(model.madeByLastRun, id: \.path) { artifact in
                ArtifactRow(artifact: artifact, url: model.url(of: artifact))
            }
            if !model.artifacts.isEmpty {
                NavigationLink {
                    List {
                        ForEach(model.artifacts, id: \.path) { artifact in
                            ArtifactRow(artifact: artifact, url: model.url(of: artifact))
                        }
                    }
                    .navigationTitle("Artifacts")
                    .inlineNavigationTitle()
                } label: {
                    LabeledContent("All Images", value: "\(model.artifacts.count)")
                }
                .accessibilityIdentifier("pythonLab.artifacts.all")
            }
        } header: {
            Text("Artifacts")
        }
    }
}

/// Something to name in `forgeRename`: a project, or a file or folder.
private struct NameTarget: Identifiable {
    let id: String
    let name: String
}

/// The projects: open one, create, rename, delete.
struct PythonProjectsView: View {
    @Bindable var model: PythonLabModel
    @Environment(\.dismiss) private var dismiss
    @State private var creating = false
    @State private var renaming: NameTarget?
    @State private var deleting: WorkspaceProject?

    var body: some View {
        List {
            Section {
                ForEach(model.projects, id: \.name) { project in
                    Button {
                        model.open(project: project.name)
                        dismiss()
                    } label: {
                        HStack {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(project.name).foregroundStyle(.primary)
                                Text(details(project)).font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                            if project.name == model.project {
                                Image(systemName: "checkmark").foregroundStyle(.tint)
                                    .accessibilityLabel("Open")
                            }
                        }
                    }
                    .accessibilityIdentifier("pythonLab.project.\(project.name)")
                    .forgeRowActions {
                        if !project.isScratch {
                            Button("Delete", systemImage: "trash", role: .destructive) { deleting = project }
                                .accessibilityIdentifier("pythonLab.project.delete")
                            Button("Rename", systemImage: "pencil") {
                                renaming = NameTarget(id: project.name, name: project.name)
                            }
                            .accessibilityIdentifier("pythonLab.project.rename")
                        }
                    }
                }
            } footer: {
                Text("Scratch is where Bonsai Harness works by default, so it can't be renamed or deleted.")
            }
        }
        .groupedList()
        .navigationTitle("Projects")
        .inlineNavigationTitle()
        .toolbar {
            Button("New Project", systemImage: "plus") { creating = true }
                .accessibilityIdentifier("pythonLab.projects.new")
        }
        .onAppear { model.reloadProjects() }
        .forgeRename(
            "New Project", isPresented: $creating, name: "", id: "pythonLab.projects.create",
            confirmTitle: "Create"
        ) { name in
            let refusal = model.createProject(name)
            if refusal == nil { dismiss() }
            return refusal
        }
        .forgeRename(
            "Rename Project", item: $renaming, name: \.name, id: "pythonLab.projects.renaming"
        ) { target, name in
            model.renameProject(target.id, to: name)
        }
        .confirmationDialog(
            deleting.map { "Delete \($0.name)?" } ?? "",
            isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
            titleVisibility: .visible,
            presenting: deleting
        ) { project in
            Button("Delete Project", role: .destructive) { model.deleteProject(project.name) }
                .accessibilityIdentifier("pythonLab.project.deleteConfirm")
        } message: { project in
            Text(project.files == 1 ? "Its 1 file is deleted too." : "Its \(project.files) files are deleted too.")
        }
    }

    private func details(_ project: WorkspaceProject) -> String {
        project.files == 1 ? "1 file" : "\(project.files) files"
    }
}

/// The open project's files and folders: open, run, rename, delete, add.
struct PythonFilesView: View {
    @Bindable var model: PythonLabModel
    @Environment(\.dismiss) private var dismiss
    /// The folder a new file or folder goes in (nil: the project's top).
    @State private var newFileIn: NameTarget?
    @State private var newFolderIn: NameTarget?
    @State private var renaming: NameTarget?
    @State private var deleting: WorkspaceEntry?

    var body: some View {
        List {
            // A file that wouldn't open, or a list that couldn't be read,
            // says so here, not on the screen underneath.
            if let error = model.error {
                Text(error).font(.footnote).forgeForeground(.danger)
                    .accessibilityIdentifier("pythonLab.files.error")
            } else if model.entries.isEmpty {
                Text("This project has no files yet.").foregroundStyle(.secondary)
            }
            ForEach(model.entries, id: \.path) { entry in
                row(entry)
                    .forgeRowActions {
                        Button("Delete", systemImage: "trash", role: .destructive) { deleting = entry }
                            .accessibilityIdentifier("pythonLab.file.delete")
                        Button("Rename", systemImage: "pencil") {
                            renaming = NameTarget(id: entry.path, name: Self.name(entry.path))
                        }
                        .accessibilityIdentifier("pythonLab.file.rename")
                        if entry.isFolder {
                            Button("New File Here", systemImage: "doc.badge.plus") {
                                newFileIn = NameTarget(id: entry.path, name: "")
                            }
                            .accessibilityIdentifier("pythonLab.file.newHere")
                        } else if entry.path.hasSuffix(".py") {
                            if model.runFile == entry.path {
                                Button("Run the Open File Instead", systemImage: "play.slash") {
                                    model.setRunFile(nil)
                                }
                                .accessibilityIdentifier("pythonLab.file.runOpen")
                            } else {
                                Button("Run This File", systemImage: "play") { model.setRunFile(entry.path) }
                                    .accessibilityIdentifier("pythonLab.file.runThis")
                            }
                        }
                    }
            }
        }
        .navigationTitle(model.project)
        .inlineNavigationTitle()
        .toolbar {
            Menu {
                Button("New File", systemImage: "doc.badge.plus") { newFileIn = NameTarget(id: "", name: "") }
                    .accessibilityIdentifier("pythonLab.files.newFile")
                Button("New Folder", systemImage: "folder.badge.plus") { newFolderIn = NameTarget(id: "", name: "") }
                    .accessibilityIdentifier("pythonLab.files.newFolder")
            } label: {
                Label("Add", systemImage: "plus")
            }
            .accessibilityIdentifier("pythonLab.files.add")
        }
        .onAppear {
            model.flush()
            model.reloadEntries()
            model.loadArtifacts()
        }
        .forgeRename(
            "New File", item: $newFileIn, name: { _ in "" }, id: "pythonLab.files.createFile",
            message: "For example main.py or data.csv.", confirmTitle: "Create"
        ) { target, name in
            let refusal = model.createFile(name, in: target.id.isEmpty ? nil : target.id)
            if refusal == nil { dismiss() }
            return refusal
        }
        .forgeRename(
            "New Folder", item: $newFolderIn, name: { _ in "" }, id: "pythonLab.files.createFolder",
            confirmTitle: "Create"
        ) { target, name in
            model.createFolder(name, in: target.id.isEmpty ? nil : target.id)
        }
        .forgeRename("Rename", item: $renaming, name: \.name, id: "pythonLab.files.renaming") { target, name in
            guard let entry = model.entries.first(where: { $0.path == target.id }) else {
                return "\(target.id) is no longer there."
            }
            return model.rename(entry, to: name)
        }
        .confirmationDialog(
            deleting.map { "Delete \(Self.name($0.path))?" } ?? "",
            isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
            titleVisibility: .visible,
            presenting: deleting
        ) { entry in
            Button(entry.isFolder ? "Delete Folder" : "Delete File", role: .destructive) { model.delete(entry) }
                .accessibilityIdentifier("pythonLab.file.deleteConfirm")
        } message: { entry in
            Text(entry.isFolder ? "Everything in it is deleted too." : "This can't be undone.")
        }
    }

    /// A file opens in the editor (an image in the image viewer); a folder
    /// is a heading for the rows under it.
    @ViewBuilder private func row(_ entry: WorkspaceEntry) -> some View {
        let depth = entry.path.filter { $0 == "/" }.count
        let label = HStack(spacing: 8) {
            Image(systemName: icon(entry)).foregroundStyle(.secondary).frame(width: 22)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(Self.name(entry.path)).foregroundStyle(.primary)
                if !entry.isFolder {
                    Text(details(entry)).font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer()
            if entry.path == model.openFile {
                Image(systemName: "pencil").foregroundStyle(.tint).accessibilityLabel("Open in the editor")
            }
            if entry.path == model.runFile {
                Image(systemName: "play.fill").foregroundStyle(.tint).accessibilityLabel("Run starts here")
            }
        }
        .padding(.leading, CGFloat(depth) * 16)
        if entry.isFolder {
            label.accessibilityIdentifier("pythonLab.folder.\(entry.path)")
        } else if let artifact = model.artifacts.first(where: { $0.path == entry.path }) {
            NavigationLink {
                ArtifactDetailView(artifact: artifact, url: model.url(of: artifact))
            } label: {
                label
            }
            .accessibilityIdentifier("pythonLab.file.\(entry.path)")
        } else {
            Button {
                model.open(file: entry.path)
                if model.openFile == entry.path { dismiss() }
            } label: {
                label
            }
            .accessibilityIdentifier("pythonLab.file.\(entry.path)")
        }
    }

    private func icon(_ entry: WorkspaceEntry) -> String {
        if entry.isFolder { return "folder" }
        if entry.path.hasSuffix(".py") { return "chevron.left.forwardslash.chevron.right" }
        if model.artifacts.contains(where: { $0.path == entry.path }) { return "photo" }
        return "doc.text"
    }

    private func details(_ entry: WorkspaceEntry) -> String {
        Format.storage(entry.bytes)
    }

    static func name(_ path: String) -> String {
        path.split(separator: "/").last.map(String.init) ?? path
    }
}
