@preconcurrency import ForgeFFI
import Foundation
import Observation

/// Runs Python code on the phone: what Python Lab's screen shows. The
/// interpreter is CPython, embedded in the app (PythonRuntime.swift); this
/// model only needs something that runs a script, so it builds and tests
/// without it.
public protocol PythonRunning: Sendable {
    /// Starts the interpreter (once) and returns its version.
    func start() async throws -> String
    /// Runs a script as the run `id`, handing its output, and its waits for
    /// `input()`, to `events` as they happen (on the interpreter's thread:
    /// copy and return). The interpreter is shared with Bonsai Harness, so
    /// the script may first wait for its turn. `limit` counts only the
    /// time it runs: not its wait for its turn, nor its waits for an
    /// answer. `timedOut` when the limit stopped it. Everything the script
    /// wrote went to `events`, so `output` holds only the runtime's own
    /// words ("Stopped before it started.").
    func run(
        _ code: String, id: UUID, limit: Duration,
        events: @escaping @Sendable (PythonConsoleEvent) -> Void
    ) async -> PythonRunResult
    /// Answers the `input()` run `id` is waiting for. False when it isn't
    /// waiting.
    func send(input line: String, id: UUID) -> Bool
    /// Stops the run `id` (a KeyboardInterrupt in it, also while it waits
    /// for an answer) if it is the one running; never another caller's
    /// script.
    func interrupt(id: UUID)
}

public struct PythonRunResult: Sendable {
    /// False when the script ended with an exception.
    public var ok: Bool
    public var output: String
    public var timedOut: Bool

    public init(ok: Bool, output: String, timedOut: Bool = false) {
        self.ok = ok
        self.output = output
        self.timedOut = timedOut
    }
}

public struct PythonLabSample: Identifiable, Sendable {
    public let id: String
    public let title: String
    public let code: String
}

/// Python Lab's workspace (python-lab-workspace-plan.md, PL1b): projects of
/// real files, the open file edited and saved as you type, and Run running
/// a file from disk. Rust (`Workspace`) owns the files and their rules.
@MainActor
@Observable
public final class PythonLabModel {
    /// Whether what's on screen is on disk.
    public enum SaveState: Equatable, Sendable {
        case saved
        case unsaved
        case failed(String)
    }

    /// The open file's text as last loaded or typed.
    public private(set) var text: String
    /// The open file's text, for the editor. Typing saves a second after it
    /// stops.
    public var code: String {
        get { text }
        set {
            // Read-only while Bonsai works in this project.
            guard newValue != text, !isLockedByBonsai else { return }
            text = newValue
            saveState = .unsaved
            // A new edit after the warning: the person chose to save it.
            heldBack = false
            scheduleSave()
        }
    }
    public private(set) var saveState: SaveState = .saved
    /// Text whose save failed when Bonsai started in this project: it
    /// stays on screen but isn't written over Bonsai's work, until the
    /// person edits it again.
    private var heldBack = false
    /// The last run's console: stdout, stderr, the answers given, notes.
    public private(set) var console = PythonConsoleText()
    /// The console as plain text.
    public var output: String { console.plainText }
    /// The running script waits for an answer to `input()`.
    public private(set) var waitingForInput = false
    /// The run waits for a Bonsai Harness run to finish first.
    public private(set) var queuedBehindBonsai = false
    /// The project's lines in the last run's traceback, outermost first,
    /// to open where the error happened (PL2b).
    public private(set) var errorFrames: [TracebackFrame] = []
    /// The line the editor should show next.
    public private(set) var lineRequest: PythonLineRequest?
    /// The open project's runs, newest first (PL2c).
    public private(set) var runs: [RunRecord] = []
    /// Whether the last run finished without an exception (nil before one).
    public private(set) var succeeded: Bool?
    public private(set) var isRunning = false
    public private(set) var lastSeconds: Double?
    /// The file the last run ran.
    public private(set) var lastRunFile: String?
    public private(set) var version: String?
    /// Why Python isn't available (not in this build, or it failed to start).
    public private(set) var unavailable: String?
    /// The last action that failed, with why (cleared by the next one).
    public internal(set) var error: String?
    /// Why the open project's files couldn't be listed, kept apart from
    /// `error` so a later listing that works clears just this.
    public private(set) var listError: String?
    /// A script running longer than this is stopped.
    public let timeLimit: Duration

    /// The projects, scratch first.
    public private(set) var projects: [WorkspaceProject] = []
    /// The open project.
    public private(set) var project: String
    /// Its files and folders, in tree order.
    public private(set) var entries: [WorkspaceEntry] = []
    /// The file in the editor, relative to the project.
    public private(set) var openFile: String?
    /// The files open as tabs above the editor, in the order opened (PL2);
    /// `openFile` is one of them. Only the one in the editor is held in
    /// memory: switching saves it and reads the next.
    public private(set) var openFiles: [String] = []

    /// At most this many tabs; opening another closes the oldest.
    public static let maxOpenFiles = 8

    /// Where Debug pauses in this project (PL2f), kept with the project.
    public private(set) var breakpoints: [DebugBreakpoint] = []
    /// The run is under the debugger.
    public private(set) var isDebugging = false
    /// Where the debugger paused and what it sees; nil while the script
    /// runs on.
    public private(set) var debugPause: DebugPause?
    /// The project the debugger runs in: pauses open its files only there.
    private var debugProject: String?
    /// The debugger is waiting for Continue or a step.
    public private(set) var awaitingDebugCommand = false

    /// The open file's breakpoint lines, for the editor's gutter.
    public var breakpointLines: Set<Int> {
        Set(breakpoints.filter { $0.file == openFile }.map { Int($0.line) })
    }

    /// The paused line, when it is in the open file.
    public var pausedLine: Int? {
        guard let top = debugPause?.stack.last, top.file == openFile else { return nil }
        return Int(top.line)
    }
    /// The file Run starts; nil runs the open file.
    public private(set) var runFile: String?

    /// The project's outputs (images, tables, text, anything but Python),
    /// newest first (PL2e).
    public private(set) var artifacts: [ProjectArtifact] = []
    /// The outputs the last run made or changed.
    public private(set) var madeByLastRun: [ProjectArtifact] = []

    public let samples: [PythonLabSample]

    @ObservationIgnored private let runtime: (any PythonRunning)?
    @ObservationIgnored private let workspace: Workspace?
    @ObservationIgnored private var started = false
    @ObservationIgnored private var pendingSave: Task<Void, Never>?
    @ObservationIgnored private var runID: UUID?
    /// Stop was tapped before the run had an id (while Python started).
    @ObservationIgnored private var stopRequested = false
    /// Remembers the last project between launches.
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let activity: PythonActivity
    /// Pure-Python packages from PyPI (PL2a); nil in a build with no Python.
    @ObservationIgnored public let packages: PythonPackagesModel?
    static let lastProjectKey = "pythonLab.lastProject"

    /// `workspace` nil keeps the code in memory only (a build or test with
    /// no projects folder).
    public init(
        runtime: (any PythonRunning)?, workspace: Workspace? = nil,
        timeLimit: Duration = .seconds(30), defaults: UserDefaults = .standard,
        activity: PythonActivity = .shared, packages: PythonPackagesModel? = nil
    ) {
        self.runtime = runtime
        self.activity = activity
        self.packages = packages
        self.workspace = workspace
        self.timeLimit = timeLimit
        self.defaults = defaults
        // The graphics samples write files, so they need a project.
        samples =
            workspace == nil
            ? Self.allSamples
            : Self.allSamples
                + pythonLabGraphicsSamples().map {
                    PythonLabSample(id: $0.id, title: $0.title, code: $0.code)
                }
        text = Self.allSamples[0].code
        project = "scratch"
        if runtime == nil {
            unavailable = "This build of Device Lab has no Python."
        }
        if workspace != nil {
            open(project: defaults.string(forKey: Self.lastProjectKey) ?? "scratch")
        }
    }

    /// Whether this build has an interpreter at all.
    public var hasPython: Bool { runtime != nil }
    /// Whether files are saved (a projects folder exists).
    public var hasWorkspace: Bool { workspace != nil }

    /// The open project's folder.
    public var projectFolder: String? {
        try? workspace?.projectFolder(project: project)
    }

    /// The file Run will run.
    public var fileToRun: String? { runFile ?? openFile }

    /// Bonsai Harness is working in the open project: it is shown
    /// read-only, so the person and Bonsai never write one file at once
    /// (PL1e).
    public var isLockedByBonsai: Bool {
        hasWorkspace && activity.bonsaiProject == project
    }

    /// Bonsai is about to work in `name`: text whose save just failed
    /// there is held back now, whether or not Python Lab is on screen.
    public func bonsaiStarting(in name: String) {
        guard hasWorkspace, name == project, case .failed = saveState else { return }
        heldBack = true
    }

    /// Reads the open file and the project's files again from disk, after
    /// Bonsai worked on them. Edits not yet saved are kept, not replaced.
    public func reloadFromDisk() {
        guard let workspace else { return }
        reloadEntries()
        loadArtifacts()
        // Tabs whose files Bonsai removed close (the one in the editor stays).
        openFiles.removeAll { tab in
            tab != openFile && !entries.contains { $0.path == tab }
        }
        if heldBack, let openFile, !isLockedByBonsai {
            error = "Your changes to \(openFile) couldn't be saved before Bonsai started, and Bonsai may have changed the file since. They're still on screen, not saved; your next edit saves them over Bonsai's version."
        }
        guard let openFile, saveState == .saved else { return }
        if let fresh = try? workspace.readText(project: project, path: openFile), fresh != text {
            text = fresh
        } else if !entries.contains(where: { $0.path == openFile }) {
            // Bonsai removed or renamed it: the text stays on screen,
            // unsaved, so nothing of it is lost.
            saveState = .unsaved
        }
    }

    /// Why a change to the project is refused while Bonsai works in it.
    static let bonsaiWorking = "Bonsai is working in this project. Wait until it finishes, or stop it in Bonsai Harness."

    // MARK: Import and export

    /// Copies files the person picked in the Files app into the project
    /// (into `folder`, if given). Returns why any couldn't be, or nil.
    public func importFiles(_ urls: [URL], into folder: String? = nil) -> String? {
        guard let workspace else { return nil }
        if isLockedByBonsai { return Self.bonsaiWorking }
        var refusals: [String] = []
        for url in urls {
            // A file from the picker may be outside the app's sandbox.
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            do {
                _ = try workspace.importFile(project: project, folder: folder ?? "", source: url.path)
            } catch {
                refusals.append("\(url.lastPathComponent): \(Self.message(error))")
            }
        }
        reloadEntries()
        loadArtifacts()
        return refusals.isEmpty ? nil : refusals.joined(separator: "\n")
    }

    /// The open project as a .zip in a temporary folder, to share. Python
    /// Lab's projects are also plain folders in the Files app.
    public func exportProject() -> URL? {
        guard let workspace else { return nil }
        flush()
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("Python Lab export \(UUID().uuidString)", isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let path = try workspace.exportProject(project: project, outFolder: folder.path)
            error = nil
            return URL(fileURLWithPath: path)
        } catch {
            self.error = "Couldn't export \(project): \(Self.message(error))"
            return nil
        }
    }

    /// Starts the interpreter if it hasn't started, for the version line.
    public func prepare() async {
        guard let runtime, !started else { return }
        do {
            version = try await runtime.start()
            started = true
            unavailable = nil
        } catch {
            unavailable = "Python did not start: \(error.localizedDescription)"
        }
    }

    // MARK: Projects

    public func reloadProjects() {
        projects = workspace?.projects() ?? []
    }

    /// Opens a project, and in it the file that was open last (or the
    /// first Python file, or a new main.py when it has none).
    public func open(project name: String) {
        guard let workspace, savedBeforeLeaving() else { return }
        reloadProjects()
        let target = projects.contains { $0.name == name } ? name : "scratch"
        // A project whose files can't be listed isn't opened: the one
        // that's open stays, with its text and output.
        let listed: [WorkspaceEntry]
        do {
            listed = try workspace.entries(project: target)
        } catch {
            self.error = "Couldn't open \(target): \(Self.message(error))"
            return
        }
        // The new project may have a file of the same name.
        openFile = nil
        project = target
        defaults.set(target, forKey: Self.lastProjectKey)
        let state = (try? workspace.state(project: target)) ?? WorkspaceState(
            runFile: nil, currentFile: nil, openFiles: [], breakpoints: [])
        runFile = state.runFile
        entries = listed
        listError = nil
        // Rust already left out tabs whose files are gone.
        openFiles = Array(state.openFiles.prefix(Self.maxOpenFiles))
        breakpoints = state.breakpoints
        if let current = state.currentFile {
            open(file: current)
        } else if let first = entries.first(where: { !$0.isFolder && $0.path.hasSuffix(".py") }) {
            open(file: first.path)
        } else {
            createFirstFile()
        }
        loadArtifacts()
        loadRuns()
        madeByLastRun = []
        if !isRunning {
            succeeded = nil
            console = PythonConsoleText()
        }
    }

    public func createProject(_ name: String) -> String? {
        guard let workspace else { return nil }
        do {
            try workspace.createProject(name: name)
        } catch {
            return Self.message(error)
        }
        open(project: name.trimmingCharacters(in: .whitespaces))
        return nil
    }

    public func renameProject(_ old: String, to new: String) -> String? {
        guard let workspace else { return nil }
        // Bonsai's task works in it by name.
        if activity.bonsaiProject == old { return Self.bonsaiWorking }
        if old == project { flush() }
        do {
            try workspace.renameProject(from: old, to: new)
        } catch {
            return Self.message(error)
        }
        let trimmed = new.trimmingCharacters(in: .whitespaces)
        if old == project {
            project = trimmed
            defaults.set(trimmed, forKey: Self.lastProjectKey)
        }
        reloadProjects()
        return nil
    }

    public func deleteProject(_ name: String) {
        guard let workspace else { return }
        if activity.bonsaiProject == name {
            error = Self.bonsaiWorking
            return
        }
        do {
            try workspace.deleteProject(name: name)
            error = nil
        } catch {
            self.error = Self.message(error)
            return
        }
        if name == project {
            // Nothing to save: the files are gone.
            pendingSave?.cancel()
            saveState = .saved
            openFile = nil
            open(project: "scratch")
        } else {
            reloadProjects()
        }
    }

    // MARK: Files

    /// Lists the open project's files; false, with the reason in `error`,
    /// when they can't be read.
    @discardableResult
    public func reloadEntries() -> Bool {
        guard let workspace else {
            entries = []
            return true
        }
        do {
            entries = try workspace.entries(project: project)
            listError = nil
            return true
        } catch {
            entries = []
            listError = "Couldn't list the files in \(project): \(Self.message(error))"
            return false
        }
    }

    /// Saves the open file before another takes its place. If it can't be
    /// saved, stays on it, so the only copy of the edits isn't replaced.
    private func savedBeforeLeaving() -> Bool {
        flush()
        if case .failed(let reason) = saveState {
            error = "Your changes to \(openFile ?? "this file") aren't saved (\(reason)), so it stays open. Fix that, or undo the changes, first."
            return false
        }
        return true
    }

    /// Opens a file in the editor, saving the one that was open first.
    public func open(file path: String) {
        guard let workspace else { return }
        if path == openFile { return }
        guard savedBeforeLeaving() else { return }
        do {
            let text = try workspace.readText(project: project, path: path)
            self.text = text
            openFile = path
            addTab(path)
            saveState = .saved
            error = nil
            saveProjectState()
        } catch {
            self.error = Self.message(error)
        }
    }

    /// Closes a tab. Closing the one in the editor saves it and shows the
    /// tab beside it; the last tab can't be closed (the editor always has
    /// a file).
    public func close(file path: String) {
        guard openFiles.count > 1, let index = openFiles.firstIndex(of: path) else { return }
        if path == openFile {
            let next = openFiles[index + 1 < openFiles.count ? index + 1 : index - 1]
            open(file: next)
            // Not switched (the save failed): the tab stays.
            guard openFile == next else { return }
        }
        openFiles.removeAll { $0 == path }
        saveProjectState()
    }

    private func addTab(_ path: String) {
        guard !openFiles.contains(path) else { return }
        openFiles.append(path)
        if openFiles.count > Self.maxOpenFiles {
            // The oldest tab that isn't the one just opened.
            if let oldest = openFiles.firstIndex(where: { $0 != path }) {
                openFiles.remove(at: oldest)
            }
        }
    }

    /// A new file in the project (in `folder`, if given), opened.
    public func createFile(_ name: String, in folder: String? = nil, text: String = "") -> String? {
        guard let workspace else { return nil }
        if isLockedByBonsai { return Self.bonsaiWorking }
        // The new file opens next, so the open one must save first; if it
        // can't, nothing is created.
        guard savedBeforeLeaving() else { return error }
        let path = Self.join(folder, name)
        do {
            // Never replaces a file that's there, whatever its case.
            try workspace.createFile(project: project, path: path, text: text)
        } catch {
            return Self.message(error)
        }
        reloadEntries()
        open(file: path)
        return nil
    }

    public func createFolder(_ name: String, in folder: String? = nil) -> String? {
        guard let workspace else { return nil }
        if isLockedByBonsai { return Self.bonsaiWorking }
        do {
            try workspace.createFolder(project: project, path: Self.join(folder, name))
        } catch {
            return Self.message(error)
        }
        reloadEntries()
        return nil
    }

    /// Renames a file or folder in place (same folder, new name).
    public func rename(_ entry: WorkspaceEntry, to name: String) -> String? {
        guard let workspace else { return nil }
        if isLockedByBonsai { return Self.bonsaiWorking }
        let parent = entry.path.split(separator: "/").dropLast().joined(separator: "/")
        let target = Self.join(parent.isEmpty ? nil : parent, name)
        flush()
        do {
            try workspace.renameEntry(project: project, from: entry.path, to: target)
        } catch {
            return Self.message(error)
        }
        openFile = openFile.map { Self.moved($0, from: entry.path, to: target) }
        openFiles = openFiles.map { Self.moved($0, from: entry.path, to: target) }
        breakpoints = breakpoints.map {
            DebugBreakpoint(file: Self.moved($0.file, from: entry.path, to: target), line: $0.line)
        }
        runFile = runFile.map { Self.moved($0, from: entry.path, to: target) }
        saveProjectState()
        reloadEntries()
        return nil
    }

    public func delete(_ entry: WorkspaceEntry) {
        guard let workspace else { return }
        if isLockedByBonsai {
            error = Self.bonsaiWorking
            return
        }
        let inside = { (path: String?) in
            path.map { $0 == entry.path || $0.hasPrefix(entry.path + "/") } ?? false
        }
        do {
            try workspace.deleteEntry(project: project, path: entry.path)
            error = nil
        } catch {
            // The edits stay unsaved until the file is really gone; the
            // list shows what is left of a folder that went only in part.
            self.error = Self.message(error)
            reloadEntries()
            return
        }
        if inside(openFile) {
            // Nothing to save: the file is gone.
            pendingSave?.cancel()
            pendingSave = nil
            saveState = .saved
        }
        if inside(runFile) { runFile = nil }
        reloadEntries()
        openFiles.removeAll { inside($0) && $0 != openFile }
        breakpoints.removeAll { inside($0.file) }
        if inside(openFile) {
            let gone = openFile
            openFile = nil
            openFiles.removeAll { $0 == gone }
            if let tab = openFiles.last {
                open(file: tab)
            } else if let first = entries.first(where: { !$0.isFolder && $0.path.hasSuffix(".py") }) {
                open(file: first.path)
            } else {
                text = ""
            }
        }
        saveProjectState()
        loadArtifacts()
    }

    /// Makes `path` the file Run starts, or nil for the open file.
    public func setRunFile(_ path: String?) {
        runFile = path
        saveProjectState()
    }

    /// Adds a sample to the project as a new file (`hello.py`, then
    /// `hello 2.py`, ...) and opens it; what you wrote stays where it was.
    public func load(_ sample: PythonLabSample) {
        guard let workspace else {
            text = sample.code
            return
        }
        if isLockedByBonsai {
            error = Self.bonsaiWorking
            return
        }
        guard savedBeforeLeaving() else { return }
        // The first free name; Rust refuses one that is taken, even when
        // the list is out of date or the case differs.
        for n in 1...100 {
            let name = n == 1 ? "\(sample.id).py" : "\(sample.id) \(n).py"
            do {
                try workspace.createFile(project: project, path: name, text: sample.code)
            } catch WorkspaceError.Exists {
                continue
            } catch {
                self.error = Self.message(error)
                return
            }
            reloadEntries()
            open(file: name)
            return
        }
        error = "There are already 100 copies of \(sample.title)."
    }

    // MARK: Saving

    /// Saves the open file now if it changed (leaving the screen, the app
    /// going to the background, opening another file, running).
    public func flush() {
        pendingSave?.cancel()
        pendingSave = nil
        guard saveState != .saved, let workspace, let openFile else { return }
        // Text whose save already failed isn't retried while Bonsai works
        // in the project (edits from before it started still save).
        if case .failed = saveState, isLockedByBonsai { heldBack = true }
        guard !heldBack else { return }
        do {
            try workspace.writeText(project: project, path: openFile, text: code)
            saveState = .saved
        } catch {
            saveState = .failed(Self.message(error))
            // Failing while Bonsai works: not retried over its work later.
            if isLockedByBonsai { heldBack = true }
        }
    }

    private func scheduleSave() {
        pendingSave?.cancel()
        pendingSave = Task { [weak self] in
            try? await Task.sleep(for: .seconds(1))
            guard !Task.isCancelled else { return }
            self?.flush()
            self?.reloadEntries()
        }
    }

    private func saveProjectState() {
        guard let workspace else { return }
        let state = WorkspaceState(
            runFile: runFile, currentFile: openFile, openFiles: openFiles, breakpoints: breakpoints)
        do {
            try workspace.saveState(project: project, state: state)
        } catch {
            // Only which file was open is lost, never what was written.
            self.error = "Python Lab couldn't remember the open file: \(Self.message(error))"
        }
    }

    private func createFirstFile() {
        text = ""
        openFile = nil
        if let refusal = createFile("main.py", text: Self.firstFile) {
            error = refusal
        }
    }

    /// What a project with no Python file starts with.
    static let firstFile = """
        print("Hello from Python Lab")

        """

    // MARK: Running

    /// Runs the file, or with `debugging` runs it under the debugger so it
    /// pauses on the project's breakpoints (PL2f).
    public func run(debugging: Bool = false) async {
        guard let runtime, !isRunning, !isLockedByBonsai else { return }
        // Debugging needs a project file to set breakpoints in.
        if debugging && (projectFolder == nil || fileToRun == nil) { return }
        // Taken before the first wait, so a second tap can't start a
        // second run while Python starts.
        isRunning = true
        stopRequested = false
        await prepare()
        // Stop tapped while Python started: nothing runs.
        if stopRequested {
            isRunning = false
            console = PythonConsoleText()
            console.append("Stopped before it started.", as: .note)
            return
        }
        // Bonsai started in this project while Python started.
        if isLockedByBonsai {
            isRunning = false
            error = Self.bonsaiWorking
            return
        }
        // Saved after Python started, so edits made while it started run too.
        flush()
        if case .failed(let reason) = saveState {
            error = "Not run, because the file couldn't be saved: \(reason)"
            isRunning = false
            return
        }
        guard started else {
            isRunning = false
            return
        }
        console = PythonConsoleText()
        errorFrames = []
        succeeded = nil
        error = nil
        let id = UUID()
        runID = id
        var script = code
        var before: [String: ProjectArtifact] = [:]
        let startedIn = project
        debugProject = debugging ? startedIn : nil
        let folder = projectFolder
        let file = fileToRun
        if let folder, let file {
            script = debugging
                ? pythonLabDebugScript(project: folder, path: file, breakpoints: breakpoints)
                : pythonLabConsoleScript(project: folder, path: file)
            before = Dictionary(
                projectOutputs(project: folder).map { ($0.path, $0) },
                uniquingKeysWith: { first, _ in first })
        }
        lastRunFile = file
        isDebugging = debugging
        debugPause = nil
        awaitingDebugCommand = false
        // Output reaches the screen at most every 50 ms.
        let inbox = ConsoleInbox()
        let pump = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(50))
                self?.apply(inbox.drain())
            }
        }
        let clock = ContinuousClock()
        let begin = clock.now
        let result = await runtime.run(script, id: id, limit: timeLimit) { inbox.push($0) }
        let elapsed = clock.now - begin
        pump.cancel()
        apply(inbox.drain())
        runID = nil
        waitingForInput = false
        activity.labWaitingForInput = false
        queuedBehindBonsai = false
        // A run whose project is no longer open shows its output, but its
        // images and file list belong to that project, not this one.
        if let folder, project == startedIn {
            if !result.ok {
                let stderr = console.segments.filter { $0.kind == .error }.map(\.text).joined()
                errorFrames = pythonTracebackFrames(output: stderr)
            }
            artifacts = projectOutputs(project: folder)
            madeByLastRun = artifacts.filter { artifact in
                guard let old = before[artifact.path] else { return true }
                return old.bytes != artifact.bytes || old.modifiedMs != artifact.modifiedMs
            }
            reloadEntries()
        }
        console.append(result.output, as: .note)
        if result.timedOut {
            console.append(
                "\nReached the \(timeLimit.components.seconds) s limit, so Python Lab asked the script to stop.\n",
                as: .note)
        }
        if console.isEmpty {
            console.append("(no output)", as: .note)
        }
        lastSeconds = Double(elapsed.components.seconds)
            + Double(elapsed.components.attoseconds) / 1e18
        succeeded = result.ok
        if let workspace, let file {
            let record = RunRecord(
                file: file,
                startedMs: Int64(Date().timeIntervalSince1970 * 1000 - (lastSeconds ?? 0) * 1000),
                seconds: lastSeconds ?? 0, ok: result.ok, timedOut: result.timedOut,
                output: console.plainText)
            // In the project it ran in, even if another is open now.
            do {
                try workspace.recordRun(project: startedIn, run: record)
            } catch {
                self.error = "The run finished but couldn't be added to History: \(Self.message(error))"
            }
            if project == startedIn { loadRuns() }
        }
        isDebugging = false
        debugPause = nil
        awaitingDebugCommand = false
        isRunning = false
    }

    // MARK: Debugging

    /// Adds a breakpoint on `line` of the open file, or removes the one
    /// there.
    public func toggleBreakpoint(line: Int) {
        guard let openFile, line > 0 else { return }
        let point = DebugBreakpoint(file: openFile, line: UInt32(line))
        if breakpoints.contains(point) {
            breakpoints.removeAll { $0 == point }
        } else {
            breakpoints.append(point)
        }
        saveProjectState()
    }

    /// Removes every breakpoint in the project.
    public func clearBreakpoints() {
        breakpoints = []
        saveProjectState()
    }

    public enum DebugCommand: String, Sendable {
        case `continue`
        /// Into a call on this line.
        case step
        /// Over this line.
        case next
        /// On to the caller.
        case out
    }

    /// Tells the paused debugger what to do next.
    @discardableResult
    public func debug(_ command: DebugCommand) -> Bool {
        guard let runtime, let runID, awaitingDebugCommand else { return false }
        awaitingDebugCommand = false
        return runtime.send(input: command.rawValue, id: runID)
    }

    /// The debugger paused: show where, in the editor.
    private func paused(_ json: String) {
        guard let pause = pythonDebugPause(json: json) else { return }
        debugPause = pause
        // Only in the project being debugged, not one opened since.
        if project == debugProject, let top = pause.stack.last {
            show(top)
        }
    }

    /// Reads the open project's run history again.
    public func loadRuns() {
        runs = (try? workspace?.runs(project: project)) ?? []
    }

    /// Forgets the open project's runs.
    public func clearRuns() {
        do {
            try workspace?.clearRuns(project: project)
        } catch {
            self.error = Self.message(error)
        }
        loadRuns()
    }

    /// Opens a traceback's file at its line.
    public func show(_ frame: TracebackFrame) {
        open(file: frame.file)
        if openFile == frame.file {
            lineRequest = PythonLineRequest(line: Int(frame.line))
        }
    }

    /// Shows what the run sent since the last batch.
    private func apply(_ events: [PythonConsoleEvent]) {
        for event in events {
            switch event {
            case .started:
                queuedBehindBonsai = false
            case .queuedBehindBonsai:
                queuedBehindBonsai = true
            case .output(let text):
                console.append(text, as: .output)
            case .error(let text):
                console.append(text, as: .error)
            case .debugPaused(let json):
                paused(json)
            case .waitingForInput:
                if debugPause != nil {
                    // The debugger reads its command, not the script.
                    awaitingDebugCommand = true
                } else {
                    waitingForInput = true
                }
                activity.labWaitingForInput = true
            case .inputEnded(let line):
                activity.labWaitingForInput = false
                if debugPause != nil {
                    // A command, not an answer: nothing for the console.
                    awaitingDebugCommand = false
                    debugPause = nil
                    if line == nil { isDebugging = false }
                    continue
                }
                waitingForInput = false
                if let line {
                    console.append(line + "\n", as: .input)
                }
            }
        }
    }

    /// Answers the running script's `input()` with `line` (its first line
    /// only, as typed into one field). False when nothing is waiting.
    @discardableResult
    public func sendInput(_ line: String) -> Bool {
        guard let runtime, let runID, waitingForInput else { return false }
        let first = line.split(separator: "\n", omittingEmptySubsequences: false).first ?? ""
        return runtime.send(input: String(first), id: runID)
    }

    /// Empties the console. While a script runs, its later output still
    /// shows.
    public func clearConsole() {
        console = PythonConsoleText()
        if !isRunning {
            succeeded = nil
            lastSeconds = nil
        }
    }

    /// Reads the project's outputs again (after a delete, or on opening).
    public func loadArtifacts() {
        guard let projectFolder else { return }
        artifacts = projectOutputs(project: projectFolder)
    }

    /// Where an artifact is, to show or share it.
    public func url(of artifact: ProjectArtifact) -> URL? {
        projectFolder.map { URL(fileURLWithPath: $0).appendingPathComponent(artifact.path) }
    }

    /// Where a project file is, to share it.
    public func url(of entry: WorkspaceEntry) -> URL? {
        projectFolder.map { URL(fileURLWithPath: $0).appendingPathComponent(entry.path) }
    }

    /// Stops this screen's running script (never Bonsai's).
    public func stop() {
        if let runID {
            runtime?.interrupt(id: runID)
        } else if isRunning {
            stopRequested = true
        }
    }

    // MARK: Helpers

    static func join(_ folder: String?, _ name: String) -> String {
        guard let folder, !folder.isEmpty else { return name }
        return "\(folder)/\(name)"
    }

    /// `path` after `from` was renamed to `to` (a file, or anything in a
    /// renamed folder).
    static func moved(_ path: String, from: String, to: String) -> String {
        if path == from { return to }
        if path.hasPrefix(from + "/") { return to + path.dropFirst(from.count) }
        return path
    }

    /// The words for a refusal (the same as Rust's `WorkspaceError`
    /// messages; UniFFI's own description is the case's name).
    nonisolated static func message(_ error: Error) -> String {
        guard let failure = error as? WorkspaceError else { return error.localizedDescription }
        switch failure {
        case .Invalid(let reason), .TooLarge(let reason), .Storage(let reason):
            return reason
        case .NotFound(let what, let name):
            return "There is no \(what) called \(name)."
        case .Exists(let name):
            return "\(name) already exists."
        case .Protected:
            return "The scratch project is Bonsai's default, so it can't be renamed or deleted."
        }
    }

    static let allSamples: [PythonLabSample] = [
        PythonLabSample(id: "hello", title: "Hello", code: """
            import sys, platform
            print("Hello from Python on this iPhone")
            print(sys.version)
            platform.platform()
            """),
        PythonLabSample(id: "modules", title: "Standard library check", code: """
            # Imports a module from each part of the standard library that
            # needs a compiled extension (a framework in the app).
            import importlib
            names = ["math", "json", "datetime", "decimal", "hashlib", "random",
                     "struct", "array", "sqlite3", "ssl", "zlib", "bz2", "lzma",
                     "csv", "pickle", "unicodedata", "zoneinfo", "ctypes",
                     "socket", "select", "xml.etree.ElementTree", "asyncio"]
            failed = []
            for name in names:
                try:
                    importlib.import_module(name)
                except Exception as error:
                    failed.append(f"{name}: {error}")
            print(f"{len(names) - len(failed)} of {len(names)} modules imported")
            print("\\n".join(failed) or "all of them")
            """),
        PythonLabSample(id: "primes", title: "Speed: primes", code: """
            import time
            start = time.perf_counter()
            limit = 200_000
            sieve = bytearray([1]) * (limit + 1)
            sieve[0:2] = b"\\x00\\x00"
            for i in range(2, int(limit ** 0.5) + 1):
                if sieve[i]:
                    sieve[i * i :: i] = bytes(len(range(i * i, limit + 1, i)))
            count = sum(sieve)
            print(f"{count} primes below {limit:,} in {time.perf_counter() - start:.3f} s")
            """),
        PythonLabSample(id: "sqlite", title: "SQLite", code: """
            import sqlite3
            db = sqlite3.connect(":memory:")
            db.execute("create table walks (day text, km real)")
            db.executemany("insert into walks values (?, ?)",
                           [("Mon", 3.2), ("Tue", 5.1), ("Wed", 2.4)])
            total, = db.execute("select sum(km) from walks").fetchone()
            print("SQLite", sqlite3.sqlite_version, "- total", round(total, 1), "km")
            """),
        PythonLabSample(id: "https", title: "HTTPS request", code: """
            import json, urllib.request
            with urllib.request.urlopen("https://www.python.org/api/v2/downloads/release/?is_latest=true", timeout=10) as response:
                releases = json.load(response)
            sorted(r["name"] for r in releases)[-3:]
            """),
        PythonLabSample(id: "loop", title: "Stop test (endless loop)", code: """
            # Runs until you tap Stop (or the time limit stops it).
            n = 0
            while True:
                n += 1
            """),
    ]
}
