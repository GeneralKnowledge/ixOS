/// What a form sheet's toolbar allows, from the form's state. Plain values,
/// so the rules are checked on Linux; `forgeFormToolbar` draws them.
struct ForgeFormToolbarState: Hashable, Sendable {
    var canConfirm: Bool
    var isWorking: Bool
    var hasChanges: Bool

    /// Cancel is always there; it waits while the work runs.
    var cancelEnabled: Bool { !isWorking }
    /// The confirm button is a spinner while the work runs.
    var showsSpinner: Bool { isWorking }
    var confirmEnabled: Bool { canConfirm && !isWorking }
    /// A swipe down can't close the sheet while it saves, or while it holds
    /// changes the person would lose. Cancel still closes it on purpose.
    var blocksSwipeToDismiss: Bool { isWorking || hasChanges }
}

#if canImport(SwiftUI)
import SwiftUI

extension View {
    /// The Cancel and confirm buttons of a form in a sheet. The confirm
    /// button turns into a spinner while `isWorking`. The sheet can't be
    /// swiped away while it is working, or while `hasChanges` (what the person
    /// typed would be lost); Cancel still closes it. Buttons get the
    /// identifiers `<id>.cancel` and `<id>.confirm`, the spinner `<id>.working`.
    ///
    ///     .forgeFormToolbar(id: "taskForm", confirmTitle: "Add", canConfirm: model.canSave,
    ///                       isWorking: model.isSaving, hasChanges: model.hasChanges,
    ///                       cancel: { dismiss() }, confirm: save)
    public func forgeFormToolbar(
        id: String,
        confirmTitle: LocalizedStringKey = "Save",
        canConfirm: Bool = true,
        isWorking: Bool = false,
        hasChanges: Bool = false,
        cancel: @escaping () -> Void,
        confirm: @escaping () -> Void
    ) -> some View {
        let state = ForgeFormToolbarState(canConfirm: canConfirm, isWorking: isWorking, hasChanges: hasChanges)
        return toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel", action: cancel)
                    .disabled(!state.cancelEnabled)
                    .accessibilityIdentifier("\(id).cancel")
            }
            ToolbarItem(placement: .confirmationAction) {
                if state.showsSpinner {
                    ProgressView()
                        .accessibilityLabel(Text("Saving"))
                        .accessibilityIdentifier("\(id).working")
                } else {
                    Button(action: confirm) { Text(confirmTitle).fontWeight(.semibold) }
                        .disabled(!state.confirmEnabled)
                        .accessibilityIdentifier("\(id).confirm")
                }
            }
        }
        .forgeSwipeGuard(state.blocksSwipeToDismiss)
    }
}

#endif
