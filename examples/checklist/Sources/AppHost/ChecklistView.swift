#if canImport(SwiftUI)
import ChecklistFFI
import Combine
import ForgeKit
import ForgeUI
import SwiftUI
import UniformTypeIdentifiers

/// The whole app: ForgeKit opens the core, then shows the checklist.
public struct ChecklistRoot: View {
    @Bindable private var host: ForgeHost<ChecklistCore>

    public init(host: ForgeHost<ChecklistCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            ChecklistScreen(core: core, changes: host.changes)
        }
        .forgeTheme(.standard)
    }
}

struct ChecklistScreen: View {
    @State private var model: ChecklistModel
    let changes: UInt64

    @State private var newTitle = ""
    @State private var query = ""
    @State private var results: [Item]?
    /// The words `results` answer; until they match `query`, a search is
    /// still running and the screen must not say "No Results".
    @State private var resultsFor = ""
    @State private var editing: Editing?
    @State private var dueEditing: Editing?
    @State private var showing: Showing = .all
    @State private var export: URL?
    @State private var importing = false
    @State private var imported: CsvImportSummary?
    @State private var confirmingClear = false
    @FocusState private var addFocused: Bool

    init(core: ChecklistCore, changes: UInt64) {
        _model = State(initialValue: ChecklistModel(core: core))
        self.changes = changes
    }

    private var searching: Bool { !query.trimmingCharacters(in: .whitespaces).isEmpty }
    private var shown: [Item] { searching ? (results ?? []) : model.items }
    private var showingAgenda: Bool { !searching && showing == .agenda }

    var body: some View {
        NavigationStack {
            List {
                if !searching {
                    Picker("Show", selection: $showing) {
                        Text("All").tag(Showing.all)
                        Text("Agenda").tag(Showing.agenda)
                    }
                    .pickerStyle(.segmented)
                    .listRowBackground(Color.clear)
                    .accessibilityIdentifier("checklist.showing")
                }
                if showingAgenda {
                    agendaSection("Overdue", model.agenda.overdue, id: "overdue")
                    agendaSection("Today", model.agenda.today, id: "today")
                    agendaSection("Upcoming", model.agenda.upcoming, id: "upcoming")
                } else {
                    if !searching {
                        addRow
                    }
                    Section {
                        ForEach(shown, id: \.id) { item in
                            row(item)
                        }
                    } footer: {
                        if !searching && model.summary.total > 0 {
                            Text("\(model.summary.done) of \(model.summary.total) done")
                                .forgeText(.note)
                                .accessibilityIdentifier("checklist.summary")
                        }
                    }
                }
            }
            .overlay { emptyState }
            .navigationTitle("Checklist")
            .searchable(text: $query, prompt: "Search items")
            .toolbar { toolbar }
            // The sheet shows a failed rename itself: an alert from this
            // screen can't present over it (friction log A39).
            .forgeRename("Rename", item: $editing, name: \.item.title, id: "rename") { editing, title in
                if await model.rename(editing.item, to: title) { return nil }
                let reason = model.errorMessage
                model.errorMessage = nil
                return reason ?? String(localized: "The item couldn't be renamed.")
            }
            .sheet(item: $dueEditing) { editing in
                DueDateSheet(model: model, item: editing.item)
            }
            .forgeShareFile($export)
            .fileImporter(isPresented: $importing, allowedContentTypes: [.commaSeparatedText]) { result in
                switch result {
                case .success(let url):
                    Task { imported = await model.importCSV(at: url) }
                case .failure(let error):
                    model.errorMessage = "The file couldn't be opened. \(error.localizedDescription)"
                }
            }
            // Clearing removes every done item at once and can't be undone.
            .confirmationDialog(
                "Clear \(model.summary.done) done \(model.summary.done == 1 ? "item" : "items")?",
                isPresented: $confirmingClear, titleVisibility: .visible
            ) {
                Button("Clear Done Items", role: .destructive) {
                    Task { await model.clearDone() }
                }
                .accessibilityIdentifier("checklist.clearDone.confirm")
            } message: {
                Text("They're deleted from this iPhone. This can't be undone.")
            }
            .alert(
                importTitle,
                isPresented: Binding(get: { imported != nil }, set: { if !$0 { imported = nil } })
            ) {
                Button("OK", role: .cancel) {}
                    .accessibilityIdentifier("checklist.importOK")
            } message: {
                Text(importMessage)
            }
            // Held back while the Rename or Due Date sheet is up; it shows
            // once the sheet closes.
            .alert(
                "Couldn't do that",
                isPresented: Binding(
                    get: { editing == nil && dueEditing == nil && model.errorMessage != nil },
                    set: { if !$0 { model.errorMessage = nil } }
                )
            ) {
                Button("OK", role: .cancel) {}
                    .accessibilityIdentifier("checklist.errorOK")
            } message: {
                Text(model.errorMessage ?? "")
            }
        }
        // Rust committed something (or the app came back from the
        // background): re-read.
        .onChange(of: changes) { model.reload() }
        // Midnight passed with the app open: the agenda's Today moves on.
        .onReceive(NotificationCenter.default.publisher(for: .NSCalendarDayChanged).receive(on: RunLoop.main)) { _ in
            model.reload()
        }
        // Restarted on every keystroke and change; the previous search is
        // cancelled in Rust.
        .task(id: SearchKey(query: query, changes: changes)) {
            guard searching else { results = nil; return }
            if let found = await model.search(query) {
                results = found
                resultsFor = query
            }
        }
    }

    private var addRow: some View {
        HStack {
            TextField("Add an item", text: $newTitle)
                .focused($addFocused)
                .submitLabel(.done)
                .onSubmit { add() }
                .accessibilityIdentifier("checklist.newTitle")
            Button {
                add()
            } label: {
                Image(systemName: "plus.circle.fill").font(.title2)
            }
            .disabled(newTitle.trimmingCharacters(in: .whitespaces).isEmpty)
            .accessibilityLabel("Add item")
            .accessibilityIdentifier("checklist.add")
        }
    }

    private func row(_ item: Item) -> some View {
        Button {
            Task { await model.toggle(item) }
        } label: {
            HStack {
                Image(systemName: item.done ? "checkmark.circle.fill" : "circle")
                    // `.tint` is the theme's accent (forgeTheme sets it).
                    .foregroundStyle(item.done ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                    .imageScale(.large)
                VStack(alignment: .leading, spacing: 2) {
                    Text(item.title)
                        .strikethrough(item.done)
                        .foregroundStyle(item.done ? .secondary : .primary)
                    if let due = dueText(item) {
                        dueLabel(due, overdue: isOverdue(item))
                    }
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(item.title)
        .accessibilityValue(([item.done ? "Done" : "Not done"] + [dueText(item)].compactMap { $0 }).joined(separator: ", "))
        .accessibilityHint("Double-tap to mark it \(item.done ? "not done" : "done").")
        .accessibilityIdentifier("checklist.item.\(item.id)")
        .forgeRowActions {
            Button(role: .destructive) {
                Task { await model.remove(item) }
            } label: {
                Label("Delete", systemImage: "trash")
            }
            .accessibilityIdentifier("checklist.delete.\(item.id)")
            Button {
                editing = Editing(item: item)
            } label: {
                Label("Rename", systemImage: "pencil")
            }
            .accessibilityIdentifier("checklist.rename.\(item.id)")
            Button {
                dueEditing = Editing(item: item)
            } label: {
                Label("Due Date", systemImage: "calendar")
            }
            .accessibilityIdentifier("checklist.due.\(item.id)")
        }
    }

    @ViewBuilder
    private func agendaSection(_ title: LocalizedStringKey, _ items: [Item], id: String) -> some View {
        if !items.isEmpty {
            Section {
                ForEach(items, id: \.id) { item in
                    row(item)
                }
            } header: {
                Text(title).accessibilityIdentifier("checklist.agenda.\(id)")
            }
        }
    }

    @ViewBuilder
    private func dueLabel(_ text: String, overdue: Bool) -> some View {
        let label = Label(text, systemImage: "calendar").font(.footnote)
        if overdue {
            label.forgeForeground(.warning)
        } else {
            label.foregroundStyle(.secondary)
        }
    }

    /// Late: not done, and due before today.
    private func isOverdue(_ item: Item) -> Bool {
        guard let due = item.due, !item.done else { return false }
        return due < model.today
    }

    /// "Due today", "Due tomorrow", "Overdue: 20 Nov", "Due 24 Nov".
    private func dueText(_ item: Item) -> String? {
        guard let due = item.due, let date = ChecklistDay.date(from: due) else { return nil }
        if due == model.today { return String(localized: "Due today") }
        let calendar = Calendar.current
        if let tomorrow = calendar.date(byAdding: .day, value: 1, to: calendar.startOfDay(for: Date())),
           ChecklistDay.string(from: tomorrow) == due {
            return String(localized: "Due tomorrow")
        }
        let sameYear = calendar.component(.year, from: date) == calendar.component(.year, from: Date())
        let day = sameYear
            ? date.formatted(.dateTime.day().month(.abbreviated))
            : date.formatted(.dateTime.day().month(.abbreviated).year())
        return isOverdue(item) ? String(localized: "Overdue: \(day)") : String(localized: "Due \(day)")
    }

    @ViewBuilder
    private var emptyState: some View {
        if searching && resultsFor == query && results?.isEmpty == true {
            ForgeStateView.noResults(for: query)
        } else if showingAgenda && model.agenda.overdue.isEmpty && model.agenda.today.isEmpty
            && model.agenda.upcoming.isEmpty
        {
            ForgeStateView.empty(
                "Nothing due",
                systemImage: "calendar",
                description: "Give an item a due date from its menu to see it here."
            )
            .allowsHitTesting(false)
        } else if !searching && !showingAgenda && model.items.isEmpty {
            ForgeStateView.empty(
                "Nothing to do",
                systemImage: "checklist",
                description: "Add the first item above."
            )
            .allowsHitTesting(false)
        }
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .primaryAction) {
            Menu {
                Button {
                    Task {
                        export = await model.exportText()
                    }
                } label: {
                    Label("Share as Text", systemImage: "square.and.arrow.up")
                }
                .disabled(model.items.isEmpty)
                .accessibilityIdentifier("checklist.share")
                Button {
                    Task {
                        export = await model.exportCSV()
                    }
                } label: {
                    Label("Share as Spreadsheet", systemImage: "tablecells")
                }
                .disabled(model.items.isEmpty)
                .accessibilityIdentifier("checklist.shareCSV")
                Button {
                    importing = true
                } label: {
                    Label("Import from Spreadsheet…", systemImage: "square.and.arrow.down")
                }
                .accessibilityIdentifier("checklist.importCSV")
                Button(role: .destructive) {
                    confirmingClear = true
                } label: {
                    Label("Clear Done Items", systemImage: "checkmark.circle.badge.xmark")
                }
                .disabled(model.summary.done == 0)
                .accessibilityIdentifier("checklist.clearDone")
            } label: {
                Label("More", systemImage: "ellipsis.circle")
            }
            .accessibilityIdentifier("checklist.more")
        }
    }

    private var importTitle: String {
        let added = imported?.added ?? 0
        return added == 1 ? "1 item added" : "\(added) items added"
    }

    /// The rows Rust skipped, the first few by line.
    private var importMessage: String {
        guard let skipped = imported?.skipped, !skipped.isEmpty else { return "" }
        let more = skipped.count > 5 ? "\nand \(skipped.count - 5) more" : ""
        return "\(skipped.count) skipped:\n" + skipped.prefix(5).joined(separator: "\n") + more
    }

    private func add() {
        let title = newTitle
        Task {
            if await model.add(title) {
                newTitle = ""
                addFocused = true
            }
        }
    }
}

private struct SearchKey: Equatable {
    let query: String
    let changes: UInt64
}

private struct Editing: Identifiable {
    let item: Item
    var id: UInt64 { item.id }
}

private enum Showing: Hashable {
    case all, agenda
}

/// Picks the day an item is due, or removes it. Shows a refusal itself: the
/// screen's alert can't present over a sheet.
private struct DueDateSheet: View {
    let model: ChecklistModel
    let item: Item

    @State private var day: Date
    @State private var saving = false
    @State private var failure: String?
    @Environment(\.dismiss) private var dismiss

    init(model: ChecklistModel, item: Item) {
        self.model = model
        self.item = item
        _day = State(initialValue: item.due.flatMap(ChecklistDay.date(from:)) ?? Date())
    }

    private var changed: Bool { ChecklistDay.string(from: day) != item.due }

    var body: some View {
        NavigationStack {
            Form {
                DatePicker("Due", selection: $day, displayedComponents: .date)
                    .datePickerStyle(.graphical)
                    .accessibilityIdentifier("due.date")
                if item.due != nil {
                    Section {
                        Button("Remove Due Date", role: .destructive) {
                            Task { await save(nil) }
                        }
                        .disabled(saving)
                        .accessibilityIdentifier("due.remove")
                    }
                }
            }
            .navigationTitle("Due Date")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .forgeFormToolbar(
                id: "due", isWorking: saving, hasChanges: changed, cancel: { dismiss() }
            ) { Task { await save(day) } }
            .alert(
                "Couldn't set the date",
                isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })
            ) {
                Button("OK", role: .cancel) {}
                    .accessibilityIdentifier("due.errorOK")
            } message: {
                Text(failure ?? "")
            }
        }
    }

    private func save(_ date: Date?) async {
        saving = true
        defer { saving = false }
        do {
            try await model.setDue(item, to: date)
            dismiss()
        } catch {
            failure = Checklist.message(for: error)
        }
    }
}
#endif
