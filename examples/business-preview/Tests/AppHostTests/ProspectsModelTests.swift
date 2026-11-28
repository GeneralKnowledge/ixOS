import BusinessPreviewApp
@preconcurrency import BusinessPreviewFFI
import ForgeAppKit
import Foundation
import XCTest

/// Drives the view models and ForgeKit's host against the app's real Rust
/// core (rust/) through the generated Swift bindings. It imports no SwiftUI,
/// so `forge check swift` runs it on Linux; `forge test` runs it on a Mac.
final class ProspectsModelTests: XCTestCase {
    /// Writes the pack Rust's own tests use (rust/tests/fixtures): a made-up
    /// riding school with a logo, one verified price and one unverified.
    private func writePack(in dir: URL) throws -> URL {
        let pack = dir.appendingPathComponent("pack", isDirectory: true)
        let images = pack.appendingPathComponent("images", isDirectory: true)
        try FileManager.default.createDirectory(at: images, withIntermediateDirectories: true)
        try Data(Self.profile.utf8).write(to: pack.appendingPathComponent("profile.json"))
        try XCTUnwrap(Data(base64Encoded: Self.logo)).write(to: images.appendingPathComponent("logo.png"))
        return pack
    }

    /// A 2 by 1 pixel PNG.
    private static let logo = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAIAAAB7QOjdAAAADUlEQVR4nGM4uiwLiAANkAOrn+CqoAAAAABJRU5ErkJggg=="

    private static let profile = """
        {
          "schema": "business-profile/0",
          "id": "riding-school-example",
          "category": "riding_school",
          "name": { "value": "The Riding School", "source": "s1", "method": "jsonld", "verified": true },
          "tagline": { "value": "Your next adventure starts here", "source": "s1", "method": "html", "verified": true },
          "branding": {
            "primary": { "value": "#171717", "source": "s2", "method": "css", "verified": true },
            "accent": { "value": "#C5A66A", "source": "s2", "method": "css", "verified": true },
            "logo": { "value": "images/logo.png", "source": "s1", "method": "html", "verified": true }
          },
          "services": [
            {
              "name": { "value": "Private lesson (30 min)", "source": "s3", "method": "html", "verified": true },
              "price": { "value": { "amount": "45.00", "currency": "GBP" }, "source": "s3", "method": "llm", "confidence": 0.8, "verified": false },
              "duration_minutes": { "value": 30, "source": "s3", "method": "llm", "confidence": 0.9, "verified": true }
            },
            {
              "name": { "value": "Group lesson", "source": "s3", "method": "html", "verified": true },
              "price": { "value": { "amount": "35.00", "currency": "GBP" }, "source": "s3", "method": "html", "verified": true }
            }
          ],
          "contact": {
            "phone": { "value": "01506 000000", "source": "s1", "method": "jsonld", "verified": true },
            "email": { "value": "hello@example.com", "source": "s1", "method": "html", "verified": true },
            "address": { "value": "Example Farm, Livingston, EH54 0AA", "source": "s1", "method": "jsonld", "verified": true },
            "social": [
              { "value": "https://www.facebook.com/example", "source": "s1", "method": "html", "verified": true }
            ]
          },
          "hours": [
            { "value": { "day": "saturday", "opens": "09:00", "closes": "17:00" }, "source": "s1", "method": "jsonld", "verified": true }
          ],
          "actions": {
            "book": { "kind": "link", "value": "https://booking.example/riding-school", "source": "s4", "method": "html", "verified": true }
          },
          "sources": {
            "s1": { "url": "https://example.com/", "fetched": "2026-11-28T10:00:00Z" },
            "s2": { "url": "https://example.com/assets/site.css", "fetched": "2026-11-28T10:00:02Z" },
            "s3": { "url": "https://example.com/lessons", "fetched": "2026-11-28T10:00:05Z" },
            "s4": { "url": "https://example.com/book", "fetched": "2026-11-28T10:00:08Z" }
          },
          "preview": { "status": "concept", "consent": "none" }
        }
        """

    private func makeDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-business-preview-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    @MainActor
    private func open(_ dir: URL) async throws -> (ForgeHost<BusinessPreviewCore>, ProspectsModel) {
        let environment = ForgeEnvironment(dataDirectory: dir.path, cachesDirectory: dir.path)
        let host = ForgeHost(environment: { environment }) { env in
            try BusinessPreviewCore(storageDirectory: env.dataDirectory)
        }
        await host.start()
        XCTAssertEqual(host.status, .ready)
        return (host, ProspectsModel(core: try XCTUnwrap(host.core)))
    }

    @MainActor
    func testAnImportedPackIsListedShownAndDeleted() async throws {
        let dir = try makeDirectory()
        let (_, model) = try await open(dir)
        XCTAssertTrue(model.prospects.isEmpty)

        _ = try await model.core.importPack(path: try writePack(in: dir).path)
        model.reload()
        XCTAssertEqual(model.prospects.map(\.name), ["The Riding School"])
        XCTAssertEqual(model.prospects.first?.isConcept, true)

        let preview = PreviewModel(core: model.core, id: "riding-school-example")
        await preview.load()
        guard case .ready(let business) = preview.state else {
            return XCTFail("the preview didn't load: \(preview.state)")
        }
        // Every screen carries the concept label, and the unverified price stays hidden.
        XCTAssertEqual(business.conceptLabel, PreviewModel.conceptLabel)
        XCTAssertNil(business.services[0].price)
        XCTAssertEqual(business.services[1].price, "£35.00")
        XCTAssertEqual(business.book?.kind, .page)
        XCTAssertNotNil(business.brand)

        model.askToDelete(model.prospects[0])
        let delete = try XCTUnwrap(model.alert?.action(for: "Delete"))
        model.handle(delete)
        for _ in 0..<100 where !model.prospects.isEmpty {
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTAssertTrue(model.prospects.isEmpty)
    }

    @MainActor
    func testARefusedPackSaysWhy() async throws {
        let dir = try makeDirectory()
        let (_, model) = try await open(dir)
        let junk = dir.appendingPathComponent("notes.txt")
        try Data("hello".utf8).write(to: junk)
        do {
            _ = try await model.core.importPack(path: junk.path)
            XCTFail("a text file was taken as a pack")
        } catch let error as BusinessPreviewError {
            XCTAssertEqual(ForgeFailure(error).title, "This Profile Can't Be Shown")
        }
        let refused = await model.download(address: "http://example.com/pack.zip")
        XCTAssertEqual(refused?.title, "Can't Use That Address")
        XCTAssertTrue(model.prospects.isEmpty)
    }
}
