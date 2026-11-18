#if canImport(SwiftUI)
import ChecklistFFI
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
    @State private var editing: Editing?
    @State private var export: ExportFile?
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

    var body: some View {
        NavigationStack {
            List {
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
            .overlay { emptyState }
            .navigationTitle("Checklist")
            .searchable(text: $query, prompt: "Search items")
            .toolbar { toolbar }
            // The sheet shows a failed rename itself: an alert from this
            // screen can't present over it (friction log A39).
            .sheet(item: $editing) { editing in
                RenameSheet(item: editing.item) { title in
                    if await model.rename(editing.item, to: title) { return nil }
                    let reason = model.errorMessage
                    model.errorMessage = nil
                    return reason ?? String(localized: "The item couldn't be renamed.")
                }
            }
            .sheet(item: $export) { file in
                ShareSheet(file: file)
            }
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
            // Held back while the Rename sheet is up; it shows once the
            // sheet closes.
            .alert(
                "Couldn't do that",
                isPresented: Binding(get: { editing == nil && model.errorMessage != nil }, set: { if !$0 { model.errorMessage = nil } })
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
        // Restarted on every keystroke and change; the previous search is
        // cancelled in Rust.
        .task(id: SearchKey(query: query, changes: changes)) {
            guard searching else { results = nil; return }
            if let found = await model.search(query) { results = found }
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
                Text(item.title)
                    .strikethrough(item.done)
                    .foregroundStyle(item.done ? .secondary : .primary)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(item.title)
        .accessibilityValue(item.done ? "Done" : "Not done")
        .accessibilityHint("Double-tap to mark it \(item.done ? "not done" : "done").")
        .accessibilityIdentifier("checklist.item.\(item.id)")
        .swipeActions {
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
        }
    }

    @ViewBuilder
    private var emptyState: some View {
        if searching && results?.isEmpty == true {
            ForgeStateView.noResults(for: query)
        } else if !searching && model.items.isEmpty {
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
                        if let url = await model.exportText() { export = ExportFile(url: url, kind: .text) }
                    }
                } label: {
                    Label("Share as Text", systemImage: "square.and.arrow.up")
                }
                .disabled(model.items.isEmpty)
                .accessibilityIdentifier("checklist.share")
                Button {
                    Task {
                        if let url = await model.exportCSV() { export = ExportFile(url: url, kind: .spreadsheet) }
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

private struct RenameSheet: View {
    let item: Item
    /// Saves the title; nil when saved, otherwise why it wasn't.
    let save: (String) async -> String?
    @State private var title: String
    @State private var failure: String?
    @State private var saving = false
    @Environment(\.dismiss) private var dismiss

    init(item: Item, save: @escaping (String) async -> String?) {
        self.item = item
        self.save = save
        _title = State(initialValue: item.title)
    }

    var body: some View {
        NavigationStack {
            Form {
                TextField("Title", text: $title)
                    .accessibilityIdentifier("rename.title")
            }
            .navigationTitle("Rename")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            // Rust checks the title, so Save stays enabled and a refusal
            // is shown below. A changed title keeps the sheet from being
            // swiped away.
            .forgeFormToolbar(
                id: "rename",
                canConfirm: true,
                isWorking: saving,
                hasChanges: title != item.title,
                cancel: { dismiss() },
                confirm: {
                    saving = true
                    Task {
                        let reason = await save(title)
                        saving = false
                        if let reason {
                            failure = reason
                        } else {
                            dismiss()
                        }
                    }
                }
            )
            .alert(
                "Couldn't Rename",
                isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })
            ) {
                Button("OK", role: .cancel) {}
                    .accessibilityIdentifier("rename.errorOK")
            } message: {
                Text(failure ?? "")
            }
        }
        .presentationDetents([.medium])
    }
}

private struct ShareSheet: View {
    let file: ExportFile
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(spacing: 16) {
                Image(systemName: file.kind == .text ? "doc.text" : "tablecells")
                    .font(.largeTitle).foregroundStyle(.secondary)
                Text(file.kind == .text
                    ? "Your checklist is ready to share as text."
                    : "Your checklist is ready to share as a spreadsheet.")
                    .multilineTextAlignment(.center)
                ShareLink(item: file.url) {
                    Label("Share", systemImage: "square.and.arrow.up")
                }
                .buttonStyle(.forgePrimary)
                .accessibilityIdentifier("share.send")
            }
            .padding()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("share.done")
                }
            }
        }
        .presentationDetents([.medium])
    }
}

private struct Editing: Identifiable {
    let item: Item
    var id: UInt64 { item.id }
}

private struct ExportFile: Identifiable {
    enum Kind { case text, spreadsheet }
    let url: URL
    let kind: Kind
    var id: URL { url }
}
#endif
