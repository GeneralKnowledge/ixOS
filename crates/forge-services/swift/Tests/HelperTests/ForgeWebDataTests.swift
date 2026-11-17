import Foundation
import XCTest

@testable import ForgeServicesHelpers

#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// `ForgeWebData` against a stub server (a URLProtocol), with a real
/// URLCache in a temporary directory.
final class ForgeWebDataTests: XCTestCase {
    private var directory: URL!

    override func setUp() {
        super.setUp()
        directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("ForgeWebDataTests-\(UUID().uuidString)", isDirectory: true)
        StubServer.reset()
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: directory)
        super.tearDown()
    }

    private func makeWeb(maxBytes: Int = 1_000) -> ForgeWebData {
        let configuration = URLSessionConfiguration.default
        configuration.protocolClasses = [StubServer.self]
        return ForgeWebData(configuration: configuration, cacheDirectory: directory, maxBytes: maxBytes)
    }

    /// Linux Foundation's URLCache doesn't keep responses from a stub
    /// URLProtocol, so tests that need a copy on disk run on macOS (the App
    /// model workflow on main) and on the phone, not on Linux.
    private func requireDiskCache() throws {
        #if canImport(FoundationNetworking)
            throw XCTSkip("needs Apple's URLCache; runs on macOS")
        #endif
    }

    private func address(_ path: String) -> URL { URL(string: "https://stub.test/\(path)")! }
    private func body(_ text: String) -> Data { Data(text.utf8) }

    // MARK: - Answers

    func testReturnsTheBody() async {
        StubServer.set("a", .init(body: body("picture")))
        let result = await makeWeb().data(from: address("a"))
        XCTAssertEqual(result, .success(body("picture")))
    }

    func testHTTPErrorsAreReportedWithTheirStatus() async {
        let web = makeWeb()
        StubServer.set("missing", .init(status: 404))
        StubServer.set("broken", .init(status: 500, body: body("oops")))
        let missing = await web.data(from: address("missing"))
        let broken = await web.data(from: address("broken"))
        XCTAssertEqual(missing, .failure(.http(404)))
        XCTAssertEqual(broken, .failure(.http(500)))
    }

    // MARK: - Size limit

    func testDeclaredOversizedResponseIsRefused() async {
        let big = Data(repeating: 7, count: 5_000)
        StubServer.set("big", .init(headers: ["Content-Length": "5000"], body: big))
        let result = await makeWeb(maxBytes: 1_000).data(from: address("big"))
        XCTAssertEqual(result, .failure(.tooLarge))
    }

    func testUndeclaredOversizedResponseIsRefusedWhileArriving() async {
        let big = Data(repeating: 7, count: 3_000)
        StubServer.set("stream", .init(body: big, chunks: 6))
        let result = await makeWeb(maxBytes: 1_000).data(from: address("stream"))
        XCTAssertEqual(result, .failure(.tooLarge))
    }

    func testResponseAtTheLimitIsAccepted() async {
        let exact = Data(repeating: 1, count: 1_000)
        StubServer.set("exact", .init(body: exact, chunks: 4))
        let result = await makeWeb(maxBytes: 1_000).data(from: address("exact"))
        XCTAssertEqual(result, .success(exact))
    }

    // MARK: - Sharing and cancelling

    func testTwoRequestsForOneAddressShareOneDownload() async {
        StubServer.set("shared", .init(body: body("once"), delay: 0.3))
        let web = makeWeb()
        async let first = web.data(from: address("shared"))
        async let second = web.data(from: address("shared"))
        let results = await [first, second]
        XCTAssertEqual(results, [.success(body("once")), .success(body("once"))])
        XCTAssertEqual(StubServer.hits("shared"), 1)
    }

    func testCancelledCallerGetsCancelledAtOnceAndTheDownloadStops() async throws {
        StubServer.set("slow", .init(body: body("late"), delay: 3))
        let web = makeWeb()
        let task = Task { await web.data(from: address("slow")) }
        try await Task.sleep(nanoseconds: 200_000_000)
        let cancelledAt = Date()
        task.cancel()
        let result = await task.value
        XCTAssertEqual(result, .failure(.cancelled))
        XCTAssertLessThan(Date().timeIntervalSince(cancelledAt), 1)
        try await Task.sleep(nanoseconds: 300_000_000)
        XCTAssertEqual(StubServer.stops("slow"), 1, "nobody waits, so the download is stopped")
    }

    func testOneCancelledCallerLeavesTheOtherDownloading() async throws {
        StubServer.set("half", .init(body: body("kept"), delay: 0.6))
        let web = makeWeb()
        let leaving = Task { await web.data(from: address("half")) }
        let staying = Task { await web.data(from: address("half")) }
        try await Task.sleep(nanoseconds: 100_000_000)
        leaving.cancel()
        let left = await leaving.value
        let stayed = await staying.value
        XCTAssertEqual(left, .failure(.cancelled))
        XCTAssertEqual(stayed, .success(body("kept")))
        XCTAssertEqual(StubServer.hits("half"), 1)
    }

    // MARK: - Freshness and offline

    func testFreshCopyIsUsedWithoutAskingTheServer() async throws {
        try requireDiskCache()
        StubServer.set("fresh", .init(headers: ["Cache-Control": "max-age=3600"], body: body("v1")))
        let web = makeWeb()
        _ = await web.data(from: address("fresh"))
        StubServer.set("fresh", .init(headers: ["Cache-Control": "max-age=3600"], body: body("v2")))
        let again = await web.data(from: address("fresh"))
        XCTAssertEqual(again, .success(body("v1")))
        XCTAssertEqual(StubServer.hits("fresh"), 1)
    }

    func testChangedPictureAtTheSameAddressIsFetched() async {
        StubServer.set("changes", .init(headers: ["Cache-Control": "no-cache"], body: body("old")))
        let web = makeWeb()
        let first = await web.data(from: address("changes"))
        StubServer.set("changes", .init(headers: ["Cache-Control": "no-cache"], body: body("new")))
        let second = await web.data(from: address("changes"))
        XCTAssertEqual(first, .success(body("old")))
        XCTAssertEqual(second, .success(body("new")))
    }

    func testReloadAsksTheServerPastAFreshCopy() async throws {
        try requireDiskCache()
        StubServer.set("reload", .init(headers: ["Cache-Control": "max-age=3600"], body: body("v1")))
        let web = makeWeb()
        _ = await web.data(from: address("reload"))
        StubServer.set("reload", .init(headers: ["Cache-Control": "max-age=3600"], body: body("v2")))
        let reloaded = await web.data(from: address("reload"), reload: true)
        XCTAssertEqual(reloaded, .success(body("v2")))
        XCTAssertEqual(StubServer.hits("reload"), 2)
    }

    func testForgetRemovesTheDiskCopy() async throws {
        try requireDiskCache()
        StubServer.set("forget", .init(headers: ["Cache-Control": "max-age=3600"], body: body("v1")))
        let web = makeWeb()
        _ = await web.data(from: address("forget"))
        web.forget(address("forget"))
        StubServer.set("forget", .init(headers: ["Cache-Control": "max-age=3600"], body: body("v2")))
        let after = await web.data(from: address("forget"))
        XCTAssertEqual(after, .success(body("v2")))
    }

    func testOfflineUsesTheStaleDiskCopy() async throws {
        try requireDiskCache()
        StubServer.set("offline-hit", .init(headers: ["Cache-Control": "max-age=0"], body: body("saved")))
        let web = makeWeb()
        _ = await web.data(from: address("offline-hit"))
        StubServer.set("offline-hit", .init(error: .notConnectedToInternet))
        let offline = await web.data(from: address("offline-hit"))
        XCTAssertEqual(offline, .success(body("saved")))
    }

    func testOfflineWithNoCopyReportsOffline() async {
        StubServer.set("offline-miss", .init(error: .notConnectedToInternet))
        let result = await makeWeb().data(from: address("offline-miss"))
        XCTAssertEqual(result, .failure(.offline))
    }

    func testOtherFailuresAreNotCalledOffline() async {
        StubServer.set("tls", .init(error: .secureConnectionFailed))
        let result = await makeWeb().data(from: address("tls"))
        XCTAssertEqual(result, .failure(.failed))
    }
}

#if canImport(ImageIO)
final class ForgeImagesDataTests: XCTestCase {
    func testDataThatIsNotAPictureGivesNoThumbnail() async {
        let image = await ForgeImages.thumbnail(of: Data("not a picture".utf8), maxPixels: 64)
        XCTAssertNil(image)
    }
}
#endif

// MARK: - Stub server

/// Answers requests to https://stub.test/<path> with the reply set for that
/// path, counting requests and stops.
final class StubServer: URLProtocol, @unchecked Sendable {
    struct Reply {
        var status = 200
        var headers: [String: String] = [:]
        var body = Data()
        /// Pieces the body arrives in.
        var chunks = 1
        /// Seconds before the answer starts.
        var delay: TimeInterval = 0
        /// Fail with this instead of answering.
        var error: URLError.Code?
    }

    private static let lock = NSLock()
    private static var replies: [String: Reply] = [:]
    private static var hitCounts: [String: Int] = [:]
    private static var stopCounts: [String: Int] = [:]

    static func reset() {
        lock.lock()
        replies = [:]
        hitCounts = [:]
        stopCounts = [:]
        lock.unlock()
    }

    static func set(_ path: String, _ reply: Reply) {
        lock.lock()
        replies["/" + path] = reply
        lock.unlock()
    }

    static func hits(_ path: String) -> Int {
        lock.lock()
        defer { lock.unlock() }
        return hitCounts["/" + path, default: 0]
    }

    static func stops(_ path: String) -> Int {
        lock.lock()
        defer { lock.unlock() }
        return stopCounts["/" + path, default: 0]
    }

    private let stateLock = NSLock()
    private var stopped = false

    override class func canInit(with request: URLRequest) -> Bool {
        request.url?.host == "stub.test"
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let url = request.url!
        let path = url.path
        Self.lock.lock()
        Self.hitCounts[path, default: 0] += 1
        let reply = Self.replies[path] ?? Reply(status: 404)
        Self.lock.unlock()

        DispatchQueue.global().asyncAfter(deadline: .now() + reply.delay) { [self] in
            guard !isStopped else { return }
            if let code = reply.error {
                client?.urlProtocol(self, didFailWithError: URLError(code))
                return
            }
            var headers = reply.headers
            headers["Date"] = headers["Date"] ?? Self.httpDate(Date())
            let response = HTTPURLResponse(
                url: url, statusCode: reply.status, httpVersion: "HTTP/1.1", headerFields: headers)!
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .allowed)
            let size = max(1, (reply.body.count + reply.chunks - 1) / max(reply.chunks, 1))
            var offset = 0
            while offset < reply.body.count, !isStopped {
                let end = min(offset + size, reply.body.count)
                client?.urlProtocol(self, didLoad: reply.body.subdata(in: offset..<end))
                offset = end
            }
            if !isStopped { client?.urlProtocolDidFinishLoading(self) }
        }
    }

    override func stopLoading() {
        stateLock.lock()
        let first = !stopped
        stopped = true
        stateLock.unlock()
        guard first, let path = request.url?.path else { return }
        Self.lock.lock()
        Self.stopCounts[path, default: 0] += 1
        Self.lock.unlock()
    }

    private var isStopped: Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        return stopped
    }

    private static func httpDate(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(identifier: "GMT")
        formatter.dateFormat = "EEE, dd MMM yyyy HH:mm:ss 'GMT'"
        return formatter.string(from: date)
    }
}
