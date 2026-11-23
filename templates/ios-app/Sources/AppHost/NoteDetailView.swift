@preconcurrency import FORGE_MODULE_NAMEFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Screen 2: one note, edited in place. Changes are saved when you tap Done,
/// leave the screen or leave the app, like Notes. Deleting asks first.
struct NoteDetailView: View {
    let model: NotesModel
    let noteID: Int64

    @State private var text = ""
    @State private var isConfirmingDelete = false
    @State private var isDeleting = false
    /// The failed save's text this editor loaded, if any. Matching the
    /// saved text drops a retained draft only when it is this one, so a
    /// later save that fails keeps its text.
    @State private var loadedDraft: String?
    @FocusState private var isEditing: Bool
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        if let note = model.note(id: noteID) {
            editor(for: note)
        } else {
            // Deleted elsewhere (for example from Rust) while open.
            ForgeStateView.empty("Note Deleted", systemImage: "trash")
        }
    }

    private func editor(for note: Note) -> some View {
        VStack(spacing: 0) {
            if model.unsavedText[note.id] != nil {
                // An earlier save failed (the list showed why). The text is
                // back in the editor; this says so and offers the saved one.
                ForgeNotice(
                    "Your last changes weren't saved. They're below; leaving the note tries again.",
                    systemImage: "exclamationmark.triangle",
                    action: ForgeStateAction("Use Saved Version", identifier: "note.discardUnsaved") {
                        model.discardUnsaved(note.id)
                        loadedDraft = nil
                        text = (model.note(id: note.id) ?? note).text
                    }
                )
                .padding([.horizontal, .top])
            }
            TextEditor(text: $text)
                .focused($isEditing)
                .accessibilityIdentifier("note.text")
                .padding(.horizontal)
        }
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
                        .accessibilityIdentifier("note.done")
                    }
                }
                ToolbarItem(placement: .primaryAction) {
                    Menu("More", systemImage: "ellipsis.circle") {
                        ShareLink(item: text)
                            .accessibilityIdentifier("note.share")
                        Button("Delete", systemImage: "trash", role: .destructive) {
                            isConfirmingDelete = true
                        }
                        .accessibilityIdentifier("note.delete")
                    }
                    .accessibilityIdentifier("note.more")
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
                .accessibilityIdentifier("note.confirmDelete")
            } message: {
                Text("This can't be undone.")
            }
            .onAppear {
                loadedDraft = model.unsavedText[note.id]
                text = model.unsavedText[note.id] ?? note.text
            }
            // An earlier save that fails after this editor opened: if the
            // person hasn't changed the text yet, show the draft so leaving
            // tries it again. Newer edits are kept and saved instead.
            .onChange(of: model.unsavedText[note.id]) { _, draft in
                // Untouched: still the saved text, or the draft it loaded.
                let untouched = text == (model.note(id: note.id) ?? note).text || text == loadedDraft
                guard let draft, draft != loadedDraft, untouched else { return }
                text = draft
                loadedDraft = draft
            }
            // Saves when the screen closes and when the app leaves the
            // screen (iOS may end it later without warning).
            .forgeSaveOnLeave {
                if !isDeleting { save(note) }
            }
    }

    /// Saves if the text changed. Blank text is not a valid note (Rust
    /// refuses it), so clearing a note keeps its last saved text.
    private func save(_ note: Note) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        // Compare with what Rust holds now: an earlier save (leaving the
        // app) may already have stored this text.
        let saved = model.note(id: note.id) ?? note
        guard !trimmed.isEmpty, trimmed != saved.text else {
            // Back to the saved text: nothing is left unsaved.
            if trimmed == saved.text, let draft = model.unsavedText[note.id], draft == loadedDraft {
                model.discardUnsaved(note.id)
            }
            return
        }
        // A failure shows in the list's alert, and the model keeps the text
        // (`unsavedText`) so it isn't lost with this screen.
        Task { _ = await model.update(saved, text: trimmed) }
    }
}
