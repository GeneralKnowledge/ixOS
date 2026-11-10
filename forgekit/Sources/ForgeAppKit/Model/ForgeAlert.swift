// Alerts and confirmation dialogs as values the model owns. Foundation
// only, so a Linux test can say "tapping Delete asks for confirmation, and
// confirming deletes" without SwiftUI.

import Foundation

/// An alert or confirmation dialog, with buttons that carry the model's own
/// actions. The model sets it; the view presents it with `.forgeAlert(_:)`
/// and hands the tapped button's action back to the model.
///
///     enum LibraryAlert: Equatable { case delete(UInt64) }
///
///     model.alert = .confirm("Delete “Dune”?", message: "This can't be undone.",
///                            confirm: "Delete Book", action: .delete(book.id),
///                            identifier: "book.delete", destructive: true)
///
/// `Action` is `Never` for an alert that only informs (an error with OK).
public struct ForgeAlert<Action>: Identifiable {
    public enum Style: Hashable, Sendable {
        /// A centred alert: errors, and questions that need an answer.
        case alert
        /// An action sheet from the bottom: confirming a destructive action.
        case confirmationDialog
    }

    public let id: UUID
    public var title: String
    public var message: String?
    public var buttons: [ForgeAlertButton<Action>]
    public var style: Style

    public init(
        title: String,
        message: String? = nil,
        buttons: [ForgeAlertButton<Action>],
        style: Style = .alert
    ) {
        self.id = UUID()
        self.title = title
        self.message = message
        self.buttons = buttons
        self.style = style
    }

    /// The error alert: the failure's title and message, and OK
    /// (identifier `alert.ok`, which UI tests dismiss it by).
    public static func failure(_ failure: ForgeFailure) -> ForgeAlert {
        ForgeAlert(title: failure.title, message: failure.message, buttons: [.ok])
    }

    /// The error alert for `error`, or nil when the error only says the work
    /// was cancelled.
    public static func failure(_ error: Error) -> ForgeAlert? {
        ForgeFailure.isCancellation(error) ? nil : .failure(ForgeFailure(error))
    }

    /// Asks before doing something: the confirming button (identifier
    /// `<identifier>.confirm`) and Cancel. Destructive confirmations are a
    /// dialog from the bottom with a red button, as iOS does them.
    public static func confirm(
        _ title: String,
        message: String? = nil,
        confirm: String,
        action: Action,
        identifier: String,
        destructive: Bool = false
    ) -> ForgeAlert {
        ForgeAlert(
            title: title,
            message: message,
            buttons: [
                ForgeAlertButton(confirm, role: destructive ? .destructive : nil, action: action, identifier: "\(identifier).confirm"),
                ForgeAlertButton("Cancel", role: .cancel, identifier: "\(identifier).cancel"),
            ],
            style: destructive ? .confirmationDialog : .alert
        )
    }

    /// The action of the button titled `title`, as if it were tapped. For
    /// tests: `model.handle(model.alert!.action(for: "Delete Book")!)`.
    public func action(for title: String) -> Action? {
        buttons.first { $0.title == title }?.action
    }
}

/// Equal when everything but the identity matches, so tests can compare an
/// alert with the one they expect.
extension ForgeAlert: Equatable where Action: Equatable {
    public static func == (lhs: ForgeAlert, rhs: ForgeAlert) -> Bool {
        lhs.title == rhs.title && lhs.message == rhs.message && lhs.buttons == rhs.buttons && lhs.style == rhs.style
    }
}

extension ForgeAlert: Sendable where Action: Sendable {}

/// One button of a `ForgeAlert`.
public struct ForgeAlertButton<Action> {
    public enum Role: Hashable, Sendable {
        case cancel
        case destructive
    }

    public var title: String
    public var role: Role?
    /// What the model does when it is tapped; nil only dismisses.
    public var action: Action?
    /// The accessibility identifier UI tests tap it by.
    public var identifier: String

    public init(_ title: String, role: Role? = nil, action: Action? = nil, identifier: String) {
        self.title = title
        self.role = role
        self.action = action
        self.identifier = identifier
    }

    /// OK, which only dismisses (identifier `alert.ok`).
    public static var ok: ForgeAlertButton {
        ForgeAlertButton("OK", role: .cancel, identifier: "alert.ok")
    }
}

extension ForgeAlertButton: Equatable where Action: Equatable {}
extension ForgeAlertButton: Sendable where Action: Sendable {}
