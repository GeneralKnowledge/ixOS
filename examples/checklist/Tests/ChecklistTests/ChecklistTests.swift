// The application model across the language boundary: Swift (ForgeKit's
// host, the app's model) driving the real Rust core and runtime. These run
// on Linux and macOS with `swift test`; nothing is mocked.
import ChecklistApp
import ForgeFFI
import ForgeKit
import Foundation
import XCTest

@MainActor
final class ChecklistTests: XCTestCase {
    /// Waits for the host's event loop (a Swift task fed by Rust) to catch up.
    private func eventually(_ what: String, _ condition: () -> Bool) async {
        for _ in 0..<200 {
            if condition() { return }
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTFail("timed out waiting for \(what)")
    }

    private func host(_ environment: ForgeEnvironment) -> ForgeHost<ChecklistCore> {
        ForgeHost(environment: { environment }) { env in
            try ChecklistCore(dataDirectory: env.dataDirectory, cachesDirectory: env.cachesDirectory)
        }
    }

    func testContractVersionsAgree() {
        XCTAssertEqual(forgeAppContractVersion(), forgeKitContractVersion)
    }

    func testTheHostOwnsTheCoreAndHearsEveryCommit() async throws {
        let host = Checklist.makeTemporaryHost()
        await host.start()
        XCTAssertEqual(host.status, .ready)
        let core = try XCTUnwrap(host.core)
        let model = ChecklistModel(core: core)
        let before = host.changes

        let added = await model.add("  Buy   milk ")
        XCTAssertTrue(added)
        XCTAssertEqual(model.items.map(\.title), ["Buy milk"])
        await eventually("Changed from Rust") { host.revision == 1 && host.changes > before }

        // A refused command: nothing committed, nothing announced, and the
        // person gets the app's words for it.
        let refused = await model.add("   ")
        XCTAssertFalse(refused)
        XCTAssertEqual(model.errorMessage, "Type something to add.")
        XCTAssertEqual(core.revision(), 1)
        host.close()
    }

    func testOneCorePerDataDirectory() async throws {
        let environment = try ForgeEnvironment.temporary()
        let first = host(environment)
        await first.start()
        XCTAssertEqual(first.status, .ready)

        let second = host(environment)
        await second.start()
        guard case .failed = second.status else {
            return XCTFail("a second core opened the same checklist: \(second.status)")
        }

        // Once the first is closed and released, the data can be opened again.
        first.close()
        XCTAssertEqual(first.status, .closed)
        XCTAssertNil(first.core)
        await second.start()
        XCTAssertEqual(second.status, .ready)
        second.close()
    }

    func testTheBackgroundInterruptsOnScreenWorkOnly() async throws {
        let host = Checklist.makeTemporaryHost()
        await host.start()
        let core = try XCTUnwrap(host.core)
        let model = ChecklistModel(core: core)
        await model.add("Buy oat milk")
        await model.add("Call the vet")
        host.report(.active)
        let found = await model.search("milk")
        XCTAssertEqual(found?.map(\.title), ["Buy oat milk"])

        host.report(.background)
        do {
            _ = try await core.search(query: "milk", cancellation: nil)
            XCTFail("search ran in the background")
        } catch ChecklistError.Stopped(let kind) {
            XCTAssertEqual(kind, .interrupted)
            XCTAssertEqual(Checklist.message(for: ChecklistError.Stopped(kind: kind)), ForgeErrors.describe(.interrupted))
        }
        // Export may finish in the background.
        let url = await model.exportText()
        XCTAssertEqual(try String(contentsOf: try XCTUnwrap(url), encoding: .utf8), "[ ] Buy oat milk\n[ ] Call the vet\n")

        // Back on screen: work runs again, and the host tells views to re-read.
        let changes = host.changes
        host.report(.active)
        await eventually("resume announced") { host.changes > changes }
        let again = await model.search("vet")
        XCTAssertEqual(again?.count, 1)
        host.close()
    }

    func testCancellingTheSwiftTaskCancelsTheRustWork() async throws {
        let host = Checklist.makeTemporaryHost()
        await host.start()
        let core = try XCTUnwrap(host.core)
        _ = try await core.add(title: "a")
        host.report(.active)

        // Cancelled before it runs: the Cancellation reaches Rust at once.
        let task = Task { @MainActor in
            try await withRustCancellation { cancellation in
                try await core.search(query: "a", cancellation: cancellation)
            }
        }
        task.cancel()
        do {
            _ = try await task.value
            XCTFail("cancelled search returned results")
        } catch ChecklistError.Stopped(let kind) {
            XCTAssertEqual(kind, .cancelled)
        }
        host.close()
    }

    func testShutdownKeepsCommittedWorkAndRefusesMore() async throws {
        let environment = try ForgeEnvironment.temporary()
        do {
            let first = host(environment)
            await first.start()
            let core = try XCTUnwrap(first.core)
            _ = try await core.add(title: "Keep me")
            first.report(.terminated)
            XCTAssertNil(first.core)
            do {
                _ = try await core.add(title: "Too late")
                XCTFail("a command ran after shutdown")
            } catch ChecklistError.Stopped(let kind) {
                XCTAssertEqual(kind, .shutDown)
            }
        }
        // The next launch finds what was committed before shutdown.
        let next = host(environment)
        await next.start()
        XCTAssertEqual(next.status, .ready)
        XCTAssertEqual(next.core?.items().map(\.title), ["Keep me"])
        // A fresh core starts at revision 0; the host takes Rust's value.
        XCTAssertEqual(next.revision, next.core?.revision())
        next.close()
    }
}
