import Foundation
import Observation
@preconcurrency import ForgeFFI

// The app capability audit's surfaces (docs/research/app-capability-audit.md,
// C2 to C4): Commonplace's "organise feeds", Soundscape's "make a playlist"
// and the Bonsai Harness's verified-task card. Rust owns every check, change
// set and revision (`rust/src/a2ui_lab`); the apps own their data and apply
// a confirmed plan themselves after checking it again. These models only
// carry messages, events and the confirmed plan between them.

/// What a surface's screen shows at the top.
struct A2uiNotice: Equatable {
    enum Kind { case info, refused, warning }
    var kind: Kind
    var text: String
}

/// How an app surface reaches the app it works on: a snapshot of the app's
/// data as JSON, and the apply step for a confirmed plan, which returns a
/// sentence saying what it did. Supplied by `A2uiAppHooks`, so this file
/// needs no app's bindings.
struct A2uiAppLink {
    /// The app's name, for notices ("Commonplace").
    var name: String
    var snapshot: @MainActor () async -> String?
    var apply: @MainActor (String) async throws -> String
}

func a2uiReason(_ error: Error) -> String {
    switch error as? A2uiLabError {
    case .Refused(_, let reason)?, .Stale(let reason)?: reason
    case nil: error.localizedDescription
    }
}

/// Commonplace's "organise feeds" (C3).
@MainActor
@Observable
final class A2uiFeedsModel: A2uiSurfaceOwner {
    private(set) var changeSet: A2uiFolderChangeSet?
    private(set) var notice: A2uiNotice?
    private(set) var started = false
    private(set) var busy = false

    @ObservationIgnored private let lab: A2uiFeeds
    @ObservationIgnored private let app: A2uiAppLink
    @ObservationIgnored weak var renderer: (any A2uiRendering)?

    init(app: A2uiAppLink, lab: A2uiFeeds = A2uiFeeds()) {
        self.app = app
        self.lab = lab
    }

    var surfaceId: String { lab.surfaceId() }
    var catalogId: String { a2uiCatalogId() }

    /// A new surface from the app's data as it is now.
    func start() async {
        // An apply in flight finishes against the surface it was confirmed
        // on; a new surface waits for it.
        guard !busy else { return }
        changeSet = nil
        renderer?.reset()
        started = false
        guard let snapshot = await app.snapshot() else {
            notice = A2uiNotice(kind: .warning, text: "\(app.name) couldn't open its library.")
            return
        }
        do {
            let payload = try lab.start(snapshotJson: snapshot)
            try renderer?.render(messages: payload.messages)
            started = true
            notice = nil
        } catch {
            notice = A2uiNotice(kind: .refused, text: a2uiReason(error))
        }
    }

    func surfaceEvent(name: String, sourceComponentId: String, contextJson: String) {
        do {
            changeSet = try lab.review(
                name: name, sourceComponentId: sourceComponentId, contextJson: contextJson)
            notice = nil
        } catch {
            changeSet = nil
            notice = A2uiNotice(kind: .refused, text: a2uiReason(error))
        }
    }

    func surfaceEdited() {
        if lab.surfaceEdited() {
            changeSet = nil
            notice = A2uiNotice(kind: .info, text: "You changed a folder, so review again before confirming.")
        }
    }

    func rendererError(code: String) {
        notice = A2uiNotice(
            kind: .refused,
            text: "The renderer refused part of the surface (\(code)). Start Over to reload it.")
    }

    /// The app-owned Confirm: Rust hands back the reviewed plan, and
    /// Commonplace checks it again and applies it.
    func confirm() async {
        guard let changeSet, !busy else { return }
        busy = true
        defer { busy = false }
        do {
            let plan = try lab.confirm(revision: changeSet.revision)
            let outcome = try await app.apply(plan)
            self.changeSet = nil
            renderer?.reset()
            started = false
            notice = A2uiNotice(kind: .info, text: outcome)
        } catch {
            self.changeSet = nil
            notice = A2uiNotice(kind: .warning, text: a2uiReason(error))
        }
    }

    func cancelReview() {
        guard !busy else { return }
        changeSet = nil
    }
}

/// Soundscape's "make a playlist" (C4).
@MainActor
@Observable
final class A2uiPlaylistModel: A2uiSurfaceOwner {
    private(set) var changeSet: A2uiPlaylistChangeSet?
    private(set) var notice: A2uiNotice?
    private(set) var started = false
    private(set) var busy = false

    @ObservationIgnored private let lab: A2uiPlaylist
    @ObservationIgnored private let app: A2uiAppLink
    @ObservationIgnored weak var renderer: (any A2uiRendering)?

    init(app: A2uiAppLink, lab: A2uiPlaylist = A2uiPlaylist()) {
        self.app = app
        self.lab = lab
    }

    var surfaceId: String { lab.surfaceId() }
    var catalogId: String { a2uiCatalogId() }

    func start() async {
        // An apply in flight finishes against the surface it was confirmed
        // on; a new surface waits for it.
        guard !busy else { return }
        changeSet = nil
        renderer?.reset()
        started = false
        guard let snapshot = await app.snapshot() else {
            notice = A2uiNotice(kind: .warning, text: "\(app.name) couldn't open its library.")
            return
        }
        do {
            let payload = try lab.start(snapshotJson: snapshot)
            try renderer?.render(messages: payload.messages)
            started = true
            notice = nil
        } catch {
            notice = A2uiNotice(kind: .refused, text: a2uiReason(error))
        }
    }

    func surfaceEvent(name: String, sourceComponentId: String, contextJson: String) {
        do {
            changeSet = try lab.review(
                name: name, sourceComponentId: sourceComponentId, contextJson: contextJson)
            notice = nil
        } catch {
            changeSet = nil
            notice = A2uiNotice(kind: .refused, text: a2uiReason(error))
        }
    }

    func surfaceEdited() {
        if lab.surfaceEdited() {
            changeSet = nil
            notice = A2uiNotice(kind: .info, text: "You changed the songs, so review again before confirming.")
        }
    }

    func rendererError(code: String) {
        notice = A2uiNotice(
            kind: .refused,
            text: "The renderer refused part of the surface (\(code)). Start Over to reload it.")
    }

    func confirm() async {
        guard let changeSet, !busy else { return }
        busy = true
        defer { busy = false }
        do {
            let plan = try lab.confirm(revision: changeSet.revision)
            let outcome = try await app.apply(plan)
            self.changeSet = nil
            renderer?.reset()
            started = false
            notice = A2uiNotice(kind: .info, text: outcome)
        } catch {
            self.changeSet = nil
            notice = A2uiNotice(kind: .warning, text: a2uiReason(error))
        }
    }

    func cancelReview() {
        guard !busy else { return }
        changeSet = nil
    }
}

/// The Bonsai Harness's verified-task card (C2): read-only.
@MainActor
@Observable
final class A2uiTaskCardModel: A2uiSurfaceOwner {
    private(set) var notice: A2uiNotice?
    private(set) var shown = false

    @ObservationIgnored private let harness: BonsaiHarness?
    @ObservationIgnored weak var renderer: (any A2uiRendering)?

    init(harness: BonsaiHarness?) {
        self.harness = harness
    }

    var surfaceId: String { harness?.resultCardSurfaceId() ?? "taskResult" }
    var catalogId: String { a2uiCatalogId() }
    var available: Bool { harness != nil }

    /// Draws the card for the harness's last task, if it verified it.
    func show() {
        renderer?.reset()
        shown = false
        guard let harness else {
            notice = A2uiNotice(kind: .warning, text: "The Bonsai Harness isn't available in this build.")
            return
        }
        do {
            let payload = try harness.resultCard()
            try renderer?.render(messages: payload.messages)
            shown = true
            notice = nil
        } catch {
            notice = A2uiNotice(kind: .info, text: a2uiReason(error))
        }
    }

    func surfaceEvent(name: String, sourceComponentId: String, contextJson: String) {
        // The card registers no events, so Rust's gate never lets a control
        // that sends one through; this would be a renderer bug.
        notice = A2uiNotice(kind: .refused, text: "The card has no controls; the event was ignored.")
    }

    func surfaceEdited() {}

    func rendererError(code: String) {
        notice = A2uiNotice(kind: .refused, text: "The renderer refused the card (\(code)).")
    }
}
