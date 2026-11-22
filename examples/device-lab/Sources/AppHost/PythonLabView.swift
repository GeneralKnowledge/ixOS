@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI
import UniformTypeIdentifiers

/// Python Lab: a project of Python files, the open one in the editor (saved
/// as you type), and Run running it on the phone with the embedded CPython
/// (python-lab-workspace-plan.md, PL1b and PL1c).
struct PythonLabView: View {
    @Bindable var model: PythonLabModel
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.forgeTheme) private var theme
    /// The answer being typed for the script's `input()`.
    @State private var answer = ""
    @FocusState private var answerFocused: Bool

    var body: some View {
        List {
            if model.isLockedByBonsai {
                BonsaiLockBanner()
            }
            if model.hasWorkspace {
                projectSection
            }
            Section {
                if model.openFiles.count > 1 {
                    PythonFileTabs(model: model)
                        .listRowInsets(EdgeInsets(top: 6, leading: 8, bottom: 6, trailing: 8))
                }
                PythonCodeEditor(
                    text: $model.code,
                    isEditable: !(model.hasWorkspace && model.openFile == nil) && !model.isLockedByBonsai,
                    accessibilityLabel: model.openFile.map { "Python file \($0)" } ?? "Python script",
                    goTo: model.lineRequest)
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
                    .disabled(!model.hasPython || (model.hasWorkspace && model.fileToRun == nil) || model.isLockedByBonsai)
                    .accessibilityIdentifier("pythonLab.run")
                }
            } header: {
                Text(model.openFile ?? "Script")
            } footer: {
                VStack(alignment: .leading, spacing: 4) {
                    saveLine
                    Text("Runs on this iPhone with CPython. If the last line is an expression, its value is shown. A script is stopped after \(model.timeLimit.components.seconds) s of running; time spent waiting for your answer to input() doesn't count.")
                }
            }
            if let error = model.error ?? model.listError {
                Section {
                    Text(error).font(.footnote).forgeForeground(.danger)
                        .accessibilityIdentifier("pythonLab.error")
                }
            }
            consoleSection
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
                if let packages = model.packages {
                    NavigationLink {
                        PythonPackagesView(model: packages)
                    } label: {
                        LabeledContent("Packages", value: "\(packages.installed.count)")
                    }
                    .accessibilityIdentifier("pythonLab.packages")
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
        .onChange(of: model.isLockedByBonsai) { _, locked in
            // Save before Bonsai reads the files; read its work after.
            if locked { model.flush() } else { model.reloadFromDisk() }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase != .active { model.flush() }
        }
    }

    /// The live console (PL1d): output as the script writes it, stderr in
    /// the danger tone, your answers to input() marked, and a field for
    /// the answer while the script waits.
    @ViewBuilder private var consoleSection: some View {
        Section {
            if model.queuedBehindBonsai {
                Label("Waiting for Bonsai Harness's run to finish first.", systemImage: "hourglass")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("pythonLab.queued")
            }
            if !model.console.isEmpty {
                consoleText
                    .font(.system(.footnote, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityIdentifier("pythonLab.output")
            } else if model.isRunning {
                ProgressView("Running…")
                    .accessibilityIdentifier("pythonLab.running")
            } else {
                Text("Tap Run to see the output here.")
                    .foregroundStyle(.secondary)
            }
            // Where the error happened, to open: the last line is where it
            // was raised.
            ForEach(Array(model.errorFrames.reversed().enumerated()), id: \.offset) { index, frame in
                Button {
                    model.show(frame)
                } label: {
                    Label(
                        "\(frame.file), line \(frame.line)\(frame.function == "<module>" || frame.function.isEmpty ? "" : " in \(frame.function)")",
                        systemImage: index == 0 ? "exclamationmark.triangle" : "arrow.turn.down.right")
                }
                .accessibilityHint("Opens the file at this line")
                .accessibilityIdentifier("pythonLab.frame.\(index)")
            }
            if model.waitingForInput {
                HStack {
                    TextField("Your answer", text: $answer)
                        .font(.system(.body, design: .monospaced))
                        .noAutocapitalization()
                        .autocorrectionDisabled()
                        .submitLabel(.send)
                        .focused($answerFocused)
                        .onSubmit(sendAnswer)
                        .accessibilityLabel("Answer for the script")
                        .accessibilityIdentifier("pythonLab.answer")
                    Button("Send", systemImage: "return", action: sendAnswer)
                        .labelStyle(.iconOnly)
                        .accessibilityLabel("Send answer")
                        .accessibilityIdentifier("pythonLab.sendAnswer")
                }
                .onAppear { answerFocused = true }
            }
            HStack {
                if model.isRunning {
                    Button("Stop", systemImage: "stop.fill", role: .destructive) { model.stop() }
                        .accessibilityIdentifier("pythonLab.console.stop")
                } else {
                    Button("Rerun", systemImage: "arrow.clockwise") {
                        Task { await model.run() }
                    }
                    .disabled(!model.hasPython || (model.hasWorkspace && model.fileToRun == nil) || model.isLockedByBonsai)
                    .accessibilityIdentifier("pythonLab.rerun")
                }
                Spacer()
                if model.hasWorkspace {
                    NavigationLink {
                        PythonRunHistoryView(model: model)
                    } label: {
                        Label("History", systemImage: "clock")
                    }
                    .fixedSize()
                    .accessibilityLabel("Run history, \(model.runs.count) runs")
                    .accessibilityIdentifier("pythonLab.history")
                    Spacer()
                }
                Button("Clear", systemImage: "trash") { model.clearConsole() }
                    .disabled(model.console.isEmpty)
                    .accessibilityIdentifier("pythonLab.clear")
            }
            .buttonStyle(.borderless)
        } header: {
            Text(model.lastRunFile.map { "Console: \($0)" } ?? "Console")
        } footer: {
            if model.waitingForInput {
                Text("The script is waiting for your answer.")
                    .accessibilityIdentifier("pythonLab.waiting")
            } else if let seconds = model.lastSeconds, !model.isRunning {
                if model.succeeded == false {
                    Text("Ended with an error after \(Format.number(seconds, digits: 3)) s")
                        .forgeForeground(.danger)
                } else {
                    Text("Finished in \(Format.number(seconds, digits: 3)) s")
                }
            }
        }
    }

    /// The console as one text, each stretch in its colour.
    private var consoleText: Text {
        let colors = theme.colors
        var text = Text("")
        if let notice = model.console.removedNotice {
            text = text + Text(notice + "\n").foregroundStyle(colors.textSecondary)
        }
        for segment in model.console.segments {
            switch segment.kind {
            case .output:
                text = text + Text(segment.text)
            case .error:
                text = text + Text(segment.text).foregroundStyle(colors.tone(.danger).text)
            case .input:
                text = text + Text(segment.text).bold().foregroundStyle(colors.tone(.accent).text)
            case .note:
                text = text + Text(segment.text).foregroundStyle(colors.textSecondary)
            }
        }
        return text
    }

    private func sendAnswer() {
        if model.sendInput(answer) {
            answer = ""
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
                Text(model.succeeded == nil ? "Files a script saves, like plot.png or results.csv, show here." : "The last run made no files.")
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
                    LabeledContent("All Outputs", value: "\(model.artifacts.count)")
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
    @State private var importing = false
    @State private var importRefusal: String?
    /// The project's .zip, once exported, for the share button.
    @State private var exported: URL?

    var body: some View {
        List {
            if model.isLockedByBonsai {
                BonsaiLockBanner()
            }
            if let exported {
                ShareLink(item: exported) {
                    Label("Share \(exported.lastPathComponent)", systemImage: "square.and.arrow.up")
                }
                .accessibilityIdentifier("pythonLab.files.shareExport")
            }
            if let importRefusal {
                Text(importRefusal).font(.footnote).forgeForeground(.danger)
                    .accessibilityIdentifier("pythonLab.files.importError")
            }
            // A file that wouldn't open, or a list that couldn't be read,
            // says so here, not on the screen underneath.
            if let error = model.listError ?? model.error {
                Text(error).font(.footnote).forgeForeground(.danger)
                    .accessibilityIdentifier("pythonLab.files.error")
            } else if model.entries.isEmpty {
                Text("This project has no files yet.").foregroundStyle(.secondary)
            }
            ForEach(model.entries, id: \.path) { entry in
                row(entry)
                    .forgeRowActions {
                        Button("Delete", systemImage: "trash", role: .destructive) { deleting = entry }
                            .disabled(model.isLockedByBonsai)
                            .accessibilityIdentifier("pythonLab.file.delete")
                        Button("Rename", systemImage: "pencil") {
                            renaming = NameTarget(id: entry.path, name: Self.name(entry.path))
                        }
                        .disabled(model.isLockedByBonsai)
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
                Button("Import from Files", systemImage: "square.and.arrow.down") { importing = true }
                    .accessibilityIdentifier("pythonLab.files.import")
            } label: {
                Label("Add", systemImage: "plus")
            }
            .disabled(model.isLockedByBonsai)
            .accessibilityIdentifier("pythonLab.files.add")
            Button("Export Project", systemImage: "square.and.arrow.up") {
                exported = model.exportProject()
            }
            .accessibilityLabel("Export \(model.project) as a zip file")
            .accessibilityIdentifier("pythonLab.files.export")
        }
        .fileImporter(
            isPresented: $importing, allowedContentTypes: [.item], allowsMultipleSelection: true
        ) { result in
            switch result {
            case .success(let urls):
                importRefusal = model.importFiles(urls)
            case .failure(let error):
                importRefusal = error.localizedDescription
            }
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
        } else if let artifact = model.artifacts.first(where: { $0.path == entry.path }),
                  !ArtifactKind.editable.contains(artifact.kind) {
            // Text, JSON and CSV open in the editor here; previews are under
            // Artifacts.
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

/// Says Bonsai Harness is working in the open project, which is read-only
/// until it finishes (PL1e).
struct BonsaiLockBanner: View {
    var body: some View {
        Label(
            "Bonsai is working in this project. It's read-only until Bonsai finishes, so you and Bonsai never change a file at once.",
            systemImage: "hammer"
        )
        .font(.footnote)
        .forgeForeground(.warning)
        .accessibilityIdentifier("pythonLab.bonsaiWorking")
    }
}

/// Pure-Python packages from PyPI (PL2a): install by name, with what it
/// needs; remove with a swipe.
struct PythonPackagesView: View {
    @Bindable var model: PythonPackagesModel

    var body: some View {
        List {
            Section {
                TextField("Package name, like rich or attrs", text: $model.name)
                    .noAutocapitalization()
                    .autocorrectionDisabled()
                    .submitLabel(.go)
                    .onSubmit { Task { await model.install() } }
                    .disabled(model.isWorking)
                    .accessibilityIdentifier("pythonLab.packages.name")
                if model.isWorking {
                    ProgressView("Installing…")
                        .accessibilityIdentifier("pythonLab.packages.working")
                } else {
                    Button("Install", systemImage: "arrow.down.circle") {
                        Task { await model.install() }
                    }
                    .disabled(model.name.trimmingCharacters(in: .whitespaces).isEmpty)
                    .accessibilityIdentifier("pythonLab.packages.install")
                }
                ForEach(model.log, id: \.self) { line in
                    Text(line).font(.footnote)
                }
                if let error = model.error {
                    Text(error).font(.footnote).forgeForeground(.danger)
                        .accessibilityIdentifier("pythonLab.packages.error")
                }
            } footer: {
                Text("Pure-Python packages from PyPI, with the packages they need. Packages with compiled code (numpy, pandas) can't be installed here: iOS runs compiled code only when it's built into the app. A package already imported keeps its old version until Device Lab restarts.")
            }
            Section {
                if model.installed.isEmpty {
                    Text("None yet.").foregroundStyle(.secondary)
                }
                ForEach(model.installed, id: \.name) { package in
                    row(package)
                        .forgeRowActions {
                            Button("Remove", systemImage: "trash", role: .destructive) {
                                model.remove(package)
                            }
                            .accessibilityIdentifier("pythonLab.packages.remove")
                        }
                }
            } header: {
                Text("Installed")
            }
            if !model.bundled.isEmpty {
                Section {
                    ForEach(model.bundled, id: \.name) { row($0) }
                } header: {
                    Text("Built into this app")
                }
            }
        }
        .groupedList()
        .navigationTitle("Packages")
        .inlineNavigationTitle()
        .onAppear { model.reload() }
    }

    private func row(_ package: InstalledPackage) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            LabeledContent(package.name, value: package.version)
            if !package.summary.isEmpty {
                Text(package.summary).font(.caption).foregroundStyle(.secondary).lineLimit(2)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("pythonLab.packages.row.\(package.name)")
    }
}

/// The open project's last runs (PL2c): which file, when, how it ended and
/// what it printed.
struct PythonRunHistoryView: View {
    @Bindable var model: PythonLabModel

    var body: some View {
        List {
            if model.runs.isEmpty {
                Text("No runs in \(model.project) yet.").foregroundStyle(.secondary)
            }
            ForEach(Array(model.runs.enumerated()), id: \.offset) { index, run in
                NavigationLink {
                    ScrollView {
                        Text(run.output.isEmpty ? "(no output)" : run.output)
                            .font(.system(.footnote, design: .monospaced))
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding()
                    }
                    .navigationTitle(run.file)
                    .inlineNavigationTitle()
                } label: {
                    HStack(spacing: 10) {
                        Image(systemName: run.ok ? "checkmark.circle" : "xmark.octagon")
                            .forgeForeground(run.ok ? .success : .danger)
                            .accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(run.file)
                            Text(Self.summary(run)).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityValue(run.ok ? "Finished" : "Ended with an error")
                }
                .accessibilityIdentifier("pythonLab.history.\(index)")
            }
        }
        .groupedList()
        .navigationTitle("History")
        .inlineNavigationTitle()
        .toolbar {
            Button("Clear History", systemImage: "trash", role: .destructive) { model.clearRuns() }
                .disabled(model.runs.isEmpty)
                .accessibilityIdentifier("pythonLab.history.clear")
        }
        .onAppear { model.loadRuns() }
    }

    static func summary(_ run: RunRecord) -> String {
        let date = Date(timeIntervalSince1970: Double(run.startedMs) / 1000)
        let when = date.formatted(date: .abbreviated, time: .shortened)
        let how = run.timedOut ? "stopped at the time limit" : run.ok ? "finished" : "error"
        return "\(when), \(Format.number(run.seconds, digits: 2)) s, \(how)"
    }
}

/// The open files as tabs above the editor (PL2): tap one to switch, its
/// cross to close it. Shown once more than one file is open.
struct PythonFileTabs: View {
    @Bindable var model: PythonLabModel
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(model.openFiles, id: \.self) { path in
                        tab(path)
                    }
                }
            }
            .onChange(of: model.openFile, initial: true) { _, current in
                if let current {
                    withAnimation { proxy.scrollTo(current) }
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Open files")
        .accessibilityIdentifier("pythonLab.tabs")
    }

    private func tab(_ path: String) -> some View {
        let current = path == model.openFile
        let name = path.split(separator: "/").last.map(String.init) ?? path
        return HStack(spacing: 2) {
            Button(name) { model.open(file: path) }
                .font(.footnote.monospaced())
                .lineLimit(1)
                .accessibilityLabel(current ? "\(path), open" : path)
                .accessibilityAddTraits(current ? .isSelected : [])
                .accessibilityIdentifier("pythonLab.tab.\(path)")
            Button("Close \(path)", systemImage: "xmark") { model.close(file: path) }
                .labelStyle(.iconOnly)
                .font(.caption2)
                .accessibilityIdentifier("pythonLab.tab.close.\(path)")
        }
        .padding(.vertical, 4)
        .padding(.horizontal, 8)
        .background(
            Capsule().fill(current ? theme.colors.tone(.accent).soft : theme.colors.tone(.neutral).soft))
        .foregroundStyle(current ? theme.colors.tone(.accent).text : theme.colors.textSecondary)
        .buttonStyle(.borderless)
        .id(path)
    }
}
