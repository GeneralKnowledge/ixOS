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
    func testSuggestedFeedsAreOfferedNotSubscribed() async throws {
        let (host, model) = try await open(try makeDirectory())
        let suggestions = model.suggestedFeeds()
        XCTAssertFalse(suggestions.isEmpty)
        XCTAssertTrue(suggestions.allSatisfy { !$0.feeds.isEmpty })
        // Offered only: listing them subscribes to nothing.
        XCTAssertTrue(suggestions.flatMap(\.feeds).allSatisfy { !$0.subscribed })
        XCTAssertEqual(model.feeds, [])
        host.close()
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
        // Linux has no connectivity service: waiting for the network ends
        // at once instead of hanging the feed list's task.
        await model.refreshWhenBackOnline()
        host.close()
    }

    private static let feed = Data(
        """
        <?xml version="1.0"?><rss version="2.0"><channel><title>Fixture Feed</title>\
        <link>https://fixtures.example/</link><item><title>Alpha</title><guid>a</guid>\
        <link>https://fixtures.example/a</link></item></channel></rss>
        """.utf8)

    /// Subscribes to a local feed, then makes the next fetch hang.
    @MainActor
    private func stalledRefresh() async throws -> (ForgeHost<AppCore>, LibraryModel, FeedServer) {
        let server = try FeedServer(body: Self.feed)
        let (host, model) = try await open(try makeDirectory())
        _ = try await model.add(url: server.url)
        XCTAssertEqual(model.feeds.count, 1)
        server.stalling = true
        return (host, model, server)
    }

    @MainActor
    private func waitUntilRefreshing(_ model: LibraryModel) async throws {
        for _ in 0..<200 where !model.isRefreshing {
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertTrue(model.isRefreshing)
        // Long enough for Rust to be waiting on the hung server.
        try await Task.sleep(for: .milliseconds(200))
    }

    /// Stop stops the refresh in Rust, not just the Swift task: it returns
    /// long before the 20 s fetch timeout, nothing is reported or saved, and
    /// no alert says "cancelled".
    @MainActor
    func testStopEndsARefreshInRust() async throws {
        let (host, model, server) = try await stalledRefresh()
        let started = Date()
        let refreshing = Task { await model.refresh() }
        try await waitUntilRefreshing(model)
        model.cancelRefresh()
        await refreshing.value
        XCTAssertLessThan(Date().timeIntervalSince(started), 10)
        XCTAssertFalse(model.isRefreshing)
        XCTAssertNil(model.lastRefresh)
        XCTAssertNil(model.alert)
        withExtendedLifetime(server) {}
        host.close()
    }

    /// Leaving the screen (the task that started the refresh is cancelled)
    /// stops it in Rust the same way.
    /// A refresh the background stopped says nothing, and is tried again
    /// when the app comes back (friction #12).
    @MainActor
    func testARefreshTheBackgroundStoppedIsTriedAgainOnReturn() async throws {
        let (host, model, server) = try await stalledRefresh()
        model.follow(host)
        host.report(.active)
        let refreshing = Task { await model.refresh() }
        try await waitUntilRefreshing(model)
        host.report(.background)
        await refreshing.value
        XCTAssertTrue(model.refreshCutOff)
        XCTAssertNil(model.alert)
        XCTAssertNil(model.lastRefresh)

        server.stalling = false
        host.report(.active)
        for _ in 0..<500 where model.lastRefresh == nil {
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertNotNil(model.lastRefresh)
        XCTAssertFalse(model.refreshCutOff)
        XCTAssertNil(model.alert)
        withExtendedLifetime(server) {}
        host.close()
    }

    @MainActor
    func testCancellingTheTaskEndsARefreshInRust() async throws {
        let (host, model, server) = try await stalledRefresh()
        let started = Date()
        let refreshing = Task { await model.refresh() }
        try await waitUntilRefreshing(model)
        refreshing.cancel()
        await refreshing.value
        XCTAssertLessThan(Date().timeIntervalSince(started), 10)
        XCTAssertFalse(model.isRefreshing)
        XCTAssertNil(model.lastRefresh)
        XCTAssertNil(model.alert)
        withExtendedLifetime(server) {}
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
    func testSettingsAreRustsAndKeptAcrossLaunches() async throws {
        let dir = try makeDirectory()
        // The first launch's model and core go out of scope before the
        // relaunch: the core holds the library's lock until it is released.
        do {
            let (host, model) = try await open(dir)
            XCTAssertEqual(
                model.settings,
                AppSettings(oldestFirst: false, hideRead: false, showSummaries: true, openLinksInApp: true))
            await model.update { $0.hideRead = true }
            XCTAssertTrue(model.settings.hideRead)
            XCTAssertNil(model.alert)
            host.close()
        }
        let (host, model) = try await open(dir)
        XCTAssertTrue(model.settings.hideRead)
        host.close()
    }

    @MainActor
    func testAnEmptySearchFindsNothing() async throws {
        let (host, model) = try await open(try makeDirectory())
        let found = await model.search("  ")
        XCTAssertEqual(found, [])
        let none = await model.search("anything")
        XCTAssertEqual(none, [])
        XCTAssertNil(model.alert)
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
