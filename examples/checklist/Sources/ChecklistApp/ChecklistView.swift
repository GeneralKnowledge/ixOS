#if canImport(SwiftUI)
import ForgeFFI
import ForgeKit
import SwiftUI

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
                            .accessibilityIdentifier("checklist.summary")
                    }
                }
            }
            .overlay { emptyState }
            .navigationTitle("Checklist")
            .searchable(text: $query, prompt: "Search items")
            .toolbar { toolbar }
            .sheet(item: $editing) { editing in
                RenameSheet(item: editing.item) { title in await model.rename(editing.item, to: title) }
            }
            .sheet(item: $export) { file in
                ShareSheet(url: file.url)
            }
            .alert(
                "Couldn't do that",
                isPresented: Binding(get: { model.errorMessage != nil }, set: { if !$0 { model.errorMessage = nil } })
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
                    .foregroundStyle(item.done ? Color.accentColor : Color.secondary)
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
            ContentUnavailableView.search(text: query)
        } else if !searching && model.items.isEmpty {
            ContentUnavailableView(
                "Nothing to do",
                systemImage: "checklist",
                description: Text("Add the first item above.")
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
                        if let url = await model.exportText() { export = ExportFile(url: url) }
                    }
                } label: {
                    Label("Share as Text", systemImage: "square.and.arrow.up")
                }
                .disabled(model.items.isEmpty)
                .accessibilityIdentifier("checklist.share")
                Button(role: .destructive) {
                    Task { await model.clearDone() }
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
    let save: (String) async -> Bool
    @State private var title: String
    @Environment(\.dismiss) private var dismiss

    init(item: Item, save: @escaping (String) async -> Bool) {
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
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("rename.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        Task { if await save(title) { dismiss() } }
                    }
                    .accessibilityIdentifier("rename.save")
                }
            }
        }
        .presentationDetents([.medium])
    }
}

private struct ShareSheet: View {
    let url: URL
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(spacing: 16) {
                Image(systemName: "doc.text").font(.largeTitle).foregroundStyle(.secondary)
                Text("Your checklist is ready to share as text.")
                    .multilineTextAlignment(.center)
                ShareLink(item: url) {
                    Label("Share", systemImage: "square.and.arrow.up")
                }
                .buttonStyle(.borderedProminent)
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
    let url: URL
    var id: URL { url }
}
#endif
