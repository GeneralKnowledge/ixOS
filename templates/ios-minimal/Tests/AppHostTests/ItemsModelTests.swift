import AppHost
@preconcurrency import ForgeFFI
import Foundation
import XCTest

/// Drives the view model against the app's real Rust core (rust/) through
/// the generated Swift bindings. It imports no SwiftUI, so `forge check
/// swift` runs it on Linux; `forge test` runs it on a Mac.
final class ItemsModelTests: XCTestCase {
    private func makeDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-items-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    @MainActor
    func testItemsAreSavedAndSurviveARestart() async throws {
        let dir = try makeDirectory()
        let model = ItemsModel(storageDirectory: dir.path)
        await model.open()
        XCTAssertEqual(model.phase, .ready)
        XCTAssertEqual(model.items, [])

        await model.add(title: "Milk")
        await model.add(title: "Bread")
        XCTAssertEqual(model.items.map(\.title), ["Milk", "Bread"])
        await model.toggle(model.items[0])
        XCTAssertTrue(model.items[0].done)

        let reopened = ItemsModel(storageDirectory: dir.path)
        await reopened.open()
        XCTAssertEqual(reopened.items, model.items)
    }

    @MainActor
    func testABlankTitleShowsAnError() async throws {
        let model = ItemsModel(storageDirectory: try makeDirectory().path)
        await model.open()
        await model.add(title: "   ")
        XCTAssertEqual(model.items, [])
        XCTAssertNotNil(model.errorMessage)
    }

    @MainActor
    func testDeleteRemovesTheItem() async throws {
        let model = ItemsModel(storageDirectory: try makeDirectory().path)
        await model.open()
        await model.add(title: "a")
        await model.add(title: "b")
        await model.delete(model.items[0])
        XCTAssertEqual(model.items.map(\.title), ["b"])
    }
}
