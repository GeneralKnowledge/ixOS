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
    /// Runs a script as the run `id`. The interpreter is shared with Bonsai
    /// Harness, so the script may first wait for its turn; `limit` counts
    /// only from when it starts running. `timedOut` when the limit stopped
    /// it.
    func run(_ code: String, id: UUID, limit: Duration) async -> PythonRunResult
    /// Stops the run `id` (a KeyboardInterrupt in it) if it is the one
    /// running; never another caller's script.
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
            guard newValue != text else { return }
            text = newValue
            saveState = .unsaved
            scheduleSave()
        }
    }
    public private(set) var saveState: SaveState = .saved
    public private(set) var output = ""
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
    /// The file Run starts; nil runs the open file.
    public private(set) var runFile: String?

    /// The project's images, newest first.
    public private(set) var artifacts: [ProjectArtifact] = []
    /// The images the last run made or changed.
    public private(set) var madeByLastRun: [ProjectArtifact] = []

    public let samples: [PythonLabSample]

    @ObservationIgnored private let runtime: (any PythonRunning)?
    @ObservationIgnored private let workspace: Workspace?
    @ObservationIgnored private var started = false
    @ObservationIgnored private var pendingSave: Task<Void, Never>?
    @ObservationIgnored private var runID: UUID?
    /// Remembers the last project between launches.
    @ObservationIgnored private let defaults: UserDefaults
    static let lastProjectKey = "pythonLab.lastProject"

    /// `workspace` nil keeps the code in memory only (a build or test with
    /// no projects folder).
    public init(
        runtime: (any PythonRunning)?, workspace: Workspace? = nil,
        timeLimit: Duration = .seconds(30), defaults: UserDefaults = .standard
    ) {
        self.runtime = runtime
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
        // The new project may have a file of the same name.
        openFile = nil
        reloadProjects()
        let target = projects.contains { $0.name == name } ? name : "scratch"
        project = target
        defaults.set(target, forKey: Self.lastProjectKey)
        let state = (try? workspace.state(project: target)) ?? WorkspaceState(
            runFile: nil, currentFile: nil, openFiles: [])
        runFile = state.runFile
        // A project whose files can't be listed is not empty: don't start
        // a main.py in it.
        guard reloadEntries() else { return }
        if let current = state.currentFile {
            open(file: current)
        } else if let first = entries.first(where: { !$0.isFolder && $0.path.hasSuffix(".py") }) {
            open(file: first.path)
        } else {
            createFirstFile()
        }
        loadArtifacts()
        madeByLastRun = []
        succeeded = nil
        output = ""
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
            return true
        } catch {
            entries = []
            self.error = "Couldn't list the files in \(project): \(Self.message(error))"
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
            saveState = .saved
            error = nil
            saveProjectState()
        } catch {
            self.error = Self.message(error)
        }
    }

    /// A new file in the project (in `folder`, if given), opened.
    public func createFile(_ name: String, in folder: String? = nil, text: String = "") -> String? {
        guard let workspace else { return nil }
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
        let parent = entry.path.split(separator: "/").dropLast().joined(separator: "/")
        let target = Self.join(parent.isEmpty ? nil : parent, name)
        flush()
        do {
            try workspace.renameEntry(project: project, from: entry.path, to: target)
        } catch {
            return Self.message(error)
        }
        openFile = openFile.map { Self.moved($0, from: entry.path, to: target) }
        runFile = runFile.map { Self.moved($0, from: entry.path, to: target) }
        saveProjectState()
        reloadEntries()
        return nil
    }

    public func delete(_ entry: WorkspaceEntry) {
        guard let workspace else { return }
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
        if inside(openFile) {
            openFile = nil
            if let first = entries.first(where: { !$0.isFolder && $0.path.hasSuffix(".py") }) {
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
        do {
            try workspace.writeText(project: project, path: openFile, text: code)
            saveState = .saved
        } catch {
            saveState = .failed(Self.message(error))
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
            runFile: runFile, currentFile: openFile, openFiles: openFile.map { [$0] } ?? [])
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

    public func run() async {
        guard let runtime, !isRunning else { return }
        // Taken before the first wait, so a second tap can't start a
        // second run while Python starts.
        isRunning = true
        await prepare()
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
        output = ""
        succeeded = nil
        error = nil
        let id = UUID()
        runID = id
        var script = code
        var before: [String: ProjectArtifact] = [:]
        let startedIn = project
        let folder = projectFolder
        let file = fileToRun
        if let folder, let file {
            script = pythonLabFileScript(project: folder, path: file)
            before = Dictionary(
                projectArtifacts(project: folder).map { ($0.path, $0) },
                uniquingKeysWith: { first, _ in first })
        }
        lastRunFile = file
        let clock = ContinuousClock()
        let begin = clock.now
        let result = await runtime.run(script, id: id, limit: timeLimit)
        let elapsed = clock.now - begin
        runID = nil
        var ok = result.ok
        var text = result.output
        if let folder {
            let readable = pythonLabOutput(project: folder, ok: result.ok, output: result.output)
            ok = readable.ok
            text = readable.output
        }
        // A run whose project is no longer open shows its output, but its
        // images and file list belong to that project, not this one.
        if let folder, project == startedIn {
            artifacts = projectArtifacts(project: folder)
            madeByLastRun = artifacts.filter { artifact in
                guard let old = before[artifact.path] else { return true }
                return old.bytes != artifact.bytes || old.modifiedMs != artifact.modifiedMs
            }
            reloadEntries()
        }
        if result.timedOut {
            text += "\n\nStopped after the \(timeLimit.components.seconds) s limit."
        }
        lastSeconds = Double(elapsed.components.seconds)
            + Double(elapsed.components.attoseconds) / 1e18
        output = text.isEmpty ? "(no output)" : text
        succeeded = ok
        isRunning = false
    }

    /// Reads the project's images again (after a delete, or on opening).
    public func loadArtifacts() {
        guard let projectFolder else { return }
        artifacts = projectArtifacts(project: projectFolder)
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
        if let runID { runtime?.interrupt(id: runID) }
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
