@preconcurrency import ForgeFFI
import Foundation
#if canImport(PythonBridge)
import PythonBridge

/// The CPython interpreter embedded in Device Lab: Python.framework, with
/// the standard library in the app bundle's `python` folder (laid out by
/// scripts/prepare-python.sh). One interpreter for the app's lifetime;
/// scripts run one at a time, off the main thread. Its mutable state is
/// behind `lock`.
final class PythonRuntime: PythonRunning, @unchecked Sendable {
    struct StartError: LocalizedError {
        let message: String
        var errorDescription: String? { message }
    }

    /// One interpreter for the app: Python Lab and Bonsai Harness share it,
    /// so their scripts take turns on `queue`.
    static let shared = PythonRuntime()

    private let queue = DispatchQueue(label: "org.forgemobile.devicelab.python")

    func start() async throws -> String {
        let home = Bundle.main.bundleURL.appendingPathComponent("python").path
        // Packages a script installs or saves (pure Python only) go here.
        let site = URL.applicationSupportDirectory
            .appendingPathComponent("Python/site-packages", isDirectory: true)
        try? FileManager.default.createDirectory(at: site, withIntermediateDirectories: true)
        return try await withCheckedThrowingContinuation { continuation in
            queue.async {
                if let error = pybridge_start(home, site.path) {
                    let message = String(cString: error)
                    pybridge_free(error)
                    continuation.resume(throwing: StartError(message: message))
                    return
                }
                let version = pybridge_version()
                let text = version.map { String(cString: $0) } ?? "?"
                pybridge_free(version)
                continuation.resume(returning: text)
            }
        }
    }

    /// The run on the interpreter now, so an interrupt reaches only the
    /// script it was meant for: Python Lab and Bonsai Harness take turns on
    /// `queue`, and a Stop or a time limit for one must never stop the
    /// other's script (#367 review).
    private let lock = NSLock()
    private var active: UUID?
    /// Bonsai Harness's current run, for its `interrupt()`.
    private var harnessRun: UUID?

    /// Runs `code` once it has the interpreter. `limit`, if given, counts
    /// from then, not while it waited for its turn.
    private func run(_ code: String, id: UUID, limit: Duration?) async -> PythonRunResult {
        await withCheckedContinuation { continuation in
            queue.async { [self] in
                lock.withLock { active = id }
                let timedOut = TimedOut()
                var watchdog: DispatchWorkItem?
                if let limit {
                    let item = DispatchWorkItem { [self] in
                        lock.withLock {
                            if active == id {
                                timedOut.value = true
                                pybridge_interrupt()
                            }
                        }
                    }
                    let seconds = Double(limit.components.seconds)
                        + Double(limit.components.attoseconds) / 1e18
                    DispatchQueue.global().asyncAfter(deadline: .now() + seconds, execute: item)
                    watchdog = item
                }
                var ok: Int32 = 0
                let output = pybridge_run(code, &ok)
                watchdog?.cancel()
                let wasTimedOut = lock.withLock {
                    active = nil
                    return timedOut.value
                }
                let text = output.map { String(cString: $0) } ?? ""
                pybridge_free(output)
                continuation.resume(
                    returning: PythonRunResult(ok: ok == 1, output: text, timedOut: wasTimedOut))
            }
        }
    }

    func run(_ code: String, id: UUID, limit: Duration) async -> PythonRunResult {
        await run(code, id: id, limit: Optional(limit))
    }

    func interrupt(id: UUID) {
        // Not on `queue`: that is busy running the script.
        DispatchQueue.global().async { [self] in
            lock.withLock {
                if active == id { pybridge_interrupt() }
            }
        }
    }
}

/// Set by the watchdog under `PythonRuntime.lock`.
private final class TimedOut: @unchecked Sendable {
    var value = false
}

extension PythonLabModel {
    /// Projects in the same folder as Bonsai Harness's, so a file either
    /// of them writes is the other's to read.
    static func live() -> PythonLabModel {
        PythonLabModel(
            runtime: PythonRuntime.shared,
            workspace: try? Workspace(projectsFolder: PythonLabProjects.folder.path))
    }
}

/// Where Python Lab's projects live (Bonsai Harness's too): Documents,
/// which Device Lab shows in the Files app (On My iPhone > Device Lab >
/// Python Lab), so every project is a normal folder there.
enum PythonLabProjects {
    static let folder: URL = {
        let folder = URL.documentsDirectory
            .appendingPathComponent("Python Lab", isDirectory: true)
        // Builds before the workspace kept them where only the app could
        // see them; move them once.
        let old = URL.applicationSupportDirectory
            .appendingPathComponent("Python Lab/Projects", isDirectory: true)
        _ = try? workspaceMoveProjects(old: old.path, new: folder.path)
        return folder
    }()
}

/// Bonsai Harness runs its wrapper scripts through the same interpreter
/// (rust/src/harness/python_lab.rs writes them).
extension PythonRuntime: PythonHost {
    func run(code: String) async throws -> PythonOutcome {
        do {
            _ = try await start()
        } catch {
            throw PythonHostError.Unavailable(reason: error.localizedDescription)
        }
        // The harness keeps its own time limit (python_lab.rs) and stops a
        // run with `interrupt()`, which reaches only this run.
        let id = UUID()
        lock.withLock { harnessRun = id }
        let result = await run(code, id: id, limit: nil)
        lock.withLock { if harnessRun == id { harnessRun = nil } }
        return PythonOutcome(ok: result.ok, output: result.output)
    }

    func interrupt() {
        if let id = lock.withLock({ harnessRun }) { interrupt(id: id) }
    }
}

extension BonsaiHarnessModel {
    static func live() -> BonsaiHarnessModel {
        let projects = PythonLabProjects.folder
        try? FileManager.default.createDirectory(at: projects, withIntermediateDirectories: true)
        return BonsaiHarnessModel(
            harness: BonsaiHarness(python: PythonRuntime.shared, projectsFolder: projects.path))
    }
}
#else
extension PythonLabModel {
    static func live() -> PythonLabModel { PythonLabModel(runtime: nil) }
}

extension BonsaiHarnessModel {
    static func live() -> BonsaiHarnessModel { BonsaiHarnessModel(harness: nil) }
}
#endif
