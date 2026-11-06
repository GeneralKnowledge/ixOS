import AppHost
@preconcurrency import ForgeFFI
import Foundation
import XCTest

/// Drives the view model against the app's real Rust core (rust/) through the
/// generated Swift bindings. `forge test ios` (or scripts/test-swift-host.sh)
/// builds the host library first.
final class NotesModelTests: XCTestCase {
    private func makeDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-notes-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    @MainActor
    private func openModel(_ dir: URL) async -> NotesModel {
        let model = NotesModel(storageDirectory: dir.path)
        await model.open()
        return model
    }

    @MainActor
    func testNotesAreSavedAndSurviveARestart() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        XCTAssertNil(model.openError)
        XCTAssertEqual(model.notes, [])

        model.draft = "first"
        await model.addNote()
        model.draft = "second"
        await model.addNote()
        XCTAssertEqual(model.notes.map(\.text), ["second", "first"])
        XCTAssertEqual(model.draft, "")
        model.close()

        let reopened = await openModel(dir)
        XCTAssertEqual(reopened.notes.map(\.text), ["second", "first"])
        await reopened.delete(reopened.notes[1])
        XCTAssertEqual(reopened.notes.map(\.text), ["second"])
        reopened.close()
    }

    @MainActor
    func testRustErrorsArriveTyped() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        model.draft = "   "
        await model.addNote()
        XCTAssertNotNil(model.errorMessage)
        XCTAssertEqual(model.notes, [])

        let core = try XCTUnwrap(model.core)
        do {
            try await core.deleteNote(id: 42)
            XCTFail("expected NotFound")
        } catch AppError.NotFound(let id) {
            XCTAssertEqual(id, 42)
        }
        model.close()
    }

    @MainActor
    func testChangesMadeInRustReachTheModelThroughEvents() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        let core = try XCTUnwrap(model.core)
        _ = try await core.addNote(text: "from Rust")
        let deadline = Date().addingTimeInterval(5)
        while model.notes.isEmpty, Date() < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertEqual(model.notes.map(\.text), ["from Rust"])
        model.close()
    }

    @MainActor
    func testUnreadableDataIsReportedNotOverwritten() async throws {
        let dir = try makeDirectory()
        let file = dir.appendingPathComponent("notes.json")
        let newer = #"{"schema_version": 99, "data": {}}"#
        try newer.write(to: file, atomically: true, encoding: .utf8)
        let model = await openModel(dir)
        XCTAssertNotNil(model.openError)
        XCTAssertNil(model.core)
        XCTAssertEqual(try String(contentsOf: file, encoding: .utf8), newer)
    }

    @MainActor
    func testClosedModelRefusesWork() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        let core = try XCTUnwrap(model.core)
        model.close()
        do {
            _ = try await core.addNote(text: "late")
            XCTFail("expected ShutDown")
        } catch AppError.ShutDown {}
    }
}
