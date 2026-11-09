import AppHost
@preconcurrency import ForgeFFI
import Foundation
import XCTest

/// Drives the view model and ForgeKit's host against the app's real Rust
/// core (rust/) through the generated Swift bindings. It imports no SwiftUI,
/// so `forge check swift` runs it on Linux; `forge test` runs it on a Mac.
final class ItemsModelTests: XCTestCase {
    private func makeDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-items-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    /// A host whose data lives in `dir`, as the app's own would in
    /// Application Support.
    @MainActor
    private func open(_ dir: URL) async throws -> (ForgeHost<AppCore>, ItemsModel) {
        let environment = ForgeEnvironment(dataDirectory: dir.path, cachesDirectory: dir.path)
        let host = ForgeHost(environment: { environment }) { env in
            try AppCore(storageDirectory: env.dataDirectory)
        }
        await host.start()
        XCTAssertEqual(host.status, .ready)
        return (host, ItemsModel(core: try XCTUnwrap(host.core)))
    }

    @MainActor
    func testItemsAreSavedAndSurviveARestart() async throws {
        let dir = try makeDirectory()
        var saved: [Item] = []
        do {
            let (host, model) = try await open(dir)
            XCTAssertEqual(model.items, [])
            await model.add(title: "Milk")
            await model.add(title: "Bread")
            XCTAssertEqual(model.items.map(\.title), ["Milk", "Bread"])
            await model.toggle(model.items[0])
            XCTAssertTrue(model.items[0].done)
            saved = model.items
            host.close()
        }
        // The first core is released with its host and model, so the next
        // launch can open the same folder.
        let (host, reopened) = try await open(dir)
        XCTAssertEqual(reopened.items, saved)
        host.close()
    }

    @MainActor
    func testABlankTitleShowsAnError() async throws {
        let (host, model) = try await open(try makeDirectory())
        await model.add(title: "   ")
        XCTAssertEqual(model.items, [])
        XCTAssertNotNil(model.errorMessage)
        host.close()
    }

    @MainActor
    func testDeleteRemovesTheItem() async throws {
        let (host, model) = try await open(try makeDirectory())
        await model.add(title: "a")
        await model.add(title: "b")
        await model.delete(model.items[0])
        XCTAssertEqual(model.items.map(\.title), ["b"])
        // Two adds and a delete: three committed changes.
        XCTAssertEqual(model.core.revision(), 3)
        host.close()
    }
}
