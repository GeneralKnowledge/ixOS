#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

extension View {
    /// ForgeUI's rename sheet with a `save` that throws: the error's
    /// `ForgeFailure` message shows under the field.
    ///
    ///     .forgeRename("Rename Trip", item: $renaming, name: \.title, id: "renameTrip") { trip, title in
    ///         try await model.rename(trip, to: title)
    ///     }
    public func forgeRename<Item: Identifiable>(
        _ title: LocalizedStringKey,
        item: Binding<Item?>,
        name: @escaping (Item) -> String,
        id: String,
        message: LocalizedStringKey? = nil,
        confirmTitle: LocalizedStringKey = "Save",
        save: @escaping (Item, String) async throws -> Void
    ) -> some View {
        forgeRename(title, item: item, name: name, id: id, message: message, confirmTitle: confirmTitle) { current, newName in
            await ForgeRenameRefusal.reason { try await save(current, newName) }
        }
    }

    /// ForgeUI's rename sheet for one thing, with a `save` that throws.
    public func forgeRename(
        _ title: LocalizedStringKey,
        isPresented: Binding<Bool>,
        name: String,
        id: String,
        message: LocalizedStringKey? = nil,
        confirmTitle: LocalizedStringKey = "Save",
        save: @escaping (String) async throws -> Void
    ) -> some View {
        forgeRename(title, isPresented: isPresented, name: name, id: id, message: message, confirmTitle: confirmTitle) { newName in
            await ForgeRenameRefusal.reason { try await save(newName) }
        }
    }
}
#endif
