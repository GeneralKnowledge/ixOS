@preconcurrency import ForgeFFI
import Foundation
import Observation

/// The Bonsai Harness screen's view of Rust's `BonsaiHarness`
/// (rust/src/harness): Rust runs the task, owns its status, checks the
/// evidence and keeps the journal; this copies the snapshot for the view and
/// passes taps on. The model is a downloaded Bonsai GGUF running through
/// llama.cpp (`LlamaCppModel`, which follows the harness's grammar).
@MainActor
@Observable
public final class BonsaiHarnessModel {
    public private(set) var snapshot: HarnessSnapshot?
    public private(set) var ladder: [HarnessTask] = []
    public private(set) var files: [ProjectFile] = []
    public private(set) var error: String?
    /// The person's own task.
    public var request = ""
    /// Which model runs the task (a `LlamaModelFile` id).
    public var modelID: String = LlamaModelFile.catalogue[0].id

    @ObservationIgnored private let harness: BonsaiHarness?
    @ObservationIgnored private var models: [String: LlamaCppModel] = [:]

    public init(harness: BonsaiHarness?) {
        self.harness = harness
        if let harness {
            ladder = harness.ladder()
            snapshot = harness.snapshot()
        } else {
            error = "This build of Device Lab has no Python, so Bonsai Harness can't run."
        }
    }

    public var isAvailable: Bool { harness != nil }
    public var isRunning: Bool { snapshot?.running ?? false }

    /// The models on this phone, for the picker.
    var downloadedModels: [LlamaModelFile] {
        LlamaModelFile.catalogue.filter(\.isDownloaded)
    }

    /// Copies Rust's snapshot four times a second until the calling task is
    /// cancelled (attach with `.task`).
    public func follow() async {
        if let first = downloadedModels.first, !downloadedModels.contains(where: { $0.id == modelID }) {
            modelID = first.id
        }
        if snapshot?.task == nil, let harness {
            // Show the last task of the scratch project after a relaunch
            // (a task the app closed during is marked stopped).
            try? harness.openProject(name: "scratch")
        }
        var wasRunning = isRunning
        while !Task.isCancelled {
            refresh()
            if wasRunning && !isRunning { loadFiles() }
            wasRunning = isRunning
            try? await Task.sleep(for: .milliseconds(250))
        }
    }

    public func refresh() {
        guard let harness else { return }
        let next = harness.snapshot()
        if next != snapshot { snapshot = next }
    }

    public func run(_ task: HarnessTask) {
        perform { harness, model in
            try harness.startLadder(model: model, taskId: task.id)
        }
    }

    public func runRequest() {
        perform { [request] harness, model in
            try harness.startCustom(model: model, request: request)
        }
    }

    public func stop() {
        harness?.stop()
        refresh()
    }

    /// The last task as Markdown, to share as evidence.
    public var transcript: String { harness?.transcript() ?? "" }

    public func loadFiles() {
        files = harness?.files() ?? []
    }

    private func perform(_ start: (BonsaiHarness, LlamaCppModel) throws -> Void) {
        guard let harness else { return }
        guard let file = LlamaModelFile.catalogue.first(where: { $0.id == modelID }), file.isDownloaded
        else {
            error = "Download a Bonsai model first (Tools, Bonsai on iPhone)."
            return
        }
        let model = models[file.id] ?? LlamaCppModel(file: file)
        models[file.id] = model
        do {
            try start(harness, model)
            error = nil
        } catch let failure as HarnessError {
            error = failure.message
        } catch {
            self.error = "\(error)"
        }
        refresh()
        loadFiles()
    }
}

extension HarnessError {
    var message: String {
        switch self {
        case .Busy: "A task is still running. Stop it first."
        case .EmptyTask: "Describe the task first."
        case .UnknownTask(let id): "There is no task called \(id)."
        case .Storage(let reason): reason
        }
    }
}

extension TaskStatus {
    var title: String {
        switch self {
        case .ready: "Ready"
        case .running: "Running"
        case .verifying: "Checking the evidence"
        case .completed: "Completed"
        case .failed: "Failed"
        case .stopped: "Stopped"
        }
    }
}
