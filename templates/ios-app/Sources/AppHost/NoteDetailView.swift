@preconcurrency import ForgeFFI
import SwiftUI

/// Screen 2: one note, edited in place. Changes are saved when you tap Done
/// or leave the screen, like Notes. Deleting asks first.
struct NoteDetailView: View {
    let model: NotesModel
    let noteID: UInt64

    @State private var text = ""
    @State private var isConfirmingDelete = false
    @State private var isDeleting = false
    @FocusState private var isEditing: Bool
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        if let note = model.note(id: noteID) {
            editor(for: note)
        } else {
            // Deleted elsewhere (for example from Rust) while open.
            ContentUnavailableView("Note Deleted", systemImage: "trash")
        }
    }

    private func editor(for note: Note) -> some View {
        TextEditor(text: $text)
            .focused($isEditing)
            .padding(.horizontal)
            .scrollDismissesKeyboard(.interactively)
            .navigationTitle(Text(note.createdAt, format: .dateTime.day().month().year()))
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                if isEditing {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") {
                            isEditing = false
                            save(note)
                        }
                    }
                }
                ToolbarItem(placement: .primaryAction) {
                    Menu("More", systemImage: "ellipsis.circle") {
                        ShareLink(item: text)
                        Button("Delete", systemImage: "trash", role: .destructive) {
                            isConfirmingDelete = true
                        }
                    }
                }
            }
            .confirmationDialog(
                "Delete this note?", isPresented: $isConfirmingDelete, titleVisibility: .visible
            ) {
                Button("Delete Note", role: .destructive) {
                    isDeleting = true
                    dismiss()
                    Task { await model.delete(note) }
                }
            } message: {
                Text("This can't be undone.")
            }
            .onAppear { text = note.text }
            .onDisappear {
                if !isDeleting { save(note) }
            }
    }

    /// Saves if the text changed. Blank text is not a valid note (Rust
    /// refuses it), so clearing a note keeps its last saved text.
    private func save(_ note: Note) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, trimmed != note.text else { return }
        // A failure shows in the list's alert.
        Task { _ = await model.update(note, text: trimmed) }
    }
}
