@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI

/// Bonsai Harness: give Bonsai a Python task and watch it work in a Python
/// Lab project on this phone. The harness decides when the task is done,
/// from what Python printed, never from the model's word
/// (docs/bonsai-harness-plan.md).
struct BonsaiHarnessView: View {
    @Bindable var model: BonsaiHarnessModel
    /// The step on screen when going back through a finished task; nil
    /// follows the last one.
    @State private var replayPosition: Int?

    var body: some View {
        List {
            // Python Lab shares the interpreter, so its script waiting for
            // an answer holds Bonsai's Python runs too.
            if PythonActivity.shared.labWaitingForInput {
                Section {
                    Label("Python Lab is waiting for an answer. Bonsai's Python runs wait until it gets one or is stopped.", systemImage: "hourglass")
                        .font(.footnote)
                        .forgeForeground(.warning)
                        .accessibilityIdentifier("harness.pythonLabWaiting")
                }
            }
            if let error = model.error {
                Section {
                    Text(error).font(.footnote).forgeForeground(.danger)
                        .accessibilityIdentifier("harness.error")
                }
            }
            if model.isAvailable {
                modelSection
                if model.isRunning {
                    if let snapshot = model.snapshot, let task = snapshot.task {
                        BonsaiLiveSection(
                            task: task.description,
                            step: snapshot.phase == .thinking ? snapshot.steps.count + 1 : max(snapshot.steps.count, 1),
                            phase: snapshot.phase,
                            live: model.live
                        )
                    }
                    Section {
                        Button("Stop", systemImage: "stop.fill", role: .destructive) { model.stop() }
                            .accessibilityIdentifier("harness.stop")
                    }
                } else if let snapshot = model.snapshot, let task = snapshot.task, !snapshot.steps.isEmpty {
                    // The finished task stays where the live view was, to
                    // step back through, with its result and pictures below.
                    BonsaiReplaySection(
                        task: task.description,
                        steps: snapshot.steps,
                        replay: model.replay(step:),
                        position: $replayPosition
                    )
                }
                if let snapshot = model.snapshot, let plan = snapshot.plan {
                    BonsaiPlanSection(
                        plan: plan,
                        draft: snapshot.task == nil ? snapshot.draft : "",
                        running: snapshot.running,
                        answer: $model.planAnswer,
                        approve: model.continuePlan,
                        discard: model.discardPlan,
                        send: model.answerPlan
                    )
                }
                if let snapshot = model.snapshot, let task = snapshot.task {
                    statusSection(snapshot, task)
                }
                if !model.isRunning {
                    tasksSection
                }
                if let snapshot = model.snapshot, snapshot.task != nil {
                    stepsSection(snapshot)
                    projectSection(snapshot)
                }
            }
        }
        .groupedList()
        .navigationTitle("Bonsai Harness")
        .inlineNavigationTitle()
        .task { await model.follow() }
        .onChange(of: model.snapshot?.task?.id) { replayPosition = nil }
        .onChange(of: model.isRunning) { if model.isRunning { replayPosition = nil } }
    }

    private var modelSection: some View {
        Section {
            Picker("Model", selection: $model.modelID) {
                ForEach(model.downloadedModels) { file in
                    Text(file.name).tag(file.id)
                }
                Text("Scripted (tests the harness)").tag(BonsaiHarnessModel.scriptedID)
            }
            .disabled(model.isRunning)
            .accessibilityIdentifier("harness.model")
            if model.downloadedModels.isEmpty {
                Text("Download a Bonsai model in Tools, Bonsai on iPhone, to let Bonsai do the tasks.")
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("harness.noModel")
            }
            if model.isScripted {
                Text("Scripted replays known-good answers for the ladder tasks, so a run proves the tools, the guard and the evidence checks on this phone without a model.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        } header: {
            Text("Model")
        } footer: {
            if let limits = model.snapshot?.limits {
                Text("Up to \(limits.maxSteps) steps and \(limits.taskSeconds / 60) minutes per task, \(limits.runSeconds) s per Python run. Python's network is off.")
            }
        }
    }

    private var tasksSection: some View {
        Section {
            ForEach(model.ladder, id: \.id) { task in
                Button {
                    model.run(task)
                } label: {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(task.title).font(.body.weight(.semibold))
                        Text(task.request).font(.footnote).foregroundStyle(.secondary)
                        Text("Done when \(task.acceptance.joined(separator: "; ")).")
                            .font(.caption).foregroundStyle(.secondary)
                        if task.planned {
                            Label("Bonsai plans it as several tasks first", systemImage: "list.number")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        if task.endsBlocked {
                            Label("It can't succeed here: it should stop and ask you", systemImage: "questionmark.bubble")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
                .accessibilityIdentifier("harness.task.\(task.id)")
            }
            TextField("Your own task", text: $model.request, axis: .vertical)
                .lineLimit(2...5)
                .accessibilityIdentifier("harness.request")
            Button("Run My Task", systemImage: "play.fill") { model.runRequest() }
                .disabled(model.isScripted || model.request.isEmpty)
                .accessibilityIdentifier("harness.runRequest")
            Button("Plan My Task", systemImage: "list.number") { model.runPlan() }
                .disabled(model.isScripted || model.request.isEmpty)
                .accessibilityIdentifier("harness.planRequest")
            Toggle("Show Plans Before They Run", isOn: $model.approvePlans)
                .accessibilityIdentifier("harness.approvePlans")
        } header: {
            Text("Tasks")
        } footer: {
            Text("Each ladder task starts from a fresh project. Your own tasks share one project, kept between tasks. A planned task is split into small tasks, each checked before the next starts.")
        }
    }

    private func statusSection(_ snapshot: HarnessSnapshot, _ task: HarnessTaskRecord) -> some View {
        Section {
            LabeledContent("Status", value: task.status.title)
                .accessibilityIdentifier("harness.status")
            if snapshot.running {
                LabeledContent("Now", value: phase(snapshot.phase))
            }
            LabeledContent("Time", value: "\(Format.number(Double(snapshot.elapsedMs) / 1000, digits: 0)) s")
            LabeledContent("Finish claims checked", value: "\(task.attempts)")
            if let result = task.result {
                Text(result)
                    .font(.footnote)
                    .forgeForeground(task.status == .completed ? .success : .danger)
                    .accessibilityIdentifier("harness.result")
            }
            if !snapshot.running, let made = model.made.first {
                NavigationLink {
                    ArtifactDetailView(artifact: made, url: model.url(of: made))
                } label: {
                    MadePreview(artifact: made, url: model.url(of: made))
                }
                .accessibilityLabel("Made \(made.path)")
                .accessibilityIdentifier("harness.made")
            }
            if !snapshot.running {
                ShareLink(item: model.transcript) {
                    Label("Share Transcript", systemImage: "square.and.arrow.up")
                }
                .accessibilityIdentifier("harness.shareTranscript")
            }
        } header: {
            Text(task.description)
        } footer: {
            Text("Done when \(task.acceptance.joined(separator: "; ")). Model: \(task.modelId).")
        }
    }

    private func stepsSection(_ snapshot: HarnessSnapshot) -> some View {
        Section {
            if snapshot.steps.isEmpty {
                Text("No steps yet.").foregroundStyle(.secondary)
            }
            ForEach(snapshot.steps, id: \.index) { step in
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Image(systemName: icon(step.ok))
                            .forgeForeground(step.ok == false ? .danger : step.ok == true ? .success : .neutral)
                            .accessibilityHidden(true)
                        Text("\(step.index). \(step.tool)").font(.subheadline.weight(.semibold))
                        Spacer()
                        Text("\(Format.number(Double(step.modelMs + step.toolMs) / 1000, digits: 1)) s")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    if !step.thought.isEmpty {
                        Text(step.thought).font(.footnote)
                    }
                    Text(step.call)
                        .font(.system(.caption2, design: .monospaced))
                        .foregroundStyle(.secondary)
                        .lineLimit(6)
                    if !step.result.isEmpty {
                        Text(step.result)
                            .font(.system(.caption2, design: .monospaced))
                            .lineLimit(12)
                            .textSelection(.enabled)
                    }
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("harness.step.\(step.index)")
            }
        } header: {
            Text("Steps")
        }
    }

    private func projectSection(_ snapshot: HarnessSnapshot) -> some View {
        Section {
            ForEach(model.artifacts, id: \.path) { artifact in
                ArtifactRow(artifact: artifact, url: model.url(of: artifact))
            }
            ForEach(model.files.filter { file in !model.artifacts.contains { $0.path == file.path } }, id: \.path) { file in
                NavigationLink {
                    ScrollView {
                        Text(file.text)
                            .font(.system(.footnote, design: .monospaced))
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding()
                    }
                    .navigationTitle(file.path)
                    .inlineNavigationTitle()
                } label: {
                    LabeledContent(file.path, value: Format.storage(file.bytes))
                }
                .accessibilityIdentifier("harness.file.\(file.path)")
            }
            if model.files.isEmpty {
                Text("No files yet.").foregroundStyle(.secondary)
            }
        } header: {
            Text("Project \(snapshot.project ?? "")")
        }
        .onAppear { model.loadFiles() }
    }

    private func phase(_ phase: HarnessPhase) -> String {
        switch phase {
        case .idle: "Idle"
        case .thinking: "Bonsai is choosing the next step"
        case .acting: "Running the step"
        case .verifying: "Checking the evidence"
        }
    }

    private func icon(_ ok: Bool?) -> String {
        switch ok {
        case true?: "checkmark.circle.fill"
        case false?: "xmark.circle.fill"
        case nil: "circle.dotted"
        }
    }
}

/// The picture a task made, big enough to judge, right under the result.
/// Opens the full view to zoom or share.
private struct MadePreview: View {
    let artifact: ProjectArtifact
    let url: URL?

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Made \(artifact.path)").font(.subheadline.weight(.semibold))
            Group {
                if artifact.kind == "svg" {
                    if let url, let svg = try? String(contentsOf: url, encoding: .utf8) {
                        SVGView(svg: svg).allowsHitTesting(false)
                    }
                } else if let image = ArtifactImage.load(url) {
                    image.resizable().scaledToFit()
                }
            }
            .frame(maxWidth: .infinity, minHeight: 120, maxHeight: 260)
            .background(.white, in: RoundedRectangle(cornerRadius: 10))
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .id("\(artifact.path)-\(artifact.modifiedMs)-\(artifact.bytes)")
        }
        .padding(.vertical, 4)
    }
}
