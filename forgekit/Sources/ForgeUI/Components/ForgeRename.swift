#if canImport(SwiftUI)
import SwiftUI

extension View {
    /// Renames the item in `item` in a small sheet (or names a new one:
    /// `confirmTitle: "Create"` with an empty `name`). `save` returns nil once
    /// the name is saved, and the sheet closes; otherwise it returns why the
    /// name was refused (empty, taken, too long), which shows under the
    /// field while the sheet stays open with what the person typed. An alert
    /// with a text field can do neither.
    ///
    ///     .forgeRename("Rename Item", item: $renaming, name: \.title, id: "rename") { item, title in
    ///         await model.rename(item, to: title) ? nil : model.takeError()
    ///     }
    ///
    /// With ForgeAppKit, `save` can throw instead, and the error's
    /// `ForgeFailure` message is the reason. Identifiers: `<id>.name` for
    /// the field, `<id>.cancel` and `<id>.confirm` for the buttons,
    /// `<id>.error` for the reason.
    public func forgeRename<Item: Identifiable>(
        _ title: LocalizedStringKey,
        item: Binding<Item?>,
        name: @escaping (Item) -> String,
        id: String,
        message: LocalizedStringKey? = nil,
        confirmTitle: LocalizedStringKey = "Save",
        save: @escaping (Item, String) async -> String?
    ) -> some View {
        sheet(item: item) { current in
            ForgeRenameSheet(
                title: title, original: name(current), id: id, message: message, confirmTitle: confirmTitle
            ) { newName in
                await save(current, newName)
            }
        }
    }

    /// Renames one thing (this device, the account) in a small sheet.
    /// `save` returns nil when saved, otherwise the reason it wasn't.
    public func forgeRename(
        _ title: LocalizedStringKey,
        isPresented: Binding<Bool>,
        name: String,
        id: String,
        message: LocalizedStringKey? = nil,
        confirmTitle: LocalizedStringKey = "Save",
        save: @escaping (String) async -> String?
    ) -> some View {
        sheet(isPresented: isPresented) {
            ForgeRenameSheet(
                title: title, original: name, id: id, message: message, confirmTitle: confirmTitle, save: save)
        }
    }
}

/// The sheet behind `forgeRename`: one field, Cancel and Save, and the
/// reason under the field when the name is refused.
struct ForgeRenameSheet: View {
    let title: LocalizedStringKey
    let original: String
    let id: String
    let message: LocalizedStringKey?
    let confirmTitle: LocalizedStringKey
    let save: (String) async -> String?

    @State private var name: String
    @State private var refusal: String?
    @State private var saving = false
    @FocusState private var focused: Bool
    @Environment(\.dismiss) private var dismiss

    init(
        title: LocalizedStringKey, original: String, id: String, message: LocalizedStringKey?,
        confirmTitle: LocalizedStringKey, save: @escaping (String) async -> String?
    ) {
        self.title = title
        self.original = original
        self.id = id
        self.message = message
        self.confirmTitle = confirmTitle
        self.save = save
        _name = State(initialValue: original)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $name)
                        .focused($focused)
                        .submitLabel(.done)
                        .onSubmit(confirm)
                        .onChange(of: name) { refusal = nil }
                        .accessibilityIdentifier("\(id).name")
                } footer: {
                    if let refusal {
                        Text(verbatim: refusal)
                            .forgeForeground(.danger)
                            .accessibilityIdentifier("\(id).error")
                    } else if let message {
                        Text(message)
                    }
                }
            }
            .navigationTitle(title)
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            // The app's rules (in Rust) check the name, so Save stays enabled
            // and a refusal shows under the field. A changed name keeps the
            // sheet from being swiped away.
            .forgeFormToolbar(
                id: id,
                confirmTitle: confirmTitle,
                isWorking: saving,
                hasChanges: name != original,
                cancel: { dismiss() },
                confirm: confirm
            )
            .onAppear { focused = true }
        }
        .presentationDetents([.medium])
    }

    private func confirm() {
        guard !saving else { return }
        saving = true
        let newName = name
        Task {
            let reason = await save(newName)
            saving = false
            if let reason {
                refusal = reason
            } else {
                dismiss()
            }
        }
    }
}
#endif
