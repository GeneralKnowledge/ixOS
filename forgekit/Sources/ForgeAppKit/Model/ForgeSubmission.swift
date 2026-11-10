// The state of a button that saves, sends or deletes: working while the
// work runs, and the failure if it fails. Foundation and Observation only.

import Foundation
import Observation

/// Runs one piece of work at a time for a screen (saving a form, sending a
/// message) and keeps what the view needs to show it: `isWorking` for the
/// busy button and `failure` for the alert. A second tap while working does
/// nothing, so a slow save never runs twice.
///
///     @Observable @MainActor final class BookFormModel {
///         let saving = ForgeSubmission()
///
///         func save() async -> Bool {
///             await saving.run { try await self.library.updateBook(id: id, input: input) }
///         }
///     }
///
/// In the view, `.forgeFormToolbar(id:submission:...)` shows the spinner and
/// the alert; `ForgeButton(..., isWorking: model.saving.isWorking)` does the
/// busy button outside a sheet.
@Observable
@MainActor
public final class ForgeSubmission {
    public private(set) var isWorking = false
    /// Set when the last run failed; the view's alert clears it.
    public var failure: ForgeFailure?

    public init() {}

    /// Runs `work` unless a run is already going. True when it finished
    /// without throwing. A thrown error becomes `failure`, except a
    /// cancellation, which is not a failure and shows nothing.
    @discardableResult
    public func run(_ work: () async throws -> Void) async -> Bool {
        guard !isWorking else { return false }
        isWorking = true
        failure = nil
        defer { isWorking = false }
        do {
            try await work()
            return true
        } catch {
            if !ForgeFailure.isCancellation(error) {
                failure = ForgeFailure(error)
            }
            return false
        }
    }
}
