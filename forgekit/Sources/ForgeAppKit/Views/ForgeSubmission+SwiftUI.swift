#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

extension View {
    /// ForgeUI's form toolbar (Cancel, and a confirm button that turns into
    /// a spinner) driven by a `ForgeSubmission`: busy while it runs, its
    /// failure shown as an alert. With `hasChanges`, a swipe down can't
    /// throw away what the person typed. Identifiers `<id>.cancel`, `<id>.confirm`.
    ///
    ///     .forgeFormToolbar(id: "bookForm", confirmTitle: "Add", canConfirm: model.canSave,
    ///                       submission: model.saving, hasChanges: model.hasChanges,
    ///                       cancel: { dismiss() }) {
    ///         Task { if await model.save() { dismiss() } }
    ///     }
    public func forgeFormToolbar(
        id: String,
        confirmTitle: LocalizedStringKey = "Save",
        canConfirm: Bool = true,
        submission: ForgeSubmission,
        hasChanges: Bool = false,
        cancel: @escaping () -> Void,
        confirm: @escaping () -> Void
    ) -> some View {
        forgeFormToolbar(
            id: id,
            confirmTitle: confirmTitle,
            canConfirm: canConfirm,
            isWorking: submission.isWorking,
            hasChanges: hasChanges,
            cancel: cancel,
            confirm: confirm
        )
        .forgeAlert(failure: Bindable(submission).failure)
    }
}
#endif
