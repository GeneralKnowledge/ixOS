import ChatBridge
import Foundation
import XCTest

/// The Swift bridge against the real Rust core, its real HTTP client, and
/// (when available) the real mock server. No internet needed.
final class ChatClientTests: XCTestCase {
    /// Nothing listens on port 9, so this is a reliable "server down".
    private func offlineClient() throws -> ChatClient {
        let client = try ChatClient(
            storageDirectory: try temporaryDirectory(self), serverURL: "http://127.0.0.1:9",
            requestTimeoutMs: 2_000)
        addTeardownBlock { client.shutdown() }
        return client
    }

    func testBadServerURLIsInvalidInput() throws {
        XCTAssertThrowsError(try ChatClient(storageDirectory: "", serverURL: "localhost:8787")) {
            guard case .invalidInput = $0 as? ChatClientError else {
                return XCTFail("expected .invalidInput, got \($0)")
            }
        }
    }

    func testServerDownIsATypedNetworkError() async throws {
        let client = try offlineClient()
        do {
            _ = try await client.conversations()
            XCTFail("expected an error")
        } catch let error as ChatClientError {
            guard case .network = error else { return XCTFail("expected .network, got \(error)") }
            XCTAssertFalse(error.userMessage.isEmpty)
        }
    }

    func testFailedSendIsStoredAsFailedAndAnnounced() async throws {
        let client = try offlineClient()
        let events = try client.events()
        do {
            try await client.send("hello", to: "general")
            XCTFail("expected an error")
        } catch let error as ChatClientError {
            guard case .network = error else { return XCTFail("expected .network, got \(error)") }
        }
        let stored = client.cachedMessages(in: "general")
        XCTAssertEqual(stored.count, 1)
        XCTAssertEqual(stored.first?.status, .failed)

        var iterator = events.makeAsyncIterator()
        guard case let .messageReceived(first)? = await iterator.next() else {
            return XCTFail("expected messageReceived first")
        }
        XCTAssertEqual(first.status, .sending)
        let second = await iterator.next()
        XCTAssertEqual(second, ChatEvent.messageStatusChanged(id: first.id, status: .failed))
    }

    func testEmptyMessageIsRejected() async throws {
        let client = try offlineClient()
        do {
            try await client.send("   ", to: "general")
            XCTFail("expected an error")
        } catch let error as ChatClientError {
            guard case .invalidInput = error else {
                return XCTFail("expected .invalidInput, got \(error)")
            }
        }
        XCTAssertTrue(client.cachedMessages(in: "general").isEmpty)
    }

    func testShutdownEndsEventsAndRefusesCommands() async throws {
        let client = try offlineClient()
        let events = try client.events()
        try client.enter("general")
        client.shutdown()
        var sawShutDown = false
        for await event in events {
            if case let .streamDisconnected(_, reason, willRetry) = event {
                sawShutDown = reason == "shut down" && !willRetry
            }
        }
        XCTAssertTrue(sawShutDown)
        XCTAssertTrue(client.isShutDown)
        do {
            _ = try await client.conversations()
            XCTFail("expected an error")
        } catch let error as ChatClientError {
            XCTAssertEqual(error, .closed)
        }
    }

    func testCancellingTheIteratingTaskEndsTheStream() async throws {
        let client = try offlineClient()
        let events = try client.events()
        let task = Task {
            var count = 0
            for await _ in events { count += 1 }
            return count
        }
        try await Task.sleep(nanoseconds: 50_000_000)
        task.cancel()
        let count = await task.value
        XCTAssertEqual(count, 0)
    }

    // MARK: Against the mock server

    func testDemoInteractionAgainstTheMockServer() async throws {
        let server = try MockServer.start()
        let client = try ChatClient(
            storageDirectory: try temporaryDirectory(self), serverURL: server.url,
            requestTimeoutMs: 2_000)
        defer { client.shutdown() }
        // Someone else, posting like curl would.
        let other = try ChatClient(storageDirectory: "", serverURL: server.url, user: "curl")
        defer { other.shutdown() }

        let rooms = try await client.conversations()
        XCTAssertEqual(rooms.map(\.id), ["general", "random", "forge"])

        let events = try client.events()
        let collector = EventCollector(events)
        try client.enter("general")
        await collector.wait("connected") {
            if case .streamConnected("general") = $0 { return true }
            return false
        }
        _ = try await client.syncMessages(in: "general")

        // 1. Send: Sent.
        let first = try await client.send("first", to: "general")
        XCTAssertEqual(first.status, .sent)

        // 6. A message from someone else arrives through the stream.
        let external = try await other.send("hello from curl", to: "general")
        await collector.wait("external message") {
            if case let .messageReceived(message) = $0 { return message.id == external.id }
            return false
        }

        // 3–5. Kill the server; sending fails with a typed error and a
        // Failed message, which stays retryable.
        client.leave("general")
        server.stop()
        do {
            try await client.send("second", to: "general")
            XCTFail("expected an error")
        } catch let error as ChatClientError {
            guard case .network = error else { return XCTFail("expected .network, got \(error)") }
        }
        XCTAssertTrue(client.cachedMessages(in: "general").contains { $0.status == .failed })

        // Leaving the room stops the Rust thread behind it.
        let deadline = Date().addingTimeInterval(10)
        while client.liveStreams > 0 && Date() < deadline {
            try await Task.sleep(nanoseconds: 20_000_000)
        }
        XCTAssertEqual(client.liveStreams, 0)
        collector.stop()
    }
}

/// Collects events on a background task so tests can wait for one.
final class EventCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var seen: [ChatEvent] = []
    private var task: Task<Void, Never>?

    init(_ events: ChatEvents) {
        task = Task { [weak self] in
            for await event in events {
                guard let self else { break }
                self.lock.lock()
                self.seen.append(event)
                self.lock.unlock()
            }
        }
    }

    func snapshot() -> [ChatEvent] {
        lock.lock()
        defer { lock.unlock() }
        return seen
    }

    func wait(
        _ what: String, timeout: TimeInterval = 10, file: StaticString = #filePath,
        line: UInt = #line, _ matches: @escaping (ChatEvent) -> Bool
    ) async {
        let deadline = Date().addingTimeInterval(timeout)
        while !snapshot().contains(where: matches) {
            if Date() > deadline {
                XCTFail("timed out waiting for \(what); saw \(snapshot())", file: file, line: line)
                return
            }
            try? await Task.sleep(nanoseconds: 20_000_000)
        }
    }

    func stop() {
        task?.cancel()
    }
}
