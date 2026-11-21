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
    static func live() -> PythonLabModel { PythonLabModel(runtime: PythonRuntime()) }
}
#else
extension PythonLabModel {
    static func live() -> PythonLabModel { PythonLabModel(runtime: nil) }
}
#endif
