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
    /// Runs a script; `ok` is false when it ended with an exception.
    func run(_ code: String) async -> (ok: Bool, output: String)
    /// Stops the running script (a KeyboardInterrupt in it).
    func interrupt()
}

public struct PythonLabSample: Identifiable, Sendable {
    public let id: String
    public let title: String
    public let code: String
}

@MainActor
@Observable
public final class PythonLabModel {
    public var code: String
    public private(set) var output = ""
    /// Whether the last run finished without an exception (nil before one).
    public private(set) var succeeded: Bool?
    public private(set) var isRunning = false
    public private(set) var lastSeconds: Double?
    public private(set) var version: String?
    /// Why Python isn't available (not in this build, or it failed to start).
    public private(set) var unavailable: String?
    /// A script running longer than this is stopped.
    public let timeLimit: Duration
    /// The project folder scripts run in (the harness's `scratch` project,
    /// shared with Bonsai's own tasks), so a script that saves `plot.png`
    /// has somewhere to put it. Nil runs the bare script, as before.
    public let projectFolder: String?
    /// The project's images, newest first.
    public private(set) var artifacts: [ProjectArtifact] = []
    /// The images the last run made or changed.
    public private(set) var madeByLastRun: [ProjectArtifact] = []

    public let samples: [PythonLabSample]

    @ObservationIgnored private let runtime: (any PythonRunning)?
    @ObservationIgnored private var started = false

    public init(
        runtime: (any PythonRunning)?, projectFolder: String? = nil,
        timeLimit: Duration = .seconds(30)
    ) {
        self.runtime = runtime
        self.timeLimit = timeLimit
        self.projectFolder = projectFolder
        // The graphics samples write files, so they need the project.
        samples =
            projectFolder == nil
            ? Self.allSamples
            : Self.allSamples
                + pythonLabGraphicsSamples().map {
                    PythonLabSample(id: $0.id, title: $0.title, code: $0.code)
                }
        code = Self.allSamples[0].code
        if runtime == nil {
            unavailable = "This build of Device Lab has no Python."
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

    /// Whether this build has an interpreter at all.
    public var hasPython: Bool { runtime != nil }

    public func load(_ sample: PythonLabSample) {
        code = sample.code
    }

    public func run() async {
        guard let runtime, !isRunning else { return }
        await prepare()
        guard started else { return }
        isRunning = true
        output = ""
        succeeded = nil
        let limit = timeLimit
        let watchdog = Task {
            try? await Task.sleep(for: limit)
            if !Task.isCancelled { runtime.interrupt() }
        }
        var script = code
        var before: [String: ProjectArtifact] = [:]
        if let projectFolder {
            try? FileManager.default.createDirectory(
                atPath: projectFolder, withIntermediateDirectories: true)
            script = pythonLabScript(project: projectFolder, code: code)
            before = Dictionary(
                projectArtifacts(project: projectFolder).map { ($0.path, $0) },
                uniquingKeysWith: { first, _ in first })
        }
        let clock = ContinuousClock()
        let begin = clock.now
        var result = await runtime.run(script)
        watchdog.cancel()
        let elapsed = clock.now - begin
        if let projectFolder {
            let readable = pythonLabOutput(project: projectFolder, ok: result.ok, output: result.output)
            result = (readable.ok, readable.output)
            artifacts = projectArtifacts(project: projectFolder)
            madeByLastRun = artifacts.filter { artifact in
                guard let old = before[artifact.path] else { return true }
                return old.bytes != artifact.bytes || old.modifiedMs != artifact.modifiedMs
            }
        }
        lastSeconds = Double(elapsed.components.seconds)
            + Double(elapsed.components.attoseconds) / 1e18
        output = result.output.isEmpty ? "(no output)" : result.output
        succeeded = result.ok
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

    public func stop() {
        runtime?.interrupt()
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
