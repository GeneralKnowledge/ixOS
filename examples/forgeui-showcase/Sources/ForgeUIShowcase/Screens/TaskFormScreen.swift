#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

/// A create/edit form in a sheet: labelled fields with validation messages
/// from the model, a segmented picker, and Cancel / Save that turns into a
/// spinner while saving. SwiftUI's own `Form`; ForgeUI adds the field
/// labels, the messages and the toolbar.
struct TaskFormScreen: View {
    @Bindable var model: TaskFormModel
    let tasks: TasksModel

    @Environment(\.dismiss) private var dismiss
    @FocusState private var titleFocused: Bool

    init(model: TaskFormModel, tasks: TasksModel) {
        self.model = model
        self.tasks = tasks
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    ForgeField("Title", message: model.titleMessage) {
                        TextField("What needs doing?", text: $model.title)
                            .focused($titleFocused)
                            .submitLabel(.next)
                            .accessibilityIdentifier("taskForm.title")
                    }
                    ForgeField("Notes", message: model.notesMessage) {
                        TextField("Optional", text: $model.notes, axis: .vertical)
                            .lineLimit(3...8)
                            .accessibilityIdentifier("taskForm.notes")
                    }
                }
                Section("Priority") {
                    Picker("Priority", selection: $model.priority) {
                        ForEach(ShowcaseTask.Priority.allCases) { Text($0.title).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .accessibilityIdentifier("taskForm.priority")
                }
            }
            .forgeGroupedBackground()
            .navigationTitle(model.isNew ? "New Task" : "Edit Task")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .forgeFormToolbar(
                id: "taskForm",
                confirmTitle: model.isNew ? "Add" : "Save",
                canConfirm: model.canSave,
                isWorking: model.isSaving,
                cancel: { dismiss() },
                confirm: {
                    Task {
                        if await model.save(into: tasks) { dismiss() }
                    }
                }
            )
            .onChange(of: model.title) { model.titleEdited() }
            .onAppear { if model.isNew { titleFocused = true } }
        }
    }
}
#endif
