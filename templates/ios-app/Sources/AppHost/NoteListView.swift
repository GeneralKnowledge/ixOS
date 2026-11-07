@preconcurrency import ForgeFFI
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
                    .swipeActions {
                        Button("Delete", systemImage: "trash", role: .destructive) {
                            Task { await model.delete(note) }
                        }
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
                    Button("Settings", systemImage: "gear") { isShowingSettings = true }
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("New Note", systemImage: "square.and.pencil") { isComposing = true }
                }
            }
            .sheet(isPresented: $isComposing) {
                ComposeNoteView(model: model)
            }
            .sheet(isPresented: $isShowingSettings) {
                SettingsView(model: model)
            }
            .errorAlert("Something Went Wrong", message: $model.actionError)
            .sensoryFeedback(.success, trigger: model.saveCount)
        }
    }

    @ViewBuilder private var emptyState: some View {
        if model.notes.isEmpty {
            ContentUnavailableView {
                Label("No Notes", systemImage: "note.text")
            } description: {
                Text("Notes you write are saved on this device.")
            } actions: {
                Button("New Note") { isComposing = true }
            }
        } else if model.visibleNotes.isEmpty {
            ContentUnavailableView.search(text: model.searchText)
        }
    }
}

private struct NoteRow: View {
    let note: Note

    var body: some View {
        VStack(alignment: .leading) {
            Text(note.title)
                .font(.headline)
                .lineLimit(1)
            HStack {
                Text(note.createdAt, format: .dateTime.day().month().year())
                Text(note.preview ?? "No additional text")
                    .lineLimit(1)
            }
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
        // VoiceOver reads the row as one element.
        .accessibilityElement(children: .combine)
    }
}
