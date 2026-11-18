@preconcurrency import FORGE_MODULE_NAMEFFI
import ForgeAppKit
import SwiftUI

/// The New Note sheet: a form with Cancel and Add. The keyboard opens at
/// once, Add stays disabled until there is text, and a swipe down cannot
/// throw away what you typed.
struct ComposeNoteView: View {
    let model: NotesModel

    @State private var text = ""
    /// Add's work: busy while it runs, its failure shown here (ForgeAppKit).
    @State private var saving = ForgeSubmission()
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
                    .accessibilityIdentifier("compose.text")
            }
            .navigationTitle("New Note")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            // Cancel, and Add that turns into a spinner while saving; a
            // failure is shown as an alert, and a swipe down can't throw
            // away what was typed (ForgeUI and ForgeAppKit).
            .forgeFormToolbar(
                id: "compose", confirmTitle: "Add", canConfirm: !trimmed.isEmpty,
                submission: saving, hasChanges: !trimmed.isEmpty,
                cancel: { dismiss() }, confirm: { Task { await add() } })
            .onAppear { isFocused = true }
        }
    }

    /// On failure the sheet stays open with the text intact, so it can be
    /// fixed or retried.
    private func add() async {
        let text = trimmed
        if await saving.run({ try await model.add(text: text) }) {
            dismiss()
        }
    }
}
