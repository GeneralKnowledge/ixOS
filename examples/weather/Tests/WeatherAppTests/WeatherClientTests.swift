import Foundation
import WeatherBridge
import XCTest

/// Integration tests for the Swift bridge against the real Rust core and its
/// real HTTP client, using local endpoints only (no internet needed).
/// Run with scripts/test-swift-host.sh, which builds the host library first.
final class WeatherClientTests: XCTestCase {
    private func makeDirectory() throws -> String {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-weather-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url.path
    }

    /// A client whose endpoints are `baseURL`. Nothing listens on port 9, so
    /// the default is a reliable "offline".
    private func client(baseURL: String = "http://127.0.0.1:9", timeoutMs: UInt32 = 5_000)
        throws -> WeatherClient
    {
        let client = try WeatherClient(
            storageDirectory: try makeDirectory(),
            configuration: WeatherConfig(
                forecastBaseUrl: baseURL, geocodingBaseUrl: baseURL, requestTimeoutMs: timeoutMs))
        addTeardownBlock { client.shutdown() }
        return client
    }

    private let paris = Place(
        name: "Paris", region: "Île-de-France", country: "France", countryCode: "FR",
        coordinate: Coordinate(latitude: 48.8534, longitude: 2.3488),
        timezone: "Europe/Paris", population: 2_138_551, source: .manual)

    func testFreshClientHasNoCache() throws {
        let client = try client()
        XCTAssertNil(client.cachedReport)
        XCTAssertFalse(client.isRefreshing)
    }

    func testOfflineWithoutCacheThrowsOffline() async throws {
        let client = try client()
        do {
            _ = try await client.report(for: paris)
            XCTFail("expected an error")
        } catch let error as WeatherClientError {
            guard case .offline = error else {
                return XCTFail("expected .offline, got \(error)")
            }
            XCTAssertFalse(error.userMessage.isEmpty)
        }
    }

    func testInvalidCoordinateIsRejectedBeforeAnyRequest() async throws {
        let client = try client()
        var place = paris
        place.coordinate.latitude = 123
        do {
            _ = try await client.report(for: place)
            XCTFail("expected an error")
        } catch let error as WeatherClientError {
            guard case .invalidInput = error else {
                return XCTFail("expected .invalidInput, got \(error)")
            }
        }
    }

    func testShortSearchIsInvalidAndOfflineSearchIsOffline() async throws {
        let client = try client()
        do {
            _ = try await client.searchPlaces("x")
            XCTFail("expected an error")
        } catch let error as WeatherClientError {
            guard case .invalidInput = error else { return XCTFail("\(error)") }
        }
        do {
            _ = try await client.searchPlaces("Paris")
            XCTFail("expected an error")
        } catch let error as WeatherClientError {
            guard case .offline = error else { return XCTFail("\(error)") }
        }
    }

    func testBadConfigurationIsRejected() throws {
        XCTAssertThrowsError(
            try WeatherClient(
                storageDirectory: try makeDirectory(),
                configuration: WeatherConfig(forecastBaseUrl: "ftp://example.com"))
        ) { error in
            guard case WeatherClientError.invalidInput = error else {
                return XCTFail("\(error)")
            }
        }
    }

    /// The server accepts the connection and never answers, so the request
    /// would hang until its 30 s timeout. Cancelling the task must end the
    /// call at once with `CancellationError`.
    func testCancellingTheTaskCancelsTheRefreshAtOnce() async throws {
        let server = try SilentServer()
        let client = try client(baseURL: "http://127.0.0.1:\(server.port)", timeoutMs: 30_000)
        let place = paris
        let task = Task { try await client.report(for: place) }
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertTrue(client.isRefreshing)

        let started = ContinuousClock.now
        task.cancel()
        do {
            _ = try await task.value
            XCTFail("expected cancellation")
        } catch is CancellationError {
            XCTAssertLessThan(ContinuousClock.now - started, .seconds(2))
        }
        XCTAssertFalse(client.isRefreshing)
        XCTAssertNil(client.cachedReport)
        _ = server
    }

    func testANewRefreshSupersedesTheOldOne() async throws {
        let server = try SilentServer()
        let client = try client(baseURL: "http://127.0.0.1:\(server.port)", timeoutMs: 30_000)
        let place = paris
        let first = Task { try await client.report(for: place) }
        try await Task.sleep(for: .milliseconds(300))
        let second = Task { try await client.report(for: place) }
        do {
            _ = try await first.value
            XCTFail("expected the first refresh to be cancelled")
        } catch is CancellationError {}
        second.cancel()
        _ = try? await second.value
        _ = server
    }

    func testShutdownRefusesNewWork() async throws {
        let client = try client()
        client.shutdown()
        XCTAssertTrue(client.isShutDown)
        do {
            _ = try await client.report(for: paris)
            XCTFail("expected an error")
        } catch let error as WeatherClientError {
            XCTAssertEqual(error, .closed)
        }
    }
}

/// A TCP listener on 127.0.0.1 that never accepts or answers. Connections
/// complete through the kernel backlog, then the client waits forever.
final class SilentServer: @unchecked Sendable {
    let port: UInt16
    private let fd: Int32

    init() throws {
        let socketFD = socket(AF_INET, SOCK_STREAM, 0)
        guard socketFD >= 0 else { throw POSIXError(.EIO) }
        var addr = sockaddr_in()
        addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = 0
        addr.sin_addr.s_addr = in_addr_t(0x7F00_0001).bigEndian
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(socketFD, $0, length) }
        }
        guard bound == 0, listen(socketFD, 16) == 0 else {
            close(socketFD)
            throw POSIXError(.EADDRINUSE)
        }
        let named = withUnsafeMutablePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(socketFD, $0, &length) }
        }
        guard named == 0 else {
            close(socketFD)
            throw POSIXError(.EIO)
        }
        fd = socketFD
        port = UInt16(bigEndian: addr.sin_port)
    }

    deinit {
        close(fd)
    }
}
