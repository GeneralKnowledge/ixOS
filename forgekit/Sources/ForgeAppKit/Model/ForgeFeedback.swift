// Haptic feedback as a small vocabulary the model owns. Foundation only, so
// a Linux test can say "saving plays success" without SwiftUI; the view
// plays it with `.forgeFeedback(_:)`.
//
// Feedback should be proportional: a finished session, a saved trip or a
// sent file deserves a small response; a routine tap, a toggle or a scroll
// gets none (the system already gives its own controls theirs).

import Foundation

/// The last feedback the model asked for. Hold one in the model, call
/// `play(_:)` at the moment that deserves it, and attach
/// `.forgeFeedback(model.feedback)` once to the screen:
///
///     public private(set) var feedback = ForgeFeedback()
///     ...
///     try await app.saveTrip(trip)
///     feedback.play(.success)
public struct ForgeFeedback: Equatable, Sendable {
    /// What happened, from the person's point of view.
    public enum Kind: String, CaseIterable, Sendable {
        /// Something is done and kept: saved, sent, finished, or a file
        /// that finished arriving.
        case success
        /// Something they did didn't work. Pair it with the message that
        /// says why; never use it alone.
        case failure
        /// A choice changed among a few options: a step, a rating, a page.
        case selection
        /// New content arrived: after they asked for it, or by itself
        /// somewhere they aren't looking (a message in another chat).
        case refresh
    }

    /// The feedback asked for last, or nil before any.
    public private(set) var kind: Kind?
    /// How many times feedback was asked for; each change plays once.
    public private(set) var count = 0

    public init() {}

    /// Plays `kind` once.
    public mutating func play(_ kind: Kind) {
        self.kind = kind
        count += 1
    }
}
