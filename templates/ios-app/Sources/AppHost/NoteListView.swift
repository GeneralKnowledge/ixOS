@preconcurrency import ForgeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Screen 1: every note, newest first, with search, swipe-to-delete and a
/// context menu. Tapping a note pushes its editor; the toolbar opens the
/// compose and settings sheets.
struct NoteListView: View {
    @Bindable var model: NotesModel
    @State private var isComposing = false
    @State private var isShowingSettings = false

    var body: some View {
        NavigationStack {
            List {
                ForEach(model.visibleNotes) { note in
                    NavigationLink(value: note.id) {
                        NoteRow(note: note)
                    }
                    .accessibilityIdentifier("notes.row")
                    .swipeActions {
                        Button("Delete", systemImage: "trash", role: .destructive) {
                            Task { await model.delete(note) }
                        }
                        .accessibilityIdentifier("notes.delete")
                    }
                    .contextMenu {
                        ShareLink(item: note.text)
                        Button("Delete", systemImage: "trash", role: .destructive) {
                            Task { await model.delete(note) }
                        }
                    }
                }
            }
            .overlay { emptyState }
            .navigationTitle("FORGE_APP_NAME")
            .navigationDestination(for: UInt64.self) { id in
                NoteDetailView(model: model, noteID: id)
            }
            .searchable(text: $model.searchText, prompt: "Search")
            .toolbar {
                ToolbarItem(placement: .navigation) {
                    Button("Settings", systemImage: "gearshape") { isShowingSettings = true }
                        .accessibilityIdentifier("notes.settings")
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("New Note", systemImage: "square.and.pencil") { isComposing = true }
                        .accessibilityIdentifier("notes.add")
                }
            }
            .sheet(isPresented: $isComposing) {
                ComposeNoteView(model: model)
            }
            .sheet(isPresented: $isShowingSettings) {
                SettingsView(model: model)
            }
            .forgeAlert($model.alert)
            .sensoryFeedback(.success, trigger: model.saveCount)
        }
    }

    @ViewBuilder private var emptyState: some View {
        if model.notes.isEmpty {
            ForgeStateView.empty(
                "No Notes", systemImage: "note.text",
                description: "Notes you write are saved on this device.",
                action: ForgeStateAction("New Note", identifier: "notes.addFirst") { isComposing = true }
            )
        } else if model.visibleNotes.isEmpty {
            ForgeStateView.noResults(for: model.searchText)
        }
    }
}

private struct NoteRow: View {
    let note: Note
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        // At accessibility text sizes there is no room for the date and the
        // preview side by side, so they stack and the title may wrap.
        let large = dynamicTypeSize.isAccessibilitySize
        let details = large ? AnyLayout(VStackLayout(alignment: .leading)) : AnyLayout(HStackLayout())
        VStack(alignment: .leading) {
            Text(note.title)
                .font(.headline)
                .lineLimit(large ? 3 : 1)
            details {
                Text(note.createdAt, format: .dateTime.day().month().year())
                Text(note.preview ?? "No additional text")
                    .lineLimit(large ? 2 : 1)
            }
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
        // VoiceOver reads the row as one element.
        .accessibilityElement(children: .combine)
    }
}
