import CommonplaceApp
@preconcurrency import CommonplaceFFI
import ForgeAppKit
@preconcurrency import ForgeKit
import Foundation
import XCTest

/// Drives the view model and ForgeKit's host against the app's real Rust
/// core through the generated bindings. Fetching feeds is tested in Rust
/// (rust/src/tests.rs, with a test HTTP client); this checks the Swift side
/// sees Rust's state and errors. It imports no SwiftUI, so `forge check
/// swift` runs it on Linux too.
final class LibraryModelTests: XCTestCase {
    private func makeDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("commonplace-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    @MainActor
    private func open(_ dir: URL) async throws -> (ForgeHost<AppCore>, LibraryModel) {
        let environment = ForgeEnvironment(dataDirectory: dir.path, cachesDirectory: dir.path)
        let host = ForgeHost(environment: { environment }) { env in
            try AppCore(storageDirectory: env.dataDirectory)
        }
        await host.start()
        XCTAssertEqual(host.status, .ready)
        return (host, LibraryModel(core: try XCTUnwrap(host.core)))
    }

    @MainActor
    func testANewLibraryIsEmpty() async throws {
        let (host, model) = try await open(try makeDirectory())
        XCTAssertEqual(model.feeds, [])
        XCTAssertEqual(model.timeline(.all), [])
        XCTAssertEqual(model.counts(.unread), TimelineCounts(total: 0, unread: 0))
        XCTAssertNil(model.article(id: 1))
        // Nothing to refresh: no request, no alert.
        await model.refresh()
        XCTAssertNil(model.lastRefresh)
        XCTAssertNil(model.alert)
        // Only a refresh that found the device offline is repeated.
        await model.refreshIfOffline()
        XCTAssertNil(model.lastRefresh)
        host.close()
    }

    @MainActor
    func testABadAddressIsRefusedWithWordsForTheScreen() async throws {
        let (host, model) = try await open(try makeDirectory())
        do {
            _ = try await model.add(url: "not an address")
            XCTFail("a bad address was accepted")
        } catch let error as AppError {
            guard case .InvalidInput = error else { return XCTFail("\(error)") }
            XCTAssertEqual(
                LibraryModel.describe(error),
                "That doesn't look like a web address. Check it and try again."
            )
            XCTAssertFalse(error.failure.isRetryable)
        }
        XCTAssertEqual(model.feeds, [])
        host.close()
    }

    @MainActor
    func testOpeningAMissingArticleSetsTheAlert() async throws {
        let (host, model) = try await open(try makeDirectory())
        let text = await model.open(articleID: 42)
        XCTAssertNil(text)
        XCTAssertEqual(model.alert?.message, "That's no longer in your library.")
        host.close()
    }

    @MainActor
    func testNothingToRefreshIsNeverStale() async throws {
        let (host, model) = try await open(try makeDirectory())
        XCTAssertFalse(model.isStale(), "no feeds, nothing to refresh on launch")
        host.close()
    }

    func testAlertsAndFeedRowsSayWhatHappened() {
        XCTAssertEqual(AppError.Offline(reason: "x").failure.title, "You're Offline")
        XCTAssertEqual(AppError.NotFound.failure.title, "Not Found")
        XCTAssertEqual(LibraryModel.refreshProblem("http-503"), "The site answered with an error (HTTP 503)")
        XCTAssertEqual(LibraryModel.refreshProblem("offline"), "Offline when it last refreshed")
        XCTAssertEqual(LibraryModel.refreshProblem("HTTP 500"), "Couldn't refresh", "rows saved before kinds were stored")
    }

    func testErrorsHaveFrameworkKinds() {
        XCTAssertEqual(appErrorKind(error: .AlreadySubscribed(feedId: 1)), .invalid)
        XCTAssertEqual(appErrorKind(error: .Http(status: 503)), .unavailable)
        XCTAssertTrue(AppError.Cancelled.isCancellation)
    }

    /// UI tests launch with `-ForgeTemporaryData`: each launch starts empty,
    /// and adding `-ForgeKeepTestData` finds the last launch's data again,
    /// so a relaunch can check that a bookmark was saved.
    func testThrowawayDataStartsEmptyUnlessKept() throws {
        let root = try makeDirectory()
        let fresh = [ForgeEnvironment.temporaryDataArgument]
        let first = try ForgeEnvironment.standard(app: "Commonplace", arguments: fresh, testRoot: root)
        let marker = URL(fileURLWithPath: first.dataDirectory).appendingPathComponent("marker")
        try Data("saved".utf8).write(to: marker)

        let kept = try ForgeEnvironment.standard(
            app: "Commonplace",
            arguments: fresh + [ForgeEnvironment.keepTestDataArgument],
            testRoot: root
        )
        XCTAssertEqual(kept.dataDirectory, first.dataDirectory)
        XCTAssertTrue(FileManager.default.fileExists(atPath: marker.path))

        _ = try ForgeEnvironment.standard(app: "Commonplace", arguments: fresh, testRoot: root)
        XCTAssertFalse(FileManager.default.fileExists(atPath: marker.path))
    }

    /// Rust's `reason` fields are diagnostics: the screen gets plain words.
    func testErrorReasonsStayOffTheScreen() {
        let storage = LibraryModel.describe(AppError.Storage(reason: "disk I/O error at page 7"))
        let network = LibraryModel.describe(AppError.Network(reason: "tls handshake eof"))
        XCTAssertFalse(storage.contains("page 7"), storage)
        XCTAssertFalse(network.contains("handshake"), network)
        XCTAssertFalse(network.isEmpty)
    }
}
