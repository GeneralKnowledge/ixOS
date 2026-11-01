import AppHost
import Foundation
import XCTest

/// Drives the view model against the real Rust core through the generated Swift bindings.
/// Run with scripts/test-swift-host.sh, which builds the host library first.
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
    func testFreshInstallStartsAtZero() throws {
        let model = CounterViewModel(storageDirectory: try makeDirectory().path)
        XCTAssertFalse(model.isBlocked)
        XCTAssertEqual(model.count, 0)
        XCTAssertEqual(model.startupLabel, "Fresh install")
    }

    @MainActor
    func testActionsPersistAcrossInstances() throws {
        let dir = try makeDirectory()
        let model = CounterViewModel(storageDirectory: dir.path)
        model.increment()
        model.increment()
        model.decrement()
        XCTAssertEqual(model.count, 1)
        XCTAssertNil(model.errorMessage)

        let reopened = CounterViewModel(storageDirectory: dir.path)
        XCTAssertEqual(reopened.count, 1)
        XCTAssertEqual(reopened.startupLabel, "Restored saved state")
    }

    @MainActor
    func testDecrementAtZeroShowsErrorAndKeepsCount() throws {
        let model = CounterViewModel(storageDirectory: try makeDirectory().path)
        model.decrement()
        XCTAssertEqual(model.count, 0)
        XCTAssertNotNil(model.errorMessage)
    }

    @MainActor
    func testCorruptDataBlocksAndCanBeReset() throws {
        let dir = try makeDirectory()
        try Data("{not json".utf8).write(to: snapshotURL(in: dir))
        let model = CounterViewModel(storageDirectory: dir.path)
        XCTAssertTrue(model.isBlocked)
        XCTAssertTrue(model.canRecoverFromCorruption)

        model.recoverFromCorruption()
        XCTAssertFalse(model.isBlocked)
        XCTAssertEqual(model.count, 0)
        XCTAssertEqual(model.startupLabel, "Recovered — fresh counter")
    }

    @MainActor
    func testNewerSchemaIsNotOfferedForReset() throws {
        let dir = try makeDirectory()
        let newer = #"{"schema_version": 999, "count": 7}"#
        try Data(newer.utf8).write(to: snapshotURL(in: dir))
        let model = CounterViewModel(storageDirectory: dir.path)
        XCTAssertTrue(model.isBlocked)
        XCTAssertFalse(model.canRecoverFromCorruption)
        XCTAssertTrue(model.errorMessage?.contains("newer version") ?? false)

        // Even if a host called recovery anyway, Rust refuses to overwrite the data.
        model.recoverFromCorruption()
        XCTAssertTrue(model.isBlocked)
        XCTAssertEqual(try String(contentsOf: snapshotURL(in: dir), encoding: .utf8), newer)
    }
}
