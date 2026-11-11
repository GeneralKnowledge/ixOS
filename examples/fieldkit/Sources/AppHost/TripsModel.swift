@preconcurrency import FieldKitFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import Foundation
import Observation

private let log = ForgeLog("Trips")

/// What the screens show and the actions they take. It holds no rules:
/// recording, filtering fixes, totals and notes are FieldKit's Rust core
/// (`rust/src/lib.rs`); this model asks it and shows what it says.
@Observable
@MainActor
public final class TripsModel {
    /// Every trip, newest first, as Rust last reported it.
    public private(set) var trips: [TripSummary] = []
    /// Whether FieldKit may use location.
    public private(set) var permission: PermissionStatus = .notDetermined
    /// Why location isn't reaching the recording trip, if it isn't.
    public private(set) var locationProblem: String?

    /// Alerts the screens ask for: delete after confirming, or a failure.
    public enum Action: Equatable, Sendable {
        case delete(Int64)
        case stop
    }

    public var alert: ForgeAlert<Action>?
    /// Increases after each saved change; views use it as a haptic trigger.
    public private(set) var saveCount = 0

    public let core: FieldKitCore

    public init(core: FieldKitCore) {
        self.core = core
        reload()
        Task { await refreshPermission() }
    }

    /// The trip being recorded, if any.
    public var recording: TripSummary? { trips.first { $0.recording } }

    public func trip(id: Int64) -> TripSummary? { trips.first { $0.id == id } }

    /// Re-reads Rust's published trips. Cheap: no command is waited for.
    public func reload() {
        trips = core.trips()
        locationProblem = core.locationProblem()
    }

    /// Back on screen: re-check permission (it may have changed in
    /// Settings) and follow location again for a recording trip.
    public func resume() {
        Task {
            await refreshPermission()
            core.resumeRecording()
            reload()
        }
    }

    public func refreshPermission() async {
        permission = await core.locationPermission()
    }

    /// Shows the system prompt; call it from a button the person tapped.
    public func requestPermission() async {
        permission = await core.requestLocationPermission()
        reload()
    }

    /// Starts recording. Throws so the start sheet can stay open and show
    /// the error itself.
    public func start(title: String) async throws {
        if permission == .notDetermined {
            await requestPermission()
        }
        _ = try await core.startTrip(title: title)
        reload()
        saveCount += 1
    }

    public func askToStop() {
        alert = .confirm(
            "Stop recording?",
            message: "The trip is saved as it is now.",
            confirm: "Stop",
            action: .stop,
            identifier: "recording.confirmStop"
        )
    }

    public func askToDelete(_ trip: TripSummary) {
        alert = .confirm(
            "Delete this trip?",
            message: "Its track and notes are deleted too. This can't be undone.",
            confirm: "Delete Trip",
            action: .delete(trip.id),
            identifier: "trips.confirmDelete",
            destructive: true
        )
    }

    public func handle(_ action: Action) async {
        switch action {
        case .stop:
            await perform { core in _ = try await core.stopTrip() }
        case .delete(let id):
            await perform { core in try await core.deleteTrip(tripId: id) }
        }
    }

    public func rename(_ trip: TripSummary, to title: String) async throws {
        try await core.renameTrip(tripId: trip.id, title: title)
        reload()
    }

    /// Adds a text note; on a recording trip it is placed where you are.
    public func addNote(to tripID: Int64, text: String) async throws {
        _ = try await core.addTextNote(tripId: tripID, body: text, at: nil)
        reload()
        saveCount += 1
    }

    public func removeNote(_ note: Note) async {
        await perform { core in try await core.removeNote(noteId: note.id) }
    }

    /// A trip's notes and track, read off Rust's command thread.
    public func load(_ tripID: Int64) async throws -> (detail: TripDetail, track: [TrackPoint]) {
        async let detail = core.trip(tripId: tripID, cancellation: nil)
        async let track = core.track(tripId: tripID, cancellation: nil)
        return try await (detail, track)
    }

    /// Writes the trip as a file to share.
    public func export(_ tripID: Int64, as format: ExportFormat) async throws -> URL {
        let path = try await core.exportTrip(tripId: tripID, format: format, cancellation: nil)
        return URL(fileURLWithPath: path)
    }

    @discardableResult
    private func perform(_ command: (FieldKitCore) async throws -> Void) async -> Bool {
        do {
            try await command(core)
            reload()
            saveCount += 1
            return true
        } catch {
            log.warn("command failed: \(error)")
            alert = .failure(error)
            return false
        }
    }

    // MARK: Words for the screen

    nonisolated public static func describe(_ error: Error) -> String {
        guard let error = error as? FieldKitError else { return ForgeFailure(error).message }
        switch error {
        case .EmptyTitle: return "Give the trip a name."
        case .TitleTooLong(let max): return "Names can be up to \(max) characters."
        case .EmptyNote: return "Write something first."
        case .NoteTooLong(let max): return "Notes can be up to \(max) characters."
        case .NotFound: return "That trip or note no longer exists."
        case .AlreadyRecording: return "A trip is already being recorded. Stop it first."
        case .NotRecording: return "No trip is being recorded."
        case .File: return "That file couldn't be added."
        case .InUse: return "FieldKit is open in another copy of the app."
        case .Corrupt: return "Your trips couldn't be read."
        case .NewerData: return "Your trips were saved by a newer version of FieldKit."
        case .Storage: return "Couldn't save. Try again."
        case .Stopped: return "Stopped."
        }
    }

    nonisolated public static func title(_ error: FieldKitError) -> String {
        switch error {
        case .EmptyTitle, .TitleTooLong, .EmptyNote, .NoteTooLong: return "Check the Details"
        case .NotFound: return "Not Found"
        case .AlreadyRecording: return "Already Recording"
        case .NotRecording: return "Not Recording"
        case .File: return "Couldn't Add the File"
        case .InUse: return "Already Open"
        case .Corrupt, .NewerData, .Storage: return "Couldn't Save"
        case .Stopped: return "Stopped"
        }
    }
}

extension FieldKitError: ForgeDescribedError {
    public var failure: ForgeFailure {
        let retryable: Bool
        switch self {
        case .Storage: retryable = true
        default: retryable = false
        }
        return ForgeFailure(
            title: TripsModel.title(self), message: TripsModel.describe(self), isRetryable: retryable)
    }

    public var isCancellation: Bool {
        if case .Stopped = self { return true }
        return false
    }
}

extension TripSummary: Identifiable {}
extension Note: Identifiable {}

extension PermissionStatus {
    /// True when location may be used now (Rust's `allows_use`).
    var allowsUse: Bool { self == .granted || self == .provisional }
}

// MARK: Formatting (Foundation only, so it tests on Linux)

public enum TripFormat {
    /// "1.2 km" or "850 m" in the person's units.
    public static func distance(_ metres: Double) -> String {
        let measurement = Measurement(value: metres, unit: UnitLength.meters)
        return measurement.formatted(.measurement(width: .abbreviated, usage: .road))
    }

    /// "1:05:12" or "12:04".
    public static func duration(milliseconds: Int64) -> String {
        let total = max(0, Int(milliseconds / 1000))
        let h = total / 3600, m = total % 3600 / 60, s = total % 60
        return h > 0
            ? String(format: "%d:%02d:%02d", h, m, s)
            : String(format: "%d:%02d", m, s)
    }

    public static func date(_ ms: Int64) -> Date {
        Date(timeIntervalSince1970: TimeInterval(ms) / 1000)
    }

    /// How long a trip took, or has been running.
    public static func elapsed(_ trip: TripSummary, now: Date = Date()) -> String {
        let end = trip.endedMs ?? Int64(now.timeIntervalSince1970 * 1000)
        return duration(milliseconds: end - trip.startedMs)
    }

    /// The name a new trip starts with: "Walk on 11 Nov".
    public static func defaultTitle(now: Date = Date()) -> String {
        "Trip on \(now.formatted(.dateTime.day().month(.abbreviated)))"
    }
}
