@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI

/// AI Lab: repeatable experiments with Apple's on-device model, with the
/// timings and device readings of each run (AiLabModel, rust/src/ai_lab.rs).
struct AiLabView: View {
    @Bindable var model: AiLabModel
    /// Spike: the backends llama.cpp finds on this phone (AiLabLlama.swift).
    @State private var llamaBackends = LlamaProbe.describe()

    private var s: AiLabSnapshot { model.snapshot }

    var body: some View {
        List {
            modelSection
            experimentsSection
            if let run = s.running {
                Section("Running") {
                    RunView(run: run, live: true)
                    Button("Stop", systemImage: "stop.fill", role: .destructive) { model.stop() }
                        .accessibilityIdentifier("aiLab.stop")
                }
            }
            if let error = model.error {
                Section {
                    Text(error).forgeForeground(.danger)
                }
            }
            historySection
        }
        .groupedList()
        .navigationTitle("AI Lab")
        .inlineNavigationTitle()
        .task { await model.follow() }
    }

    // MARK: Model

    @ViewBuilder private var modelSection: some View {
        Section {
            LabeledContent("Model", value: s.model.name)
            LabeledContent("Status", value: s.model.available ? "Ready" : "Unavailable")
                .accessibilityIdentifier("aiLab.status")
            if let reason = s.model.unavailableReason {
                Text(reason).font(.footnote).foregroundStyle(.secondary)
            }
            LabeledContent("Context", value: "\(s.model.contextTokens) tokens")
            LabeledContent("llama.cpp", value: llamaBackends)
                .accessibilityIdentifier("aiLab.llamaBackends")
            Button("Check Again", systemImage: "arrow.clockwise") { model.refreshModel() }
                .accessibilityIdentifier("aiLab.checkModel")
        } footer: {
            Text("Runs on this iPhone. Nothing leaves the device.")
        }
    }

    // MARK: Experiments

    @ViewBuilder private var experimentsSection: some View {
        Section("Experiments") {
            ForEach(model.experiments, id: \.kind) { experiment in
                Button {
                    model.run(experiment)
                } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(experiment.title)
                        Text(experiment.detail).font(.footnote).foregroundStyle(.secondary)
                    }
                }
                .disabled(model.isRunning)
                .accessibilityIdentifier("aiLab.run.\(experiment.kind.id)")
            }
        }
        Section {
            TextField("Your own prompt", text: $model.customPrompt, axis: .vertical)
                .lineLimit(1...5)
                .accessibilityIdentifier("aiLab.prompt")
            Button("Run Prompt", systemImage: "play.fill") { model.runCustom() }
                .disabled(model.isRunning || model.customPrompt.trimmingCharacters(in: .whitespaces).isEmpty)
                .accessibilityIdentifier("aiLab.runPrompt")
        }
    }

    // MARK: History

    @ViewBuilder private var historySection: some View {
        Section {
            if s.history.isEmpty {
                Text("No runs yet").foregroundStyle(.secondary)
            }
            ForEach(s.history, id: \.id) { run in
                DisclosureGroup {
                    Text(run.answer.isEmpty ? "No text" : run.answer)
                        .font(.callout)
                        .textSelection(.enabled)
                } label: {
                    RunView(run: run, live: false)
                }
                .accessibilityIdentifier("aiLab.history.row")
            }
        } header: {
            Text("Runs")
        } footer: {
            if !s.history.isEmpty {
                Button("Clear Runs", role: .destructive) { model.clearHistory() }
                    .accessibilityIdentifier("aiLab.clear")
            }
        }
    }
}

/// One run's numbers; while it runs, its answer streams in under them.
private struct RunView: View {
    let run: AiRun
    let live: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(run.kind.title).font(.subheadline.weight(.semibold))
                Spacer()
                Text(run.state.name)
                    .font(.subheadline)
                    .foregroundStyle(run.state.color)
            }
            Text(timings).font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
            if let device = deviceLine {
                Text(device).font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
            }
            if let error = run.error {
                Text(error).font(.footnote).forgeForeground(.danger)
            }
            if live {
                Text(run.answer.isEmpty ? "Waiting for the first text…" : run.answer)
                    .font(.callout)
                    .foregroundStyle(run.answer.isEmpty ? .secondary : .primary)
                    .accessibilityIdentifier("aiLab.answer")
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var timings: String {
        var parts: [String] = []
        if let first = run.firstTextMs {
            parts.append("first text \(Self.seconds(first))")
        }
        parts.append("total \(Self.seconds(run.totalMs))")
        if !live {
            parts.append("≈\(run.answerTokens) tokens")
            if let speed = run.tokensPerSecond {
                parts.append("\(Format.number(speed, digits: 1)) tok/s")
            }
            parts.append(run.pieces == 1 ? "1 piece" : "\(run.pieces) pieces")
        }
        return parts.joined(separator: " · ")
    }

    private var deviceLine: String? {
        var parts: [String] = []
        if let before = run.memoryBeforeBytes {
            if let after = run.memoryAfterBytes {
                parts.append("memory \(Format.memory(before)) → \(Format.memory(after))")
            } else {
                parts.append("memory \(Format.memory(before))")
            }
        }
        if let before = run.thermalBefore {
            if let after = run.thermalAfter, after != before {
                parts.append("\(before.name) → \(after.name)")
            } else {
                parts.append(before.name)
            }
        }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    private static func seconds(_ ms: UInt64) -> String {
        "\(Format.number(Double(ms) / 1000, digits: 2)) s"
    }
}

extension AiExperimentKind {
    /// For accessibility identifiers (`aiLab.run.summary`).
    var id: String {
        switch self {
        case .generation: "generation"
        case .instructions: "instructions"
        case .summary: "summary"
        case .structured: "structured"
        case .structuredSchema: "structuredSchema"
        case .longAnswer: "longAnswer"
        case .custom: "custom"
        }
    }

    var title: String {
        switch self {
        case .generation: "Short Answer"
        case .instructions: "Follow Instructions"
        case .summary: "Summarise"
        case .structured: "Structured Output"
        case .structuredSchema: "Structured, No Macro"
        case .longAnswer: "Long Answer"
        case .custom: "Your Prompt"
        }
    }
}

extension AiRunState {
    var name: String {
        switch self {
        case .running: "Running"
        case .finished: "Done"
        case .stopped: "Stopped"
        case .refused: "Declined"
        case .failed: "Failed"
        }
    }

    var color: Color {
        switch self {
        case .running: .accentColor
        case .finished: .green
        case .stopped: .secondary
        case .refused: .orange
        case .failed: .red
        }
    }
}
