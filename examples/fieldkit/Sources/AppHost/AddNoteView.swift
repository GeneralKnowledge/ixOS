@preconcurrency import FieldKitFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// The Add Note sheet: a text note on a trip. On a recording trip it is
/// pinned where you are.
struct AddNoteView: View {
    let model: TripsModel
    let tripID: Int64

    @State private var text = ""
    @State private var saving = ForgeSubmission()
    @FocusState private var isFocused: Bool
    @Environment(\.dismiss) private var dismiss

    private var trimmed: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        NavigationStack {
            Form {
                TextField("What did you see?", text: $text, axis: .vertical)
                    .lineLimit(4...)
                    .focused($isFocused)
                    .accessibilityIdentifier("note.text")
            }
            .navigationTitle("New Note")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .forgeFormToolbar(
                id: "note", confirmTitle: "Add", canConfirm: !trimmed.isEmpty,
                submission: saving, hasChanges: !trimmed.isEmpty, cancel: { dismiss() }
            ) { Task { await add() } }
            .onAppear { isFocused = true }
        }
    }

    private func add() async {
        let text = trimmed
        if await saving.run({ try await model.addNote(to: tripID, text: text) }) {
            dismiss()
        }
    }
}
