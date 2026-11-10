import ForgeUI
import Foundation
import Observation

public struct ShowcaseTask: Identifiable, Hashable, Sendable {
    public enum Priority: String, CaseIterable, Identifiable, Sendable {
        case low
        case normal
        case high

        public var id: String { rawValue }

        public var title: String {
            switch self {
            case .low: "Low"
            case .normal: "Normal"
            case .high: "High"
            }
        }

        public var tone: ForgeTone {
            switch self {
            case .low: .neutral
            case .normal: .info
            case .high: .danger
            }
        }
    }

    public let id: UUID
    public var title: String
    public var notes: String
    public var priority: Priority
    public var isDone: Bool

    public init(id: UUID = UUID(), title: String, notes: String = "", priority: Priority = .normal, isDone: Bool = false) {
        self.id = id
        self.title = title
        self.notes = notes
        self.priority = priority
        self.isDone = isDone
    }

    public static let samples: [ShowcaseTask] = [
        ShowcaseTask(title: "Renew passport", notes: "Photos are in the desk drawer.", priority: .high),
        ShowcaseTask(title: "Water the plants", priority: .normal),
        ShowcaseTask(title: "Book a dentist appointment", priority: .normal),
        ShowcaseTask(title: "Return library books", notes: "Two are due Friday.", priority: .low),
        ShowcaseTask(title: "Call Mia about the weekend", priority: .low, isDone: true),
    ]
}

/// The task list screen's state. Foundation and Observation only, so it is
/// tested on Linux. The tasks live in memory: the showcase demonstrates
/// screens, not storage.
@Observable
@MainActor
public final class TasksModel {
    public enum Phase: Equatable, Sendable {
        case loading
        case loaded
        case failed(String)
    }

    public private(set) var tasks: [ShowcaseTask]
    public private(set) var phase: Phase = .loaded
    public var searchText = ""

    public init(tasks: [ShowcaseTask] = ShowcaseTask.samples) {
        self.tasks = tasks
    }

    /// Open tasks first, then done ones; filtered by the search text.
    public var visibleTasks: [ShowcaseTask] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        let matching = query.isEmpty ? tasks : tasks.filter {
            $0.title.localizedCaseInsensitiveContains(query) || $0.notes.localizedCaseInsensitiveContains(query)
        }
        return matching.filter { !$0.isDone } + matching.filter(\.isDone)
    }

    public var openCount: Int { tasks.filter { !$0.isDone }.count }

    public func task(_ id: ShowcaseTask.ID) -> ShowcaseTask? {
        tasks.first { $0.id == id }
    }

    @discardableResult
    public func add(_ draft: TaskDraft, priority: ShowcaseTask.Priority) -> ShowcaseTask {
        let task = ShowcaseTask(title: clean(draft.title), notes: clean(draft.notes), priority: priority)
        tasks.insert(task, at: 0)
        return task
    }

    public func update(_ id: ShowcaseTask.ID, with draft: TaskDraft, priority: ShowcaseTask.Priority) {
        guard let index = tasks.firstIndex(where: { $0.id == id }) else { return }
        tasks[index].title = clean(draft.title)
        tasks[index].notes = clean(draft.notes)
        tasks[index].priority = priority
    }

    public func toggleDone(_ id: ShowcaseTask.ID) {
        guard let index = tasks.firstIndex(where: { $0.id == id }) else { return }
        tasks[index].isDone.toggle()
    }

    public func delete(_ id: ShowcaseTask.ID) {
        tasks.removeAll { $0.id == id }
    }

    public func deleteAll() {
        tasks.removeAll()
    }

    public func restoreSamples() {
        tasks = ShowcaseTask.samples
        phase = .loaded
    }

    /// Plays a load, to show the loading and error states.
    public func reload(failing: Bool) async {
        phase = .loading
        try? await Task.sleep(for: .milliseconds(900))
        phase = failing ? .failed("The server didn't answer. Check your connection and try again.") : .loaded
    }

    private func clean(_ text: String) -> String {
        text.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

/// The create/edit form's state.
@Observable
@MainActor
public final class TaskFormModel: Identifiable {
    public var title: String
    public var notes: String
    public var priority: ShowcaseTask.Priority
    public private(set) var isSaving = false
    public private(set) var titleTouched = false
    /// The task being edited, or nil for a new one.
    public let editing: ShowcaseTask.ID?

    public init(editing task: ShowcaseTask? = nil) {
        editing = task?.id
        title = task?.title ?? ""
        notes = task?.notes ?? ""
        priority = task?.priority ?? .normal
        titleTouched = task != nil
    }

    public var isNew: Bool { editing == nil }
    public var draft: TaskDraft { TaskDraft(title: title, notes: notes) }
    public var titleMessage: ForgeFieldMessage? { TaskRules.titleMessage(draft, touched: titleTouched) }
    public var notesMessage: ForgeFieldMessage { TaskRules.notesMessage(draft) }
    public var canSave: Bool { TaskRules.canSave(draft) && !isSaving }

    /// The view calls this when the title field changes.
    public func titleEdited() {
        titleTouched = true
    }

    /// Saves into `tasks` after a short pause (standing in for real work, so
    /// the busy state shows). Returns false if the draft isn't valid.
    public func save(into tasks: TasksModel) async -> Bool {
        titleTouched = true
        guard canSave else { return false }
        isSaving = true
        defer { isSaving = false }
        try? await Task.sleep(for: .milliseconds(700))
        if let editing {
            tasks.update(editing, with: draft, priority: priority)
        } else {
            tasks.add(draft, priority: priority)
        }
        return true
    }
}
