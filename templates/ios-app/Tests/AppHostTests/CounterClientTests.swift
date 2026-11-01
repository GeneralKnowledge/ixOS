import CounterBridge
import Foundation
import XCTest

/// Integration tests for the Swift bridge against the real Rust core (no mocks).
/// Run with scripts/test-swift-host.sh, which builds the host library first.
final class CounterClientTests: XCTestCase {
    private func makeDirectory() throws -> String {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-client-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url.path
    }

    private func open(_ dir: String? = nil) async throws -> CounterClient {
        let client = try await CounterClient.open(storageDirectory: try dir ?? makeDirectory())
        addTeardownBlock { await client.shutdown() }
        return client
    }

    /// Collects events until `stop` returns true (or the stream ends).
    private func collect(
        _ events: CounterEvents, until stop: @escaping @Sendable (CounterEvent) -> Bool
    ) -> Task<[CounterEvent], Never> {
        Task {
            var seen: [CounterEvent] = []
            for await event in events {
                seen.append(event)
                if stop(event) { break }
            }
            return seen
        }
    }

    func testOpenApplyAndQuery() async throws {
        let dir = try makeDirectory()
        let client = try await open(dir)
        XCTAssertEqual(client.startupKind, .freshInstall)
        let state = try await client.apply(.increment)
        XCTAssertEqual(state.count, 1)
        XCTAssertEqual(try client.currentState().count, 1)
        await client.shutdown()

        let reopened = try await open(dir)
        XCTAssertEqual(reopened.startupKind, .restored)
        XCTAssertEqual(try reopened.currentState().count, 1)
    }

    func testConcurrentRequestsAreAllApplied() async throws {
        let client = try await open()
        try await withThrowingTaskGroup(of: Void.self) { group in
            for _ in 0..<50 {
                group.addTask { _ = try await client.apply(.increment) }
            }
            try await group.waitForAll()
        }
        XCTAssertEqual(try client.currentState().count, 50)
    }

    func testDomainErrorIsTranslated() async throws {
        let client = try await open()
        do {
            _ = try await client.apply(.decrement)
            XCTFail("expected an error")
        } catch let error as CounterError {
            XCTAssertEqual(error, .belowMinimum)
            XCTAssertEqual(error.category, .domain)
        }
        XCTAssertEqual(try client.currentState().count, 0)
    }

    func testInvalidInputAndBusyAreTranslated() async throws {
        let client = try await open()
        do {
            _ = try await client.bulkAdd(0)
            XCTFail("expected an error")
        } catch let error as CounterError {
            guard case .invalidInput = error else { return XCTFail("got \(error)") }
        }

        let long = Task { try await client.bulkAdd(10, stepDelay: .seconds(10)) }
        while client.activeOperation == nil { await Task.yield() }
        do {
            _ = try await client.bulkAdd(1)
            XCTFail("expected busy")
        } catch let error as CounterError {
            XCTAssertEqual(error, .busy)
        }
        long.cancel()
        _ = await long.result
    }

    func testCorruptAndNewerDataAreTranslated() async throws {
        let dir = try makeDirectory()
        let file = URL(fileURLWithPath: dir).appendingPathComponent("forge-counter-v1.json")
        try Data("{bad".utf8).write(to: file)
        do {
            _ = try await CounterClient.open(storageDirectory: dir)
            XCTFail("expected corrupt data")
        } catch let error as CounterError {
            guard case .corruptData = error else { return XCTFail("got \(error)") }
        }
        let recovered = try await CounterClient.open(storageDirectory: dir, recoverCorruptData: true)
        XCTAssertEqual(try recovered.currentState().count, 0)
        await recovered.shutdown()

        try Data(#"{"schema_version": 999, "count": 7}"#.utf8).write(to: file)
        do {
            _ = try await CounterClient.open(storageDirectory: dir, recoverCorruptData: true)
            XCTFail("expected newer data")
        } catch let error as CounterError {
            XCTAssertEqual(error, .newerDataVersion(found: 999, supported: 1))
        }
    }

    func testBulkAddDeliversProgressAndCompletionEvents() async throws {
        let client = try await open()
        let events = try client.events()
        let seen = collect(events) {
            if case .operationFinished = $0 { return true } else { return false }
        }
        let state = try await client.bulkAdd(40, stepDelay: .milliseconds(5))
        XCTAssertEqual(state.count, 40)

        let received = await seen.value
        let progress = received.compactMap { event -> UInt32? in
            if case let .operationProgress(p) = event { return p.completed } else { return nil }
        }
        XCTAssertFalse(progress.isEmpty)
        XCTAssertEqual(progress.last, 40)
        XCTAssertTrue(received.contains(.stateChanged(CounterState(count: 40))))
        guard case let .operationFinished(_, result) = received.last else {
            return XCTFail("missing completion event")
        }
        XCTAssertEqual(result, .completed(CounterState(count: 40)))
    }

    func testCancellingTheTaskCancelsTheRustOperation() async throws {
        let client = try await open()
        let events = try client.events()
        let seen = collect(events) {
            if case .operationFinished = $0 { return true } else { return false }
        }
        let task = Task { try await client.bulkAdd(100, stepDelay: .seconds(10)) }
        while client.activeOperation == nil { await Task.yield() }
        let started = Date()
        task.cancel()
        do {
            _ = try await task.value
            XCTFail("expected cancellation")
        } catch {
            XCTAssertTrue(error is CancellationError, "got \(error)")
        }
        XCTAssertLessThan(Date().timeIntervalSince(started), 5, "Rust stopped promptly")
        XCTAssertEqual(try client.currentState().count, 0, "nothing was committed")
        XCTAssertNil(client.activeOperation)
        guard case let .operationFinished(_, result) = await seen.value.last else {
            return XCTFail("missing completion event")
        }
        XCTAssertEqual(result, .cancelled)
    }

    func testAlreadyCancelledTaskSendsNothing() async throws {
        let client = try await open()
        let task = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return try await client.apply(.increment)
        }
        do {
            _ = try await task.value
            XCTFail("expected cancellation")
        } catch {
            XCTAssertTrue(error is CancellationError)
        }
        XCTAssertEqual(try client.currentState().count, 0)
    }

    func testCancellingTheConsumerEndsTheSubscription() async throws {
        let client = try await open()
        let events = try client.events()
        XCTAssertEqual(client.subscriptionCount, 1)
        let consumer = Task {
            for await _ in events {}
        }
        consumer.cancel()
        await consumer.value
        XCTAssertEqual(client.subscriptionCount, 0)
    }

    func testShutdownEndsEventStreamsAndRefusesCommands() async throws {
        let client = try await CounterClient.open(storageDirectory: try makeDirectory())
        let events = try client.events()
        let consumer = Task { () -> Int in
            var count = 0
            for await _ in events { count += 1 }
            return count
        }
        _ = try await client.apply(.increment)
        await client.shutdown()
        // The loop ends on its own once the instance has shut down.
        let delivered = await consumer.value
        XCTAssertEqual(delivered, 1)
        XCTAssertTrue(client.isShutDown)
        do {
            _ = try await client.apply(.increment)
            XCTFail("expected closed")
        } catch let error as CounterError {
            XCTAssertEqual(error, .closed)
            XCTAssertEqual(error.category, .lifecycle)
        }
        XCTAssertEqual(try client.currentState().count, 1, "queries still answer")
        await client.shutdown()  // idempotent
    }

    func testShutdownCancelsARunningOperation() async throws {
        let client = try await CounterClient.open(storageDirectory: try makeDirectory())
        let task = Task { try await client.bulkAdd(100, stepDelay: .seconds(10)) }
        while client.activeOperation == nil { await Task.yield() }
        await client.shutdown()
        do {
            _ = try await task.value
            XCTFail("expected cancellation")
        } catch {
            XCTAssertTrue(error is CancellationError, "got \(error)")
        }
        XCTAssertEqual(try client.currentState().count, 0)
    }
}
