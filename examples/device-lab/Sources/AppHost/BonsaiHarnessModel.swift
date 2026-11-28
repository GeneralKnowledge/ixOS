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
    /// Bonsai's current step as it writes it: the thought, then the file.
    /// Kept while the step runs, so the code stays on screen; cleared when
    /// the task ends.
    public private(set) var live: HarnessLive?
    public private(set) var ladder: [HarnessTask] = []
    public private(set) var files: [ProjectFile] = []
    /// The project's images, newest first, shown as thumbnails.
    public private(set) var artifacts: [ProjectArtifact] = []
    /// The images the last task made or changed, newest first: its result.
    public private(set) var made: [ProjectArtifact] = []
    public private(set) var error: String?
    /// The person's own task.
    public var request = ""
    /// The person's answer to a blocked plan's question.
    public var planAnswer = ""
    /// The Python Lab project the person's own tasks run in.
    public private(set) var project = "scratch"
    /// Python Lab's projects, to choose from.
    public private(set) var projects: [String] = ["scratch"]
    /// Which model runs the task: a `LlamaModelFile` id, or
    /// `scriptedID` for the known-good answers that test the harness itself.
    public var modelID: String = LlamaModelFile.catalogue[0].id

    /// The Scripted model: replays a ladder task's known-good answers, to
    /// prove the tools, guard and journal on this phone apart from Bonsai.
    public static let scriptedID = "scripted"
    public var isScripted: Bool { modelID == Self.scriptedID }

    @ObservationIgnored let harness: BonsaiHarness?
    @ObservationIgnored private let workspace: Workspace?
    @ObservationIgnored private let activity: PythonActivity
    /// A watch is running for a task that outlived its screen.
    @ObservationIgnored private var isWatching = false
    @ObservationIgnored private var models: [String: LlamaCppModel] = [:]
    /// The project's images when the task started, to tell what it made.
    @ObservationIgnored private var imagesBefore: [String: ProjectArtifact]?
    /// A task was started: load the files when it's seen stopped, even if
    /// it ended before a poll saw it running.
    @ObservationIgnored private var pendingRefresh = false

    public init(
        harness: BonsaiHarness?, workspace: Workspace? = nil, activity: PythonActivity = .shared
    ) {
        self.harness = harness
        self.workspace = workspace
        self.activity = activity
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

    /// Copies Rust's snapshot four times a second (ten while Bonsai writes) until the calling task is
    /// cancelled (attach with `.task`).
    public func follow() async {
        if !isScripted, !downloadedModels.contains(where: { $0.id == modelID }) {
            modelID = downloadedModels.first?.id ?? Self.scriptedID
        }
        reloadProjects()
        if snapshot?.task == nil, let harness {
            // Show the last task of the chosen project after a relaunch
            // (a task the app closed during is marked stopped).
            try? harness.openProject(name: project)
        }
        var wasRunning = isRunning
        while !Task.isCancelled {
            refresh()
            if (wasRunning || pendingRefresh) && !isRunning {
                pendingRefresh = false
                loadFiles()
            }
            wasRunning = isRunning
            // Faster while Bonsai writes, so the text streams smoothly.
            try? await Task.sleep(for: .milliseconds(snapshot?.phase == .thinking ? 100 : 250))
        }
        // The screen closed while Bonsai still works: keep watching, more
        // slowly, so Python Lab's project opens again when the task ends.
        if activity.bonsaiProject != nil { watchUntilDone() }
    }

    private func watchUntilDone() {
        guard !isWatching else { return }
        isWatching = true
        Task { [weak self] in
            while true {
                try? await Task.sleep(for: .milliseconds(500))
                guard let self else { return }
                self.refresh()
                if self.activity.bonsaiProject == nil {
                    self.isWatching = false
                    return
                }
            }
        }
    }

    public func refresh() {
        guard let harness else { return }
        let next = harness.snapshot()
        if next != snapshot { snapshot = next }
        // Ladder projects are hidden, so only the person's project is
        // ever one Python Lab has open.
        let working = next.running ? next.project : nil
        if activity.bonsaiProject != working { activity.bonsaiProject = working }
        if let current = next.live {
            if current != live { live = current }
        } else if !next.running, live != nil {
            live = nil
        }
    }

    public func run(_ task: HarnessTask) {
        if isScripted, let harness {
            attempt { try harness.startLadderScripted(taskId: task.id) }
            return
        }
        perform { harness, model in
            try harness.startLadder(model: model, taskId: task.id)
        }
    }

    public func runRequest() {
        // The project on screen first, so what the task makes is told
        // apart from what was there.
        if snapshot?.project != project, !isRunning {
            try? harness?.openProject(name: project)
        }
        perform { [request, project] harness, model in
            try harness.startCustom(model: model, request: request, project: project)
        }
    }

    /// Python Lab's projects again (one may have been made or renamed).
    public func reloadProjects() {
        guard let workspace else { return }
        projects = workspace.projects().map(\.name)
        if !projects.contains(project) { project = "scratch" }
    }

    /// The project the person's own tasks run in, and its last task on
    /// screen. Not while a task runs.
    public func choose(project name: String) {
        guard !isRunning, let harness, name != project else { return }
        do {
            try harness.openProject(name: name)
            project = name
            error = nil
        } catch let failure as HarnessError {
            error = failure.message
        } catch {
            self.error = "\(error)"
        }
        refresh()
        // The last task's "before" was another project's files.
        imagesBefore = nil
        made = []
        loadFiles()
    }

    /// Plans the person's own task as several tasks first (H4).
    public func runPlan() {
        if snapshot?.project != project, !isRunning {
            try? harness?.openProject(name: project)
        }
        perform { [request, project] harness, model in
            try harness.startCustomPlan(model: model, request: request, project: project)
        }
    }

    /// The person's own task, tests first: Bonsai writes the tests, the
    /// plan waits while the person checks them, then Bonsai writes the code.
    public func runTestsFirst() {
        if snapshot?.project != project, !isRunning {
            try? harness?.openProject(name: project)
        }
        perform { [request, project] harness, model in
            try harness.startCustomTestsFirst(model: model, request: request, project: project)
        }
    }

    /// Whether a plan is shown for approval before it runs (Rust keeps it).
    public var approvePlans: Bool {
        get { snapshot?.approvePlans ?? true }
        set {
            harness?.setApprovePlans(approve: newValue)
            refresh()
        }
    }

    /// Approves the plan on screen, or resumes it from its last verified
    /// task.
    public func continuePlan() {
        if isScripted, let harness {
            attempt { try harness.continuePlanScripted() }
            return
        }
        perform { harness, model in
            try harness.continuePlan(model: model)
        }
    }

    /// Answers a blocked plan's question and lets it go on.
    public func answerPlan() {
        let answer = planAnswer
        if isScripted, let harness {
            attempt { try harness.answerPlanScripted(answer: answer) }
        } else {
            perform { harness, model in
                try harness.answerPlan(model: model, answer: answer)
            }
        }
        if error == nil { planAnswer = "" }
    }

    public func discardPlan() {
        guard let harness else { return }
        do {
            try harness.discardPlan()
            error = nil
        } catch let failure as HarnessError {
            error = failure.message
        } catch {
            self.error = "\(error)"
        }
        refresh()
    }

    public func stop() {
        harness?.stop()
        refresh()
    }

    /// A finished step's whole answer, to go back through the task.
    public func replay(step: UInt32) -> HarnessLive? { harness?.replay(step: step) }

    /// The last task as Markdown, to share as evidence.
    public var transcript: String { harness?.transcript() ?? "" }

    public func loadFiles() {
        files = harness?.files() ?? []
        artifacts = projectFolder.map { projectArtifacts(project: $0) } ?? []
        if !isRunning, let before = imagesBefore {
            made = artifacts.filter { artifact in
                guard let old = before[artifact.path] else { return true }
                return old.bytes != artifact.bytes || old.modifiedMs != artifact.modifiedMs
            }
        }
    }

    /// The folder of the project on screen.
    public var projectFolder: String? { harness?.projectFolder() }

    /// Where an artifact is, to show or share it.
    public func url(of artifact: ProjectArtifact) -> URL? {
        projectFolder.map { URL(fileURLWithPath: $0).appendingPathComponent(artifact.path) }
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
        attempt { try start(harness, model) }
    }

    private func attempt(_ start: () throws -> Void) {
        imagesBefore = Dictionary(
            (projectFolder.map { projectArtifacts(project: $0) } ?? []).map { ($0.path, $0) },
            uniquingKeysWith: { first, _ in first })
        made = []
        do {
            try start()
            pendingRefresh = true
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
        case .NoPlan: "There is no plan to approve or resume."
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

extension PlanState {
    var title: String {
        switch self {
        case .planning: "Writing the plan"
        case .awaitingApproval: "Waiting for your approval"
        case .running: "Running"
        case .completed: "Completed"
        case .failed: "Failed"
        case .stopped: "Stopped"
        case .discarded: "Discarded"
        case .blocked: "Blocked: needs your answer"
        }
    }
}
