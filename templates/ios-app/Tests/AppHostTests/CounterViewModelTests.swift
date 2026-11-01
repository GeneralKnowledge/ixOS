import AppHost
import CounterBridge
import Foundation
import XCTest

/// Drives the view model against the real Rust core through the bridge and the
/// generated Swift bindings. Run with scripts/test-swift-host.sh, which builds
/// the host library first.
final class CounterViewModelTests: XCTestCase {
    private func makeDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-counter-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func snapshotURL(in dir: URL) -> URL {
        dir.appendingPathComponent("forge-counter-v1.json")
    }

    @MainActor
    private func openModel(_ dir: URL) async -> CounterViewModel {
        let model = CounterViewModel(storageDirectory: dir.path)
        await model.open()
        return model
    }

    /// Waits (without blocking the main actor) for an event-driven update.
    @MainActor
    private func eventually(
        _ condition: @MainActor () -> Bool, timeout: TimeInterval = 5,
        file: StaticString = #filePath, line: UInt = #line
    ) async {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() > deadline {
                return XCTFail("condition not met in \(timeout)s", file: file, line: line)
            }
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    @MainActor
    func testNothingOpensUntilOpenIsCalled() async throws {
        let model = CounterViewModel(storageDirectory: try makeDirectory().path)
        XCTAssertTrue(model.isLoading)
        XCTAssertNil(model.client)
        await model.open()
        XCTAssertFalse(model.isLoading)
        XCTAssertNotNil(model.client)
        await model.close()
    }

    @MainActor
    func testFreshInstallStartsAtZero() async throws {
        let model = await openModel(try makeDirectory())
        XCTAssertFalse(model.isBlocked)
        XCTAssertEqual(model.count, 0)
        XCTAssertEqual(model.startupLabel, "Fresh install")
        await model.close()
    }

    @MainActor
    func testOpenTwiceKeepsOneInstanceAndSubscription() async throws {
        let model = await openModel(try makeDirectory())
        let client = model.client
        await model.open()
        XCTAssertTrue(model.client === client)
        XCTAssertEqual(model.client?.subscriptionCount, 1)
        await model.close()
    }

    @MainActor
    func testActionsPersistAcrossInstances() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        await model.increment()
        await model.increment()
        await model.decrement()
        XCTAssertEqual(model.count, 1)
        XCTAssertNil(model.errorMessage)
        await model.close()

        let reopened = await openModel(dir)
        XCTAssertEqual(reopened.count, 1)
        XCTAssertEqual(reopened.startupLabel, "Restored saved state")
        await reopened.close()
    }

    @MainActor
    func testDecrementAtZeroShowsErrorAndKeepsCount() async throws {
        let model = await openModel(try makeDirectory())
        await model.decrement()
        XCTAssertEqual(model.count, 0)
        XCTAssertEqual(model.errorMessage, "Count cannot go below zero.")
        await model.close()
    }

    @MainActor
    func testChangesFromAnotherComponentArriveAsEvents() async throws {
        let model = await openModel(try makeDirectory())
        let shared = try XCTUnwrap(model.client)
        // Another component sharing the same client changes the state.
        _ = try await shared.apply(.increment)
        _ = try await shared.apply(.increment)
        await eventually { model.count == 2 }
        await model.close()
    }

    @MainActor
    func testBulkAddReportsProgressAndCompletes() async throws {
        let model = await openModel(try makeDirectory())
        let task = try XCTUnwrap(model.startBulkAdd(amount: 40, stepDelay: .milliseconds(20)))
        XCTAssertTrue(model.isBulkAddRunning)
        XCTAssertNil(model.startBulkAdd(), "a second bulk add is not started")
        await eventually { (model.bulkAddProgress ?? 0) > 0 }
        await task.value
        XCTAssertFalse(model.isBulkAddRunning)
        XCTAssertEqual(model.count, 40)
        XCTAssertEqual(model.lastOperationMessage, "Added 40.")
        await model.close()
    }

    @MainActor
    func testCancelledBulkAddChangesNothingAndIsNotAnError() async throws {
        let model = await openModel(try makeDirectory())
        let task = try XCTUnwrap(model.startBulkAdd(amount: 100, stepDelay: .seconds(10)))
        model.cancelBulkAdd()
        await task.value
        XCTAssertEqual(model.count, 0)
        XCTAssertNil(model.errorMessage, "cancellation is not shown as a failure")
        XCTAssertEqual(model.lastOperationMessage, "Bulk add cancelled; nothing was added.")
        await model.close()
    }

    @MainActor
    func testIncrementsStayResponsiveDuringBulkAdd() async throws {
        let model = await openModel(try makeDirectory())
        let task = try XCTUnwrap(model.startBulkAdd(amount: 10, stepDelay: .milliseconds(100)))
        await model.increment()
        XCTAssertEqual(model.count, 1)
        XCTAssertTrue(model.isBulkAddRunning)
        await task.value
        XCTAssertEqual(model.count, 11)
        await model.close()
    }

    @MainActor
    func testOperationFinishingAfterTheViewModelIsGoneIsSavedSafely() async throws {
        let dir = try makeDirectory()
        var model: CounterViewModel? = await openModel(dir)
        let task = try XCTUnwrap(model?.startBulkAdd(amount: 5, stepDelay: .milliseconds(50)))
        weak var released = model
        model = nil
        XCTAssertNil(released, "pending work does not keep the view model alive")
        // The operation keeps running in Rust and commits; its late result is dropped.
        await task.value

        // Wait for the old instance to shut down once its last reference went away.
        let reopened = CounterViewModel(storageDirectory: dir.path)
        await reopened.open()
        XCTAssertEqual(reopened.count, 5)
        await reopened.close()
    }

    @MainActor
    func testCloseShutsDownAndCanReopen() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        await model.increment()
        let client = try XCTUnwrap(model.client)
        await model.close()
        XCTAssertTrue(client.isShutDown)
        XCTAssertNil(model.client)
        await model.open()
        XCTAssertEqual(model.count, 1)
        await model.close()
    }

    @MainActor
    func testCorruptDataBlocksAndCanBeReset() async throws {
        let dir = try makeDirectory()
        try Data("{not json".utf8).write(to: snapshotURL(in: dir))
        let model = await openModel(dir)
        XCTAssertTrue(model.isBlocked)
        XCTAssertTrue(model.canRecoverFromCorruption)

        await model.recoverFromCorruption()
        XCTAssertFalse(model.isBlocked)
        XCTAssertEqual(model.count, 0)
        XCTAssertEqual(model.startupLabel, "Recovered — fresh counter")
        await model.close()
    }

    @MainActor
    func testNewerSchemaIsNotOfferedForReset() async throws {
        let dir = try makeDirectory()
        let newer = #"{"schema_version": 999, "count": 7}"#
        try Data(newer.utf8).write(to: snapshotURL(in: dir))
        let model = await openModel(dir)
        XCTAssertTrue(model.isBlocked)
        XCTAssertFalse(model.canRecoverFromCorruption)
        XCTAssertTrue(model.errorMessage?.contains("newer version") ?? false)

        // Even if a host called recovery anyway, Rust refuses to overwrite the data.
        await model.recoverFromCorruption()
        XCTAssertTrue(model.isBlocked)
        XCTAssertEqual(try String(contentsOf: snapshotURL(in: dir), encoding: .utf8), newer)
    }
}
