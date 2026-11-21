@preconcurrency import ForgeFFI
import Foundation
#if canImport(PythonBridge)
import PythonBridge

/// The CPython interpreter embedded in Device Lab: Python.framework, with
/// the standard library in the app bundle's `python` folder (laid out by
/// scripts/prepare-python.sh). One interpreter for the app's lifetime;
/// scripts run one at a time, off the main thread.
final class PythonRuntime: PythonRunning {
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

    func run(_ code: String) async -> (ok: Bool, output: String) {
        await withCheckedContinuation { continuation in
            queue.async {
                var ok: Int32 = 0
                let output = pybridge_run(code, &ok)
                let text = output.map { String(cString: $0) } ?? ""
                pybridge_free(output)
                continuation.resume(returning: (ok == 1, text))
            }
        }
    }

    func interrupt() {
        // Not on `queue`: that is busy running the script.
        DispatchQueue.global().async { pybridge_interrupt() }
    }
}

extension PythonLabModel {
    /// Scripts run in the harness's `scratch` project, so the images they
    /// save show under Artifacts and Bonsai's own tasks see the same files.
    static func live() -> PythonLabModel {
        PythonLabModel(
            runtime: PythonRuntime.shared,
            projectFolder: PythonLabProjects.folder.appendingPathComponent("scratch").path)
    }
}

/// Where Python Lab's projects live (Bonsai Harness's too).
enum PythonLabProjects {
    static let folder = URL.applicationSupportDirectory
        .appendingPathComponent("Python Lab/Projects", isDirectory: true)
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
        let result = await run(code)
        return PythonOutcome(ok: result.ok, output: result.output)
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
