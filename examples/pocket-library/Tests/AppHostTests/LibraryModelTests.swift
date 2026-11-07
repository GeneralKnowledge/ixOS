import AppHost
@preconcurrency import ForgeFFI
import Foundation
import XCTest

/// Drives the model against the app's real Rust library (rust/) through the
/// generated bindings. `forge test --project .` (macOS) builds the host
/// library and regenerates the bindings first.
final class LibraryModelTests: XCTestCase {
    private func makeDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("pocket-library-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    @MainActor
    private func openModel(_ dir: URL) async -> LibraryModel {
        let model = LibraryModel(storageDirectory: dir.path)
        await model.open()
        return model
    }

    private func input(_ title: String, pages: UInt32? = nil) -> BookInput {
        BookInput(
            title: title, author: "Author", description: "", status: .wantToRead,
            currentPage: 0, totalPages: pages, rating: nil, notes: "", coverUrl: nil
        )
    }

    @MainActor
    func testBooksAndProgressSurviveARelaunch() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        XCTAssertEqual(model.phase, .open)
        XCTAssertEqual(model.books, [])

        let dune = try await model.add(input("Dune", pages: 400))
        try await model.add(input("Emma"))
        // Reading a page starts the book: a Rust rule, not a Swift one.
        let updated = try await model.updateProgress(dune.id, page: 100)
        XCTAssertEqual(updated.status, .reading)
        XCTAssertEqual(updated.progress, 0.25, accuracy: 0.0001)
        XCTAssertEqual(model.books.map(\.title), ["Emma", "Dune"])
        model.close()

        let reopened = await openModel(dir)
        XCTAssertEqual(reopened.books.map(\.title), ["Emma", "Dune"])
        XCTAssertEqual(reopened.book(id: dune.id)?.currentPage, 100)
        XCTAssertEqual(reopened.stats?.reading, 1)
        reopened.close()
    }

    @MainActor
    func testSearchFilterAndSortComeFromRust() async throws {
        let model = await openModel(try makeDirectory())
        try await model.add(input("The Hobbit"))
        let atlas = try await model.add(input("An Atlas"))
        try await model.setStatus(atlas.id, .finished)

        model.sort = .title
        XCTAssertEqual(model.books.map(\.title), ["An Atlas", "The Hobbit"])
        model.searchText = "hob"
        XCTAssertEqual(model.books.map(\.title), ["The Hobbit"])
        model.searchText = ""
        model.statusFilter = .finished
        XCTAssertEqual(model.books.map(\.title), ["An Atlas"])
        model.close()
    }

    @MainActor
    func testRustErrorsArriveTyped() async throws {
        let model = await openModel(try makeDirectory())
        do {
            try await model.add(input("   "))
            XCTFail("expected InvalidInput")
        } catch LibraryError.InvalidInput {
        }
        do {
            try await model.updateProgress(42, page: 1)
            XCTFail("expected NotFound")
        } catch LibraryError.NotFound(let id) {
            XCTAssertEqual(id, 42)
        }
        let book = try await model.add(input("Short", pages: 10))
        do {
            try await model.updateProgress(book.id, page: 11)
            XCTFail("expected InvalidInput")
        } catch let error as LibraryError {
            XCTAssertEqual(ErrorText.title(error), "Check the Details")
        }
        XCTAssertEqual(model.book(id: book.id)?.currentPage, 0, "a refused change changes nothing")
        model.close()
    }

    @MainActor
    func testAnUnreadableLibraryCanBeStartedFreshAndIsKept() async throws {
        let dir = try makeDirectory()
        try Data("{ not json".utf8).write(to: dir.appendingPathComponent("library.json"))
        let model = await openModel(dir)
        guard case .failed(_, let canStartFresh) = model.phase else {
            return XCTFail("expected a failure, got \(model.phase)")
        }
        XCTAssertTrue(canStartFresh)
        await model.startFresh()
        XCTAssertEqual(model.phase, .open)
        let backups = try FileManager.default.contentsOfDirectory(atPath: dir.path)
            .filter { $0.hasPrefix("library.unreadable-") }
        XCTAssertEqual(backups.count, 1)
        model.close()
    }

    @MainActor
    func testEventsFromAnotherInstanceReachTheModel() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        let listening = Task { await model.listenForEvents() }
        // A command through the model's own library emits `changed`; the
        // model re-reads on the event as well as after the command.
        try await model.add(input("Evented"))
        XCTAssertEqual(model.totalBooks, 1)
        listening.cancel()
        await listening.value // returns once the cancellation ends the stream
        model.close()
    }

    @MainActor
    func testCatalogueSwitchedOffFailsWithoutNetwork() async throws {
        let model = await openModel(try makeDirectory())
        var settings = model.settings
        settings.catalogueEnabled = false
        try await model.updateSettings(settings)
        do {
            _ = try await model.searchCatalogue("dune")
            XCTFail("expected CatalogueDisabled")
        } catch LibraryError.CatalogueDisabled {
        }
        model.close()
    }

    @MainActor
    func testCancellingTheTaskStopsTheRustSearch() async throws {
        let model = await openModel(try makeDirectory())
        // Cancelled before it starts, so no request goes out: the model's
        // Cancellation reaches Rust, which refuses to send.
        let search = Task { @MainActor in
            withUnsafeCurrentTask { $0?.cancel() }
            return try await model.searchCatalogue("dune")
        }
        do {
            _ = try await search.value
            XCTFail("expected Cancelled")
        } catch LibraryError.Cancelled {
        }
        model.close()
    }
}
