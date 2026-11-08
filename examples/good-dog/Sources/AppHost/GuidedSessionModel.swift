@preconcurrency import ForgeFFI
import Foundation
import Observation

private let log = ForgeLog("GoodDog")

/// A guided session on screen: the card showing, and the owner's moves.
/// Rust's `GuidedSession` (rust/src/guided.rs) decides which card shows,
/// when Back and Next work and when the session may be recorded; this model
/// keeps the latest card for the view. It saves nothing until `record`, so
/// a session closed part way is simply dropped.
@Observable
@MainActor
public final class GuidedSessionModel {
    public private(set) var card: GuidedCard
    public let title: String
    public let ifDifficult: LinkedText
    public let safetyNote: LinkedText?
    /// A failed record, for an alert. The view clears it when dismissed.
    public var error: String?

    @ObservationIgnored private let session: GuidedSession
    @ObservationIgnored private let app: GoodDogModel

    init(session: GuidedSession, app: GoodDogModel) {
        self.session = session
        self.app = app
        card = session.card()
        title = session.title()
        ifDifficult = session.ifDifficult()
        safetyNote = session.safetyNote()
    }

    public func next() {
        move { try $0.next() }
    }

    public func back() {
        move { try $0.back() }
    }

    /// Records the session from the outcome card, as the exercise screen
    /// records. Throws Rust's `AppError`, so the card can stay and say why.
    public func record(result: SessionResult, welfareConcern: Bool) async throws -> SessionFeedback {
        defer { card = session.card() }
        return try await app.recordGuided(session, result: result, welfareConcern: welfareConcern)
    }

    private func move(_ step: (GuidedSession) throws -> GuidedCard) {
        do {
            card = try step(session)
        } catch {
            // The buttons follow `card`, so this is a double tap at an end.
            log.warn("guided move refused: \(error)")
            card = session.card()
        }
    }
}

extension GuidedCard {
    /// "Get ready", "Step 2 of 5" or "How did it go?".
    public var heading: String {
        switch kind {
        case .getReady: "Get ready"
        case .step: "Step \(stepNumber ?? 0) of \(stepCount)"
        case .outcome: "How did it go?"
        }
    }
}
