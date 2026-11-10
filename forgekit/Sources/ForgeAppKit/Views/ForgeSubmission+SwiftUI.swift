#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

extension View {
    /// ForgeUI's form toolbar (Cancel, and a confirm button that turns into
    /// a spinner) driven by a `ForgeSubmission`: busy while it runs, its
    /// failure shown as an alert. Identifiers `<id>.cancel`, `<id>.confirm`.
    ///
    ///     .forgeFormToolbar(id: "bookForm", confirmTitle: "Add", canConfirm: model.canSave,
    ///                       submission: model.saving, cancel: { dismiss() }) {
    ///         Task { if await model.save() { dismiss() } }
    ///     }
    public func forgeFormToolbar(
        id: String,
        confirmTitle: LocalizedStringKey = "Save",
        canConfirm: Bool = true,
        submission: ForgeSubmission,
        cancel: @escaping () -> Void,
        confirm: @escaping () -> Void
    ) -> some View {
        forgeFormToolbar(
            id: id,
            confirmTitle: confirmTitle,
            canConfirm: canConfirm,
            isWorking: submission.isWorking,
            cancel: cancel,
            confirm: confirm
        )
        .forgeAlert(failure: Bindable(submission).failure)
    }
}
#endif
