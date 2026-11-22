import Foundation
import Observation
@preconcurrency import ForgeFFI

/// Draws what Rust's gate accepted. `A2uiSurfaceHost` (a view-side class,
/// because A2UI's core needs Combine) is the real one.
@MainActor
protocol A2uiRendering: AnyObject {
    /// Drops the surface, ready for a new one.
    func reset()
    /// Renders A2UI messages (one JSON object each). Throws if one can't be
    /// parsed; A2UI's own validation reports through the host's listener.
    func render(messages: [String]) throws
}

/// The A2UI Lab screen's state (A2UI-1). Rust owns the plan, the gate and the
/// change set ([`A2uiLab`]); this model passes payloads to the renderer, the
/// surface's events back to Rust, and shows only what Rust returns.
@MainActor
@Observable
final class A2uiLabModel {
    /// The change set the app-owned confirmation shows. Always Rust's.
    private(set) var changeSet: A2uiChangeSet?
    /// What happened last, for the notice at the top.
    private(set) var notice: Notice?
    private(set) var saved: [A2uiSavedPlan] = []
    private(set) var started = false
    private(set) var updated = false
    let hostileCases: [A2uiHostileCase]

    struct Notice: Equatable {
        enum Kind { case info, refused, warning }
        var kind: Kind
        var text: String
    }

    @ObservationIgnored private let lab: A2uiLab
    @ObservationIgnored weak var renderer: (any A2uiRendering)?

    init(lab: A2uiLab = A2uiLab()) {
        self.lab = lab
        hostileCases = lab.hostileCases()
    }

    var surfaceId: String { lab.surfaceId() }
    var catalogId: String { lab.catalogId() }

    /// Shows the model's proposal on a new surface.
    func start() {
        changeSet = nil
        renderer?.reset()
        deliver(notice: nil) { try lab.start() }
        started = true
        updated = false
    }

    /// The agent's incremental update. If a change set was showing, it is
    /// gone: Rust dropped it, and Confirm would refuse it.
    func agentUpdate() {
        let hadReview = changeSet != nil
        let outcome = deliver(
            notice: Notice(
                kind: hadReview ? .warning : .info,
                text: hadReview
                    ? "The agent updated the plan after your review. Review it again."
                    : "The agent updated the plan."
            )
        ) { try lab.agentUpdate() }
        // Once Rust accepts the update it is spent, even if the renderer
        // then fails: retrying would only be refused, so Start Over it is.
        if outcome != .refused {
            changeSet = nil
            updated = true
        }
    }

    /// Sends one deliberately bad payload; Rust must refuse it.
    func sendHostile(_ hostile: A2uiHostileCase) {
        deliver(
            notice: Notice(
                kind: .warning,
                text: "\(hostile.title): accepted. This is a bug; the lab should have refused it."
            ),
            refused: "Refused \u{201C}\(hostile.title)\u{201D}"
        ) { try lab.sendHostile(id: hostile.id) }
    }

    /// The surface's button sent an event. Rust checks it and returns the
    /// change set; nothing the surface wrote is shown as the change set.
    func surfaceEvent(name: String, sourceComponentId: String, contextJson: String) {
        do {
            changeSet = try lab.review(
                name: name, sourceComponentId: sourceComponentId, contextJson: contextJson)
            notice = nil
        } catch {
            changeSet = nil
            notice = Notice(kind: .refused, text: Self.reason(error))
        }
    }

    /// The person changed a value on the surface.
    func surfaceEdited() {
        if lab.surfaceEdited() {
            changeSet = nil
            notice = Notice(kind: .info, text: "You changed the plan, so review it again before confirming.")
        }
    }

    /// A2UI's renderer reported a problem (only its code is shown).
    /// A2UI's renderer validates each message as it goes and reports here,
    /// so part of a batch may already be drawn: the way back is Start Over.
    func rendererError(code: String) {
        notice = Notice(
            kind: .refused,
            text: "The renderer refused part of the surface (\(code)). Start Over to reload the proposal.")
    }

    /// The app-owned Confirm.
    func confirm() {
        guard let changeSet else { return }
        do {
            let applied = try lab.confirm(revision: changeSet.revision)
            notice = Notice(
                kind: .info,
                text: applied == 1 ? "Saved 1 plan." : "Saved \(applied) plans.")
        } catch {
            notice = Notice(kind: .warning, text: Self.reason(error))
        }
        self.changeSet = nil
        saved = lab.savedPlans()
    }

    func cancelReview() {
        changeSet = nil
    }

    enum Delivery { case rendered, refused, renderFailed }

    /// Runs `payload`, renders what it returns, and shows `notice` on
    /// success or the refusal on failure. A refusal from Rust changes
    /// nothing; a render failure comes after Rust accepted the payload.
    @discardableResult
    private func deliver(
        notice success: Notice?, refused: String = "Refused", _ payload: () throws -> A2uiPayload
    ) -> Delivery {
        let accepted: A2uiPayload
        do {
            accepted = try payload()
        } catch {
            notice = Notice(kind: .refused, text: "\(refused): \(Self.reason(error))")
            return .refused
        }
        do {
            try renderer?.render(messages: accepted.messages)
        } catch {
            notice = Notice(
                kind: .refused,
                text: "The renderer couldn't read the update. Start Over to reload the proposal.")
            return .renderFailed
        }
        notice = success
        return .rendered
    }

    static func reason(_ error: Error) -> String {
        switch error as? A2uiLabError {
        case .Refused(_, let reason)?, .Stale(let reason)?: reason
        case nil: error.localizedDescription
        }
    }
}
