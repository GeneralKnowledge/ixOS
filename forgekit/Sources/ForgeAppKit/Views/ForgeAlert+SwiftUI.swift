#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

extension View {
    /// Presents the model's alert or confirmation dialog. A tapped button's
    /// action goes to `perform`; any button, or dismissing, sets the alert
    /// back to nil.
    ///
    ///     .forgeAlert($model.alert) { action in Task { await model.handle(action) } }
    ///
    /// One per presented screen: a sheet needs its own, because a view
    /// under a sheet cannot present.
    public func forgeAlert<Action>(
        _ alert: Binding<ForgeAlert<Action>?>,
        perform: @escaping (Action) -> Void
    ) -> some View {
        modifier(ForgeAlertModifier(alert: alert, perform: perform))
    }

    /// Presents an alert that only informs (its buttons carry no actions).
    public func forgeAlert(_ alert: Binding<ForgeAlert<Never>?>) -> some View {
        modifier(ForgeAlertModifier(alert: alert, perform: { _ in }))
    }

    /// Presents `failure` as an error alert with OK, and clears it when
    /// dismissed. For `ForgeSubmission.failure` and other lone failures.
    public func forgeAlert(failure: Binding<ForgeFailure?>) -> some View {
        forgeAlert(Binding<ForgeAlert<Never>?>(
            get: { failure.wrappedValue.map { ForgeAlert<Never>.failure($0) } },
            set: { if $0 == nil { failure.wrappedValue = nil } }
        ))
    }
}

private struct ForgeAlertModifier<Action>: ViewModifier {
    @Binding var alert: ForgeAlert<Action>?
    let perform: (Action) -> Void

    private func isPresented(_ style: ForgeAlert<Action>.Style) -> Binding<Bool> {
        Binding(
            get: { alert?.style == style },
            set: { if !$0, alert?.style == style { alert = nil } }
        )
    }

    func body(content: Content) -> some View {
        // The title is the app's text (often with data in it), shown as
        // written, like Text(String).
        content
            .alert(
                Text(verbatim: alert?.style == .alert ? alert?.title ?? "" : ""),
                isPresented: isPresented(.alert),
                presenting: alert
            ) { presented in
                buttons(presented)
            } message: { presented in
                if let message = presented.message { Text(verbatim: message) }
            }
            .confirmationDialog(
                Text(verbatim: alert?.style == .confirmationDialog ? alert?.title ?? "" : ""),
                isPresented: isPresented(.confirmationDialog),
                titleVisibility: .visible,
                presenting: alert
            ) { presented in
                buttons(presented)
            } message: { presented in
                if let message = presented.message { Text(verbatim: message) }
            }
    }

    @ViewBuilder
    private func buttons(_ presented: ForgeAlert<Action>) -> some View {
        ForEach(Array(presented.buttons.enumerated()), id: \.offset) { _, button in
            Button(role: button.role.map(Self.role)) {
                alert = nil
                if let action = button.action { perform(action) }
            } label: {
                Text(verbatim: button.title)
            }
            .accessibilityIdentifier(button.identifier)
        }
    }

    private static func role(_ role: ForgeAlertButton<Action>.Role) -> ButtonRole {
        switch role {
        case .cancel: .cancel
        case .destructive: .destructive
        }
    }
}
#endif
