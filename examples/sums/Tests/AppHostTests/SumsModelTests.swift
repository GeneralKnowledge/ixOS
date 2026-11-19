import SumsApp
@preconcurrency import SumsFFI
import Foundation
import XCTest

/// Drives the view model and ForgeKit's host against the app's real Rust
/// core (rust/) through the generated Swift bindings. It imports no SwiftUI,
/// so `forge check swift` runs it on Linux; `forge test` runs it on a Mac.
final class SumsModelTests: XCTestCase {
    private func makeDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-sums-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    @MainActor
    private func open(_ dir: URL) async throws -> (ForgeHost<SumsCore>, SumsModel) {
        let environment = ForgeEnvironment(dataDirectory: dir.path, cachesDirectory: dir.path)
        let host = ForgeHost(environment: { environment }) { env in
            try SumsCore(storageDirectory: env.dataDirectory)
        }
        await host.start()
        XCTAssertEqual(host.status, .ready)
        return (host, SumsModel(core: try XCTUnwrap(host.core)))
    }

    @MainActor
    func testASumIsShownAndSurvivesARestart() async throws {
        let dir = try makeDirectory()
        do {
            let (host, model) = try await open(dir)
            for key in [SumsKey.one, .two, .add, .seven, .equals] {
                await model.press(key)
            }
            XCTAssertEqual(model.screen.display, "19")
            XCTAssertEqual(model.screen.expression, "12+7")
            host.close()
        }
        let (_, reopened) = try await open(dir)
        XCTAssertEqual(reopened.screen.display, "19")
    }

    func testEveryKeyHasOneFaceAndAnIdentifier() {
        let faces = SumsKeyFace.rows.flatMap { $0 }
        XCTAssertEqual(faces.count, 20)
        XCTAssertEqual(Set(faces.map(\.identifier)).count, 20)
        XCTAssertTrue(faces.allSatisfy { $0.identifier.hasPrefix("sums.key.") })
    }
}
