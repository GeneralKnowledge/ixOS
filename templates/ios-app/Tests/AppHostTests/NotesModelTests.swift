import FORGE_MODULE_NAMEApp
@preconcurrency import FORGE_MODULE_NAMEFFI
import ForgeAppKit
import Foundation
import XCTest

#if canImport(Glibc)
    import Glibc
    private let streamSocket = Int32(SOCK_STREAM.rawValue)
#else
    import Darwin
    private let streamSocket = SOCK_STREAM
#endif

// File-scope wrappers: inside a test case, NSObject's `bind` method would
// shadow the socket function.
private func openSocket() -> Int32 { socket(AF_INET, streamSocket, 0) }
private func bindSocket(_ fd: Int32, _ sa: UnsafePointer<sockaddr>, _ len: socklen_t) -> Int32 {
    bind(fd, sa, len)
}
private func listenSocket(_ fd: Int32) -> Int32 { listen(fd, 4) }
private func socketName(_ fd: Int32, _ sa: UnsafeMutablePointer<sockaddr>, _ len: inout socklen_t) -> Int32 {
    getsockname(fd, sa, &len)
}
private func closeSocket(_ fd: Int32) { _ = close(fd) }

/// Drives the view model and ForgeKit's host against the app's real Rust
/// core (rust/) through the generated Swift bindings. `forge test ios` (or
/// scripts/test-swift-host.sh) builds the host library first. It imports no
/// SwiftUI, so `forge check swift` also runs it on Linux.
final class NotesModelTests: XCTestCase {
    private func makeDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-notes-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    /// A host whose data lives in `dir`, as the app's own would in
    /// Application Support.
    @MainActor
    private func makeHost(_ dir: URL) -> ForgeHost<AppCore> {
        let environment = ForgeEnvironment(dataDirectory: dir.path, cachesDirectory: dir.path)
        return ForgeHost(environment: { environment }) { env in
            try AppCore(storageDirectory: env.dataDirectory)
        }
    }

    @MainActor
    private func open(_ dir: URL) async throws -> (ForgeHost<AppCore>, NotesModel) {
        let host = makeHost(dir)
        await host.start()
        XCTAssertEqual(host.status, .ready)
        return (host, NotesModel(core: try XCTUnwrap(host.core)))
    }

    @MainActor
    private func eventually(_ what: String, _ condition: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(5)
        while !condition(), Date() < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertTrue(condition(), "timed out waiting for \(what)")
    }

    @MainActor
    func testNotesAreSavedAndSurviveARestart() async throws {
        let dir = try makeDirectory()
        do {
            let (host, model) = try await open(dir)
            XCTAssertEqual(model.notes, [])

            try await model.add(text: "first")
            try await model.add(text: "second\nwith more")
            XCTAssertEqual(model.notes.map(\.text), ["second\nwith more", "first"])
            XCTAssertEqual(model.notes[0].title, "second")
            XCTAssertEqual(model.notes[0].preview, "with more")
            XCTAssertEqual(model.saveCount, 2)
            host.close()
        }
        // The core is released with the host and model above, so the next
        // launch can open the same folder.
        let (host, model) = try await open(dir)
        XCTAssertEqual(model.notes.map(\.text), ["second\nwith more", "first"])
        await model.delete(model.notes[1])
        XCTAssertEqual(model.notes.map(\.text), ["second\nwith more"])
        host.close()
    }

    @MainActor
    func testEditingAndSearchGoThroughRust() async throws {
        let (host, model) = try await open(try makeDirectory())
        try await model.add(text: "Buy milk")
        try await model.add(text: "Call Sam")
        let milk = try XCTUnwrap(model.notes.last)

        let saved = await model.update(milk, text: "Buy oat milk")
        XCTAssertTrue(saved)
        XCTAssertEqual(model.note(id: milk.id)?.text, "Buy oat milk")

        model.searchText = "MILK"
        XCTAssertEqual(model.visibleNotes.map(\.text), ["Buy oat milk"])
        model.searchText = "eggs"
        XCTAssertEqual(model.visibleNotes, [])
        model.searchText = ""
        XCTAssertEqual(model.visibleNotes.count, 2)

        // Rust's validation reaches the alert, and nothing changes.
        let refused = await model.update(milk, text: "   ")
        XCTAssertFalse(refused)
        XCTAssertNotNil(model.alert)
        XCTAssertEqual(model.alert?.buttons.map(\.identifier), ["alert.ok"])
        XCTAssertEqual(model.note(id: milk.id)?.text, "Buy oat milk")
        host.close()
    }

    @MainActor
    func testSwipeDeleteAsksFirst() async throws {
        let (host, model) = try await open(try makeDirectory())
        try await model.add(text: "Keep me")
        let note = try XCTUnwrap(model.notes.first)

        model.askToDelete(note)
        XCTAssertEqual(model.notes.count, 1, "asking deletes nothing")
        let confirm = try XCTUnwrap(model.alert?.action(for: "Delete Note"))
        await model.handle(confirm)
        XCTAssertEqual(model.notes, [])
        host.close()
    }

    func testErrorAlertsSayWhatHappened() {
        XCTAssertEqual(AppError.Offline(reason: "no route").failure.title, "You're Offline")
        XCTAssertTrue(AppError.Offline(reason: "no route").failure.isRetryable)
        XCTAssertFalse(
            AppError.Storage(reason: "disk I/O").failure.message.contains("disk I/O"),
            "diagnostics stay in the log")
    }

    @MainActor
    func testRustErrorsArriveTyped() async throws {
        let (host, model) = try await open(try makeDirectory())
        do {
            try await model.add(text: "   ")
            XCTFail("expected InvalidInput")
        } catch AppError.InvalidInput {}
        XCTAssertEqual(model.notes, [])

        do {
            try await model.core.deleteNote(id: 42)
            XCTFail("expected NotFound")
        } catch AppError.NotFound(let id) {
            XCTAssertEqual(id, 42)
        }
        // Each error has a framework kind, for ForgeKit's default text.
        XCTAssertEqual(AppError.Interrupted.forgeKind, .interrupted)
        host.close()
    }

    @MainActor
    func testTheHostHearsChangesCommittedInRust() async throws {
        let (host, model) = try await open(try makeDirectory())
        let before = host.changes
        _ = try await model.core.addNote(text: "from Rust")
        // What the screen does when `changes` moves.
        try await eventually("Changed from Rust") { host.changes > before }
        model.reload()
        XCTAssertEqual(model.notes.map(\.text), ["from Rust"])
        XCTAssertEqual(host.revision, 1)
        host.close()
    }

    @MainActor
    func testOneCorePerDataFolder() async throws {
        let dir = try makeDirectory()
        let (first, _) = try await open(dir)
        let second = makeHost(dir)
        await second.start()
        guard case .failed = second.status else {
            return XCTFail("a second core opened the same notes: \(second.status)")
        }
        first.close()
    }

    @MainActor
    func testUnreadableDataIsReportedNotOverwritten() async throws {
        let dir = try makeDirectory()
        let file = dir.appendingPathComponent("notes.json")
        let newer = #"{"schema_version": 99, "data": {}}"#
        try newer.write(to: file, atomically: true, encoding: .utf8)
        let host = makeHost(dir)
        await host.start()
        guard case .failed = host.status else {
            return XCTFail("expected the failed status, got \(host.status)")
        }
        XCTAssertNil(host.core)
        XCTAssertEqual(try String(contentsOf: file, encoding: .utf8), newer)
    }

    /// A local port that accepts connections and never answers, so a request
    /// to it only ends when cancelled.
    private func silentServerURL() throws -> (String, Int32) {
        let fd = openSocket()
        var addr = sockaddr_in()
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")
        addr.sin_port = 0
        var len = socklen_t(MemoryLayout<sockaddr_in>.size)
        try withUnsafeMutablePointer(to: &addr) { ptr in
            try ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                guard bindSocket(fd, sa, len) == 0, listenSocket(fd) == 0,
                    socketName(fd, sa, &len) == 0
                else {
                    closeSocket(fd)
                    throw XCTSkip("cannot open a local socket")
                }
            }
        }
        return ("http://127.0.0.1:\(UInt16(bigEndian: addr.sin_port))/", fd)
    }

    @MainActor
    func testCancellingTheTaskStopsTheRequestInRust() async throws {
        let (host, model) = try await open(try makeDirectory())
        let (url, fd) = try silentServerURL()
        defer { closeSocket(fd) }
        model.url = url
        let started = Date()
        let check = Task { await model.checkURL() }
        try await Task.sleep(for: .milliseconds(200))
        check.cancel()
        await check.value
        XCTAssertEqual(model.urlResult, "Cancelled.")
        XCTAssertLessThan(Date().timeIntervalSince(started), 5)
        XCTAssertFalse(model.isCheckingURL)
        host.close()
    }

    @MainActor
    func testGoingToTheBackgroundInterruptsTheRequestInRust() async throws {
        let (host, model) = try await open(try makeDirectory())
        host.report(.active)
        let (url, fd) = try silentServerURL()
        defer { closeSocket(fd) }
        model.url = url
        let started = Date()
        let check = Task { await model.checkURL() }
        try await Task.sleep(for: .milliseconds(200))
        host.report(.background)
        await check.value
        XCTAssertEqual(model.urlResult, NotesModel.describe(AppError.Interrupted))
        XCTAssertLessThan(Date().timeIntervalSince(started), 5)
        XCTAssertEqual(model.core.phase(), .background)

        // Back on screen, the host tells the screens to re-read.
        let changes = host.changes
        host.report(.active)
        try await eventually("resume announced") { host.changes > changes }
        host.close()
    }

    @MainActor
    func testTerminationShutsTheCoreDownAndKeepsWhatWasSaved() async throws {
        let dir = try makeDirectory()
        // Reported before Rust is open: applied when it opens.
        let host = makeHost(dir)
        host.report(.active)
        await host.start()
        let core = try XCTUnwrap(host.core)
        XCTAssertEqual(core.phase(), .active)
        _ = try await core.addNote(text: "kept")

        host.report(.terminated)
        XCTAssertNil(host.core)
        XCTAssertEqual(core.phase(), .terminated)
        do {
            _ = try await core.addNote(text: "late")
            XCTFail("expected ShutDown")
        } catch AppError.ShutDown {}
    }
}
