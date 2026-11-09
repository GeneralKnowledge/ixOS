@preconcurrency import ForgeApp
@testable import AppHost
@preconcurrency import ForgeFFI
import ForgeKit
import Foundation
import XCTest

/// The model and ForgeHost against the real Rust core, on Linux or macOS.
@MainActor
final class ItemsModelTests: XCTestCase {
    private func openHost() async throws -> ForgeHost<AppCore> {
        let host = ItemsModel.makeHost(environment: try ForgeEnvironment.temporary())
        await host.start()
        XCTAssertEqual(host.status, .ready)
        return host
    }

    /// Waits (briefly) for the host's change counter to pass `value`.
    private func waitForChanges(_ host: ForgeHost<AppCore>, above value: UInt64) async throws {
        for _ in 0..<200 where host.changes <= value {
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTAssertGreaterThan(host.changes, value)
    }

    func testCommandsArriveAsRevisionsAndTheModelReQueries() async throws {
        let host = try await openHost()
        let model = ItemsModel(host: host)
        let before = host.changes
        await model.add(title: "Milk")
        try await waitForChanges(host, above: before)
        model.refresh()
        XCTAssertEqual(model.items.map(\.title), ["Milk"])
        XCTAssertEqual(host.revision, 1)
    }

    func testErrorsAreDescribedByFrameworkKind() async throws {
        let host = try await openHost()
        let model = ItemsModel(host: host)
        await model.add(title: "   ")
        XCTAssertEqual(model.errorMessage, ForgeErrors.describe(.invalid))
    }

    func testCancellingTheSwiftTaskCancelsTheRustWork() async throws {
        let host = try await openHost()
        host.report(.active)
        let model = ItemsModel(host: host)
        let task = Task { await model.count(to: 1000) }
        try await Task.sleep(nanoseconds: 100_000_000)
        task.cancel()
        await task.value
        XCTAssertEqual(model.countResult, ForgeErrors.describe(.cancelled))
    }

    func testTheBackgroundInterruptsOnScreenWork() async throws {
        let host = try await openHost()
        host.report(.active)
        let model = ItemsModel(host: host)
        let task = Task { await model.count(to: 1000) }
        try await Task.sleep(nanoseconds: 100_000_000)
        host.report(.background)
        await task.value
        XCTAssertEqual(model.countResult, ForgeErrors.describe(.interrupted))
    }

    func testTypesFromForgeAppCrossBothModules() async throws {
        let host = try await openHost()
        // AppPhase comes from ForgeApp (ForgeKit) and goes through the app's
        // own generated AppCore.phase().
        host.report(.inactive)
        XCTAssertEqual(host.core?.phase(), AppPhase.inactive)
        XCTAssertEqual(forgeAppContractVersion(), forgeKitContractVersion)
    }
}
