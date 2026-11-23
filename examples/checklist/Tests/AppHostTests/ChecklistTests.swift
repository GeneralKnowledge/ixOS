// The application model across the language boundary: Swift (ForgeKit's
// host, the app's model) driving the real Rust core and runtime. These run
// on Linux and macOS with `swift test`; nothing is mocked.
import ChecklistApp
import ChecklistFFI
@preconcurrency import ForgeKit  // ForgeKit builds in Swift 5 mode; `forge check swift` builds tests in 6
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
        // The model's search stops quietly: a stop is not an error to show.
        let stopped = await model.search("milk")
        XCTAssertNil(stopped)
        XCTAssertNil(model.errorMessage)
        XCTAssertTrue(ChecklistModel.isStop(CancellationError()))
        XCTAssertFalse(ChecklistModel.isStop(ChecklistError.InUse))
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

    func testFinishingWorkHoldsBackgroundTimeUntilItIsDone() async throws {
        let host = Checklist.makeTemporaryHost()
        let time = FakeBackgroundTime()
        host.backgroundTime = time
        await host.start()
        XCTAssertEqual(host.launch, LaunchReport(unfinishedBackgroundWork: false))
        let core = try XCTUnwrap(host.core)
        let model = ChecklistModel(core: core)
        await model.add("Buy oat milk")
        host.report(.active)
        host.report(.background)
        XCTAssertEqual(time.log, [], "nothing was running, so no time was asked for")

        // An export started in the background is finishing work: the host
        // holds background time while it runs, and gives it back after.
        let url = await model.exportText()
        XCTAssertNotNil(url)
        await eventually("background time held, then given back") { time.log == ["begin", "end"] }

        host.report(.active)
        host.close()
    }

    /// CL-4: Swift turns the phone's local day into Rust's `YYYY-MM-DD`,
    /// and the model's agenda follows it.
    func testDueDatesFillTheAgendaOnTheLocalDay() async throws {
        let host = Checklist.makeTemporaryHost()
        await host.start()
        let core = try XCTUnwrap(host.core)
        let model = ChecklistModel(core: core)
        await model.add("Post the parcel")
        await model.add("Call the vet")
        await model.add("Someday")
        let calendar = Calendar(identifier: .gregorian)
        let now = Date()
        let yesterday = try XCTUnwrap(calendar.date(byAdding: .day, value: -1, to: now))
        let nextWeek = try XCTUnwrap(calendar.date(byAdding: .day, value: 7, to: now))

        try await model.setDue(model.items[0], to: now)
        try await model.setDue(model.items[1], to: yesterday)
        try await model.setDue(model.items[2], to: nextWeek)
        XCTAssertEqual(model.items[0].due, ChecklistDay.string(from: now))
        XCTAssertEqual(model.agenda.today.map(\.title), ["Post the parcel"])
        XCTAssertEqual(model.agenda.overdue.map(\.title), ["Call the vet"])
        XCTAssertEqual(model.agenda.upcoming.map(\.title), ["Someday"])

        // A saved day reads back as the start of that local day.
        let start = try XCTUnwrap(ChecklistDay.date(from: model.today))
        XCTAssertEqual(ChecklistDay.string(from: start), model.today)
        XCTAssertNil(ChecklistDay.date(from: "next week"))

        // Done, or with its day removed, an item leaves the agenda.
        await model.toggle(model.items[0])
        try await model.setDue(model.items[1], to: nil)
        XCTAssertNil(model.items[1].due)
        XCTAssertTrue(model.agenda.today.isEmpty)
        XCTAssertTrue(model.agenda.overdue.isEmpty)
        host.close()
    }
}

/// Records what the host asks of the system's background time.
@MainActor
private final class FakeBackgroundTime: ForgeBackgroundTime {
    var log: [String] = []

    func begin(onExpiry: @escaping @MainActor () -> Void) {
        log.append("begin")
    }

    func end() {
        log.append("end")
    }
}
