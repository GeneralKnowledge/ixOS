// A short message at the bottom of the screen for work that continues, or
// that can be taken back: "Sending 3 files" with progress, "Item deleted"
// with Undo. Foundation only, so a Linux test can check the model shows it
// and that Undo reaches the model; the view is `.forgeBanner(_:perform:)`.
//
// iOS has no system toast, so keep it rare: errors that need a decision
// stay alerts (ForgeAlert), and routine saves need no banner at all.

import Foundation

/// A banner the model owns, with an optional action (Undo, Show) that
/// carries the model's own value back, like `ForgeAlert`:
///
///     enum ListUndo: Equatable { case restore(Item) }
///
///     model.banner = .undo("Deleted “Milk”", action: .restore(item),
///                          identifier: "list.undo")
public struct ForgeBanner<Action>: Identifiable {
    /// The banner's button.
    public struct Button {
        public var title: String
        public var action: Action
        /// The accessibility identifier, `screen.element`.
        public var identifier: String

        public init(_ title: String, action: Action, identifier: String) {
            self.title = title
            self.action = action
            self.identifier = identifier
        }
    }

    public let id: UUID
    public var message: String
    /// 0...1 while work runs, nil when there is nothing to measure.
    public var progress: Double?
    public var button: Button?
    /// How long it stays before it goes by itself, in seconds; nil keeps it
    /// until the model clears it (running work).
    public var duration: Double?

    public init(_ message: String, progress: Double? = nil, button: Button? = nil, duration: Double? = 4) {
        self.id = UUID()
        self.message = message
        self.progress = progress.map { min(max($0, 0), 1) }
        self.button = button
        self.duration = duration
    }

    /// A finished action that can be taken back for a few seconds. Undo
    /// stays a little longer than a plain message, so it can be reached.
    public static func undo(_ message: String, action: Action, identifier: String) -> ForgeBanner {
        ForgeBanner(message, button: Button("Undo", action: action, identifier: identifier), duration: 6)
    }

    /// Work that continues while the person carries on. Stays until the
    /// model replaces or clears it.
    public static func working(_ message: String, progress: Double? = nil) -> ForgeBanner {
        ForgeBanner(message, progress: progress, duration: nil)
    }

    /// The same banner (same id, so it isn't shown again) with new progress
    /// or text, for updating running work.
    public func updated(message: String? = nil, progress: Double?) -> ForgeBanner {
        var copy = self
        if let message { copy.message = message }
        copy.progress = progress.map { min(max($0, 0), 1) }
        return copy
    }

    /// Running work with nothing to measure: the banner shows a spinner.
    public var showsActivity: Bool {
        duration == nil && progress == nil
    }

    /// What VoiceOver says when the banner appears.
    public var announcement: String {
        guard let button else { return message }
        return "\(message). \(button.title) is available."
    }
}

extension ForgeBanner where Action == Never {
    /// A message that only informs ("Saved to Files").
    public static func message(_ text: String) -> ForgeBanner {
        ForgeBanner(text)
    }
}
