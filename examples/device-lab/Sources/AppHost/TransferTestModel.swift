@preconcurrency import ForgeFFI
import Foundation
import Observation

/// The Background Downloads screen's view of Rust's transfer ledger
/// (`transfer_test.rs`). Rust and Forge's `transfers` service own every
/// rule; this copies the rows and passes taps on.
@MainActor
@Observable
public final class TransferTestModel {
    public private(set) var rows: [TransferRow] = []
    public private(set) var error: String?
    /// Set by "End the app when I leave": the next move to the background
    /// ends the app the way iOS reclaims a suspended one (acceptance test 1).
    public var endWhenBackgrounded = false

    public init() {}

    /// Copies the rows twice a second until the calling task is cancelled.
    public func follow() async {
        while !Task.isCancelled {
            refresh()
            try? await Task.sleep(for: .milliseconds(500))
        }
    }

    public func refresh() {
        rows = transferTestRows()
    }

    public func start(megabytes: UInt32) {
        run { _ = try transferTestStart(megabytes: megabytes) }
    }

    public func cancel(_ row: TransferRow) {
        run { try transferTestCancel(id: row.id) }
    }

    public func retry(_ row: TransferRow) {
        run { try transferTestRetry(id: row.id) }
    }

    public func clear() {
        run { try transferTestClear() }
    }

    /// Called when the app moves to the background.
    public func didEnterBackground() {
        guard endWhenBackgrounded else { return }
        // A moment for the system to take the app-switcher snapshot.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            transferTestEndApp()
        }
    }

    private func run(_ action: () throws -> Void) {
        do {
            try action()
            error = nil
        } catch {
            self.error = "\(error)"
        }
        refresh()
    }
}
