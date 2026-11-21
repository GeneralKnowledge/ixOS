@preconcurrency import ForgeFFI
import Foundation
import Observation

/// The llama.cpp models screen: which GGUF files are on the phone, their
/// downloads, and one AI Lab per model (AiLabLlama.swift runs them).
///
/// Downloads are ordinary foreground `URLSession` downloads: keep Device Lab
/// open until one finishes (1.2 GB for the 4B model).
@MainActor
@Observable
final class LlamaLabModel {
    enum FileState: Equatable {
        case missing
        case downloading(Double)
        case ready
        case failed(String)
    }

    let files = LlamaModelFile.catalogue
    /// The backends llama.cpp found ("MTL0, BLAS, CPU" on an iPhone).
    let backends: String
    private(set) var states: [String: FileState] = [:]
    /// Space this app may still use (iOS's "important usage" figure, which
    /// can be far below what Settings shows).
    private(set) var freeBytes: Int64?
    /// What iOS lets the app allocate now.
    private(set) var availableMemory: UInt64?
    private(set) var lastLoadSeconds: Double?

    @ObservationIgnored private var tasks: [String: URLSessionDownloadTask] = [:]
    @ObservationIgnored private var labs: [String: AiLabModel] = [:]

    init() {
        backends = LlamaProbe.describe()
        refresh()
    }

    func state(_ file: LlamaModelFile) -> FileState { states[file.id] ?? .missing }

    func refresh() {
        for file in files where tasks[file.id] == nil {
            if file.isDownloaded {
                states[file.id] = .ready
            } else if case .failed = states[file.id] {
                // Keep the error on screen until the next try.
            } else {
                states[file.id] = .missing
            }
        }
        freeBytes = Self.freeSpace()
        availableMemory = LlamaRuntime.availableMemory()
        lastLoadSeconds = LlamaRuntime.shared.lastLoadSeconds
    }

    /// AI Lab running this model (made once per model).
    func lab(for file: LlamaModelFile) -> AiLabModel {
        if let lab = labs[file.id] { return lab }
        let lab = AiLabModel(lab: AiLab(model: LlamaCppModel(file: file)), structured: nil)
        labs[file.id] = lab
        return lab
    }

    func download(_ file: LlamaModelFile) {
        guard tasks[file.id] == nil, let destination = file.localURL else { return }
        refresh()
        let headroom: Int64 = 200_000_000
        if let free = freeBytes, free < file.bytes + headroom {
            states[file.id] = .failed(
                "Needs about \(Format.storage(UInt64(file.bytes + headroom))) free; this app can use \(Format.storage(UInt64(max(free, 0)))). Free some space and try again."
            )
            return
        }
        let id = file.id
        let task = URLSession.shared.downloadTask(with: file.url) { temporary, response, error in
            // Runs off the main thread; the file must move before it returns.
            var failure: String?
            var cancelled = false
            if let error {
                cancelled = (error as NSError).code == NSURLErrorCancelled
                failure = cancelled ? nil : error.localizedDescription
            } else if let http = response as? HTTPURLResponse, http.statusCode != 200 {
                failure = "The server answered \(http.statusCode)."
            } else if let temporary {
                do {
                    try? FileManager.default.removeItem(at: destination)
                    try FileManager.default.moveItem(at: temporary, to: destination)
                } catch {
                    failure = "Could not save the model: \(error.localizedDescription)"
                }
            }
            Task { @MainActor [weak self] in
                self?.finished(id, failure: failure, cancelled: cancelled)
            }
        }
        tasks[id] = task
        states[id] = .downloading(0)
        task.resume()
        let started = ObjectIdentifier(task)
        Task { [weak self] in
            while let self, let running = self.tasks[id], ObjectIdentifier(running) == started {
                self.states[id] = .downloading(running.progress.fractionCompleted)
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
    }

    func cancel(_ file: LlamaModelFile) {
        tasks[file.id]?.cancel()
    }

    func remove(_ file: LlamaModelFile) {
        guard let url = file.localURL else { return }
        LlamaRuntime.shared.unload(path: url.path)
        try? FileManager.default.removeItem(at: url)
        states[file.id] = .missing
        refresh()
    }

    private func finished(_ id: String, failure: String?, cancelled: Bool) {
        tasks[id] = nil
        if let failure {
            states[id] = .failed(failure)
        } else if cancelled {
            states[id] = .missing
        }
        refresh()
    }

    private static func freeSpace() -> Int64? {
        guard let folder = try? LlamaModelFile.folder,
            let values = try? folder.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
        else { return nil }
        return values.volumeAvailableCapacityForImportantUsage
    }
}
