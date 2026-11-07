import SwiftUI

/// The New Note sheet: a form with Cancel and Add. The keyboard opens at
/// once, Add stays disabled until there is text, and a swipe down cannot
/// throw away what you typed.
struct ComposeNoteView: View {
    let model: NotesModel

    @State private var text = ""
    @State private var isSaving = false
    @State private var error: String?
    @FocusState private var isFocused: Bool
    @Environment(\.dismiss) private var dismiss

    private var trimmed: String {
        text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    var body: some View {
        NavigationStack {
            Form {
                TextField("Note", text: $text, axis: .vertical)
                    .lineLimit(6...)
                    .focused($isFocused)
            }
            .navigationTitle("New Note")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if isSaving {
                        ProgressView()
                    } else {
                        Button("Add") { Task { await add() } }
                            .disabled(trimmed.isEmpty)
                    }
                }
            }
            .interactiveDismissDisabled(!trimmed.isEmpty)
            .errorAlert("Couldn't Add Note", message: $error)
            .onAppear { isFocused = true }
        }
    }

    private func add() async {
        isSaving = true
        defer { isSaving = false }
        do {
            try await model.add(text: trimmed)
            dismiss()
        } catch {
            // Stay open with the text intact so it can be fixed or retried.
            self.error = NotesModel.describe(error)
        }
    }
}
