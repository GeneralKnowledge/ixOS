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
    /// Runs handed to `queue` and not finished.
    private var waiting: Set<UUID> = []
    /// Runs stopped while they waited for their turn: they don't start.
    private var stoppedEarly: Set<UUID> = []
    /// Bonsai Harness's current run, for its `interrupt()`.
    private var harnessRun: UUID?

    /// Runs `execute` once run `id` has the interpreter, returning what it
    /// returns. `limit`, if given, counts the time `clock` says the run
    /// spent running: from when it starts, not while it waited for its
    /// turn or (for Python Lab's console) for an answer.
    private func run(
        id: UUID, limit: Duration?, clock: RunClock = RunClock(),
        queued: (@Sendable () -> Void)? = nil, started: (@Sendable () -> Void)? = nil,
        execute: @escaping @Sendable () -> (ok: Bool, output: String)
    ) async -> PythonRunResult {
        let behindBonsai = lock.withLock {
            waiting.insert(id)
            return active != nil && active == harnessRun
        }
        if behindBonsai { queued?() }
        defer {
            lock.withLock {
                waiting.remove(id)
                stoppedEarly.remove(id)
            }
        }
        return await withCheckedContinuation { continuation in
            queue.async { [self] in
                let stopped = lock.withLock {
                    active = id
                    return stoppedEarly.remove(id) != nil
                }
                if stopped {
                    lock.withLock { active = nil }
                    continuation.resume(returning: PythonRunResult(
                        ok: false, output: "Stopped before it started.",
                        timedOut: false))
                    return
                }
                started?()
                clock.begin()
                let timedOut = TimedOut()
                var watchdog: DispatchSourceTimer?
                if let limit {
                    // Checked ten times a second, so time spent waiting for
                    // an answer can be left out.
                    let timer = DispatchSource.makeTimerSource(queue: .global())
                    timer.schedule(deadline: .now() + .milliseconds(100), repeating: .milliseconds(100))
                    timer.setEventHandler { [self] in
                        guard clock.running() >= limit else { return }
                        timer.cancel()
                        lock.withLock {
                            // Only a script still running counts as timed
                            // out, not one that returned a moment before.
                            if active == id, pybridge_interrupt() == 1 {
                                timedOut.value = true
                            }
                        }
                    }
                    timer.resume()
                    watchdog = timer
                }
                let (ok, output) = execute()
                watchdog?.cancel()
                let wasTimedOut = lock.withLock {
                    active = nil
                    return timedOut.value
                }
                continuation.resume(
                    returning: PythonRunResult(ok: ok, output: output, timedOut: wasTimedOut))
            }
        }
    }

    /// A Python Lab run: output streams to `events` as the script writes
    /// it, and `input()` waits for `send(input:id:)`.
    func run(
        _ code: String, id: UUID, limit: Duration,
        events: @escaping @Sendable (PythonConsoleEvent) -> Void
    ) async -> PythonRunResult {
        let clock = RunClock()
        let sink = StreamSink(events: events, clock: clock)
        let result = await run(
            id: id, limit: limit, clock: clock,
            queued: { events(.queuedBehindBonsai) },
            started: { events(.started) }
        ) {
            let context = Unmanaged.passRetained(sink)
            defer { context.release() }
            // A C function can't capture: everything it needs comes
            // through `context`.
            let ok = pybridge_run_streaming(code, 1, { context, kind, text, length in
                guard let context else { return }
                let sink = Unmanaged<StreamSink>.fromOpaque(context).takeUnretainedValue()
                let string = text.map {
                    String(decoding: UnsafeRawBufferPointer(start: $0, count: Int(length)), as: UTF8.self)
                } ?? ""
                sink.receive(kind: Int(kind), text: string)
            }, context.toOpaque())
            return (ok == 1, "")
        }
        return result
    }

    func send(input line: String, id: UUID) -> Bool {
        guard lock.withLock({ active == id }) else { return false }
        return pybridge_send_input(line) == 1
    }

    /// Stops run `id`: now if it is running, or as soon as its turn comes
    /// if it is waiting behind another run (which it never touches).
    func interrupt(id: UUID) {
        // Not on `queue`: that is busy running the script.
        DispatchQueue.global().async { [self] in
            lock.withLock {
                if active == id {
                    _ = pybridge_interrupt()
                } else if waiting.contains(id) {
                    stoppedEarly.insert(id)
                }
            }
        }
    }
}

/// Set by the watchdog under `PythonRuntime.lock`.
private final class TimedOut: @unchecked Sendable {
    var value = false
}

/// How long a run has spent running: from `begin()`, less the time it
/// waited for an answer to `input()`. Its own lock, never
/// `PythonRuntime.lock`: it is updated from the bridge's callback, which
/// holds Python's GIL, and the watchdog holds `PythonRuntime.lock` while
/// it waits for the GIL.
final class RunClock: @unchecked Sendable {
    private let lock = NSLock()
    private let clock = ContinuousClock()
    private var began: ContinuousClock.Instant?
    private var waited: Duration = .zero
    private var waitBegan: ContinuousClock.Instant?

    func begin() {
        lock.withLock { began = clock.now }
    }

    func pause() {
        lock.withLock { if waitBegan == nil { waitBegan = clock.now } }
    }

    func resume() {
        lock.withLock {
            if let waitBegan {
                waited += clock.now - waitBegan
                self.waitBegan = nil
            }
        }
    }

    /// Time spent running; while waiting, as it was when the wait began.
    func running() -> Duration {
        lock.withLock {
            guard let began else { return .zero }
            return (waitBegan ?? clock.now) - began - waited
        }
    }
}

/// What the bridge's callback reaches through its context pointer.
private final class StreamSink: @unchecked Sendable {
    let events: @Sendable (PythonConsoleEvent) -> Void
    let clock: RunClock

    init(events: @escaping @Sendable (PythonConsoleEvent) -> Void, clock: RunClock) {
        self.events = events
        self.clock = clock
    }

    /// `kind` is PythonBridge.h's PYBRIDGE_STDOUT (1), PYBRIDGE_STDERR (2),
    /// PYBRIDGE_INPUT_WAITING (3), PYBRIDGE_INPUT_ENDED (4) or
    /// PYBRIDGE_INPUT_CANCELLED (5).
    func receive(kind: Int, text: String) {
        switch kind {
        case 1: events(.output(text))
        case 2: events(.error(text))
        case 3:
            clock.pause()
            events(.waitingForInput)
        case 4:
            clock.resume()
            events(.inputEnded(text))
        case 5:
            clock.resume()
            events(.inputEnded(nil))
        default: break
        }
    }
}

extension PythonLabModel {
    /// Projects in the same folder as Bonsai Harness's, so a file either
    /// of them writes is the other's to read.
    static func live() -> PythonLabModel {
        let model = PythonLabModel(
            runtime: PythonRuntime.shared,
            workspace: try? Workspace(projectsFolder: PythonLabProjects.folder.path))
        if let reason = PythonLabProjects.moveFailed {
            model.error = "Your projects couldn't move to the Files app (\(reason)), so they stay inside the app for now. Device Lab tries again next time it opens."
        }
        return model
    }
}

/// Where Python Lab's projects live (Bonsai Harness's too): Documents,
/// which Device Lab shows in the Files app (On My iPhone > Device Lab >
/// Python Lab), so every project is a normal folder there.
enum PythonLabProjects {
    /// Why the projects couldn't move to Files this launch, if they couldn't.
    nonisolated(unsafe) static var moveFailed: String?

    static let folder: URL = {
        let folder = URL.documentsDirectory
            .appendingPathComponent("Python Lab", isDirectory: true)
        // Builds before the workspace kept them where only the app could
        // see them; move them once.
        let old = URL.applicationSupportDirectory
            .appendingPathComponent("Python Lab/Projects", isDirectory: true)
        do {
            let moved = try workspaceMoveProjects(old: old.path, new: folder.path)
            if !moved, FileManager.default.fileExists(atPath: old.path) {
                // Both places hold projects: the move never overwrites, so
                // the old ones stay where they were. Say so rather than let
                // them vanish from view.
                PythonLabProjects.moveFailed = "the Python Lab folder in Files already has projects, so the older ones are still in the app's own storage"
            }
        } catch {
            // Keep working where the projects are, so none of them
            // disappears; the move is tried again on the next launch.
            PythonLabProjects.moveFailed = PythonLabModel.message(error)
            return old
        }
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
        let result = await run(id: id, limit: nil) {
            var ok: Int32 = 0
            let output = pybridge_run(code, &ok)
            let text = output.map { String(cString: $0) } ?? ""
            pybridge_free(output)
            return (ok == 1, text)
        }
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
            harness: BonsaiHarness(python: PythonRuntime.shared, projectsFolder: projects.path),
            workspace: try? Workspace(projectsFolder: projects.path))
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
