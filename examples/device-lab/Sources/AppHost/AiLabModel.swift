@preconcurrency import ForgeFFI
import Foundation
import Observation

/// The AI Lab screen's view of Rust's `AiLab` (rust/src/ai_lab.rs). Rust
/// runs the model, times each run and keeps the history; this copies the
/// snapshot for the view and passes taps on.
///
/// Structured output is the one run Swift does itself (Foundation Models'
/// guided generation has no Rust shape): `structured` is that run, and it
/// reports to Rust as it goes, so the run gets the same record.
@MainActor
@Observable
public final class AiLabModel {
    /// Runs a structured experiment of `kind`: given the instructions and
    /// the prompt, calls `update` with the answer so far and returns the
    /// whole answer. Throws `AiModelError` (`.Refused` when the model
    /// declined).
    public typealias StructuredRun =
        @Sendable (
            _ kind: AiExperimentKind, _ instructions: String, _ prompt: String,
            _ update: @escaping @Sendable (String) -> Void
        ) async throws -> String

    public private(set) var snapshot: AiLabSnapshot
    public let experiments: [AiExperiment] = aiLabExperiments()
    public private(set) var error: String?
    /// The user's own prompt.
    public var customPrompt = ""

    @ObservationIgnored private let lab: AiLab
    @ObservationIgnored private let structured: StructuredRun?
    @ObservationIgnored private var structuredTask: Task<Void, Never>?

    public init(lab: AiLab, structured: StructuredRun?) {
        self.lab = lab
        self.structured = structured
        snapshot = lab.snapshot()
    }

    public var isRunning: Bool { snapshot.running != nil }

    /// Copies Rust's snapshot ten times a second until the calling task is
    /// cancelled (attach with `.task`).
    public func follow() async {
        refreshModel()
        while !Task.isCancelled {
            refresh()
            try? await Task.sleep(for: .milliseconds(100))
        }
    }

    public func refresh() {
        let next = lab.snapshot()
        if next != snapshot { snapshot = next }
    }

    /// Asks the model again whether it can answer.
    public func refreshModel() {
        _ = lab.refreshModel()
        refresh()
    }

    public func run(_ experiment: AiExperiment) {
        if experiment.kind == .structured || experiment.kind == .structuredSchema {
            runStructured(experiment)
            return
        }
        perform {
            _ = try lab.run(
                kind: experiment.kind, instructions: experiment.instructions,
                prompt: experiment.prompt, maxTokens: experiment.maxTokens,
                temperature: experiment.temperature)
        }
    }

    public func runCustom() {
        perform {
            _ = try lab.run(
                kind: .custom, instructions: "", prompt: customPrompt, maxTokens: 600,
                temperature: 0.7)
        }
    }

    public func stop() {
        lab.stop()
        structuredTask?.cancel()
        refresh()
    }

    public func clearHistory() {
        lab.clearHistory()
        refresh()
    }

    private func runStructured(_ experiment: AiExperiment) {
        guard let structured else {
            error = "Structured output needs iOS 26 with Apple Intelligence."
            return
        }
        perform { _ = try lab.beginExternal(kind: experiment.kind, prompt: experiment.prompt) }
        guard error == nil else { return }
        let lab = lab
        structuredTask = Task { [weak self] in
            // nil keeps the partial answer Rust already has (Stop, a failure).
            var answer: String?
            var failure: String?
            var refused = false
            do {
                answer = try await structured(
                    experiment.kind, experiment.instructions, experiment.prompt
                ) {
                    lab.updateExternal(soFar: $0)
                }
            } catch AiModelError.Refused(let reason) {
                failure = reason
                refused = true
            } catch is CancellationError {
                // Stopped: Rust already knows.
            } catch {
                failure = "\(error)"
            }
            try? lab.finishExternal(answer: answer, error: failure, refused: refused)
            self?.refresh()
        }
    }

    private func perform(_ action: () throws -> Void) {
        do {
            try action()
            error = nil
        } catch let error as AiLabError {
            self.error = error.message
        } catch {
            self.error = "\(error)"
        }
        refresh()
    }
}

extension AiLabError {
    var message: String {
        switch self {
        case .Busy: "A run is still going. Stop it first."
        case .EmptyPrompt: "Write a prompt first."
        case .NotRunning: "That run has already ended."
        }
    }
}
