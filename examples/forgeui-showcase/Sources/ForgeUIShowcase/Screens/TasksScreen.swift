#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

/// A list screen: search, a summary card, rows with badges, swipe actions
/// with a confirmation, and every state a list can be in (loading, failed,
/// empty, no results). The Simulate menu plays them.
struct TasksScreen: View {
    let model: TasksModel

    @State private var editing: TaskFormModel?
    @State private var pendingDelete: ShowcaseTask?

    @Environment(\.forgeTheme) private var theme

    init(model: TasksModel) {
        self.model = model
    }

    var body: some View {
        @Bindable var model = model
        content
            .forgeGroupedBackground()
            .navigationTitle("Tasks")
            .searchable(text: $model.searchText, prompt: "Search tasks")
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button { editing = TaskFormModel() } label: { Label("Add Task", systemImage: "plus") }
                        .accessibilityIdentifier("tasks.add")
                }
                ToolbarItem(placement: .secondaryAction) {
                    simulateMenu
                }
            }
            .sheet(item: $editing) { form in
                TaskFormScreen(model: form, tasks: model)
            }
            .confirmationDialog(
                "Delete this task?",
                isPresented: Binding(get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } }),
                titleVisibility: .visible,
                presenting: pendingDelete
            ) { task in
                Button("Delete Task", role: .destructive) { model.delete(task.id) }
                    .accessibilityIdentifier("tasks.delete.confirm")
            } message: { task in
                Text("“\(task.title)” will be removed. This can't be undone.")
            }
    }

    @ViewBuilder
    private var content: some View {
        switch model.phase {
        case .loading:
            ForgeStateView.loading("Loading tasks")
        case let .failed(message):
            ForgeStateView.failed("Couldn't Load Tasks", message: message, retry: ForgeStateAction("Try Again", identifier: "tasks.retry") {
                Task { await model.reload(failing: false) }
            })
        case .loaded where model.tasks.isEmpty:
            ForgeStateView.empty("No Tasks", systemImage: "checklist", description: "Tasks you add appear here.",
                                 action: ForgeStateAction("Add Task", identifier: "tasks.empty.add") { editing = TaskFormModel() })
        case .loaded where model.visibleTasks.isEmpty:
            ForgeStateView.noResults(for: model.searchText)
        case .loaded:
            list
        }
    }

    private var list: some View {
        List {
            if model.searchText.isEmpty {
                Section {
                    ForgeCard {
                        HStack(alignment: .firstTextBaseline) {
                            Text("\(model.openCount) open")
                                .font(.title2.weight(.bold))
                            Spacer()
                            ForgeStatus(model.openCount == 0 ? "All done" : "On track", tone: model.openCount == 0 ? .success : .accent)
                        }
                        ProgressView(value: Double(model.tasks.count - model.openCount), total: Double(max(model.tasks.count, 1)))
                            .accessibilityLabel(Text("Done"))
                    }
                    .listRowInsets(EdgeInsets())
                    .listRowBackground(Color.clear)
                }
            }
            Section {
                ForEach(model.visibleTasks) { task in
                    NavigationLink {
                        TaskDetailScreen(taskID: task.id, model: model)
                    } label: {
                        TaskRow(task: task)
                    }
                    .accessibilityIdentifier("tasks.row.\(task.title)")
                    .swipeActions(edge: .trailing) {
                        Button { pendingDelete = task } label: { Label("Delete", systemImage: "trash") }
                            .tint(theme.colors.tone(.danger).solid)
                            .accessibilityIdentifier("tasks.delete")
                    }
                    .swipeActions(edge: .leading) {
                        Button { model.toggleDone(task.id) } label: {
                            Label(task.isDone ? "Reopen" : "Done", systemImage: task.isDone ? "arrow.uturn.backward" : "checkmark")
                        }
                        .tint(theme.colors.tone(.success).solid)
                        .accessibilityIdentifier("tasks.toggleDone")
                    }
                }
            }
        }
    }

    private var simulateMenu: some View {
        Menu {
            Button("Load") { Task { await model.reload(failing: false) } }
                .accessibilityIdentifier("tasks.simulate.load")
            Button("Fail to Load") { Task { await model.reload(failing: true) } }
                .accessibilityIdentifier("tasks.simulate.fail")
            Button("Remove All") { model.deleteAll() }
                .accessibilityIdentifier("tasks.simulate.empty")
            Button("Restore Samples") { model.restoreSamples() }
                .accessibilityIdentifier("tasks.simulate.restore")
        } label: {
            Label("Simulate", systemImage: "wand.and.stars")
        }
        .accessibilityIdentifier("tasks.simulate")
    }
}

private struct TaskRow: View {
    let task: ShowcaseTask

    @Environment(\.forgeTheme) private var theme

    var body: some View {
        ForgeRow(
            task.title,
            subtitle: task.notes.isEmpty ? nil : task.notes,
            systemImage: task.isDone ? "checkmark" : "circle",
            tone: task.isDone ? .success : .neutral
        ) {
            if !task.isDone {
                ForgeBadge(task.priority.title, tone: task.priority.tone)
            }
        }
        .opacity(task.isDone ? 0.6 : 1)
        .accessibilityValue(Text(task.isDone ? "Done" : task.priority.title + " priority"))
    }
}

/// A detail screen: header, notes, and the actions on one task.
struct TaskDetailScreen: View {
    let taskID: ShowcaseTask.ID
    let model: TasksModel

    @State private var editing: TaskFormModel?
    @State private var confirmingDelete = false
    @Environment(\.dismiss) private var dismiss

    init(taskID: ShowcaseTask.ID, model: TasksModel) {
        self.taskID = taskID
        self.model = model
    }

    var body: some View {
        if let task = model.task(taskID) {
            List {
                Section {
                    ForgeDetailHeader(task.title, subtitle: task.isDone ? "Done" : "Open", systemImage: "checklist",
                                      tone: task.isDone ? .success : .accent) {
                        ForgeBadge(task.priority.title + " priority", tone: task.priority.tone)
                    }
                    .listRowBackground(Color.clear)
                }
                Section("Notes") {
                    if task.notes.isEmpty {
                        Text("No notes").foregroundStyle(.secondary)
                    } else {
                        Text(task.notes)
                    }
                }
                Section {
                    ForgeButton(task.isDone ? "Reopen Task" : "Mark as Done", systemImage: task.isDone ? "arrow.uturn.backward" : "checkmark") {
                        model.toggleDone(task.id)
                    }
                    .buttonStyle(.forge(.primary, expands: true))
                    .accessibilityIdentifier("task.toggleDone")
                    Button("Delete Task", role: .destructive) { confirmingDelete = true }
                        .buttonStyle(.forge(.tertiary, expands: true))
                        .accessibilityIdentifier("task.delete")
                }
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets())
            }
            .forgeGroupedBackground()
            .navigationTitle("Task")
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button("Edit") { editing = TaskFormModel(editing: task) }
                        .accessibilityIdentifier("task.edit")
                }
            }
            .sheet(item: $editing) { form in
                TaskFormScreen(model: form, tasks: model)
            }
            .confirmationDialog("Delete this task?", isPresented: $confirmingDelete, titleVisibility: .visible) {
                Button("Delete Task", role: .destructive) {
                    model.delete(task.id)
                    dismiss()
                }
                .accessibilityIdentifier("task.delete.confirm")
            } message: {
                Text("This can't be undone.")
            }
        } else {
            ForgeStateView.empty("Task Deleted", systemImage: "trash", description: "This task no longer exists.")
        }
    }
}
#endif
