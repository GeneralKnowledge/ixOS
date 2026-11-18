import UrduNaturallyApp
@preconcurrency import UrduNaturallyFFI
import Foundation
import XCTest

/// Drives the view model and ForgeKit's host against the app's real Rust
/// core (rust/) through the generated Swift bindings. It imports no SwiftUI,
/// so `forge check swift` runs it on Linux; `forge test` runs it on a Mac.
final class UrduModelTests: XCTestCase {
    private func makeDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-urdu-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    @MainActor
    private func open(_ dir: URL) async throws -> (ForgeHost<UrduCore>, UrduModel) {
        let environment = ForgeEnvironment(dataDirectory: dir.path, cachesDirectory: dir.path)
        let host = ForgeHost(environment: { environment }) { env in
            try UrduCore(storageDirectory: env.dataDirectory)
        }
        await host.start()
        XCTAssertEqual(host.status, .ready)
        return (host, UrduModel(core: try XCTUnwrap(host.core)))
    }

    @MainActor
    func testABeginnerStartsWithTheFirstConversation() async throws {
        let (_, model) = try await open(try makeDirectory())
        XCTAssertEqual(model.home.suggestion, .conversation(scenarioId: "meeting-someone-new"))
        XCTAssertEqual(model.home.topics.first?.level, .foundation)
        let scenario = try XCTUnwrap(model.scenario("catching-up"))
        XCTAssertEqual(scenario.summary.character.name, "Ali")
        XCTAssertTrue(scenario.turns.contains { $0.speaker == .you })
    }

    @MainActor
    func testProgressAndNameSurviveARestart() async throws {
        let dir = try makeDirectory()
        do {
            let (host, model) = try await open(dir)
            await model.setName("Sam")
            let phrase = await model.record("main-theek-hoon", skill: .speaking, outcome: .independent)
            XCTAssertEqual(phrase?.skills.speaking, 1)
            XCTAssertEqual(model.phrase("mera-naam-hai")?.roman, "Mera naam Sam hai.")
            host.close()
        }
        let (_, reopened) = try await open(dir)
        XCTAssertEqual(reopened.settings.name, "Sam")
        XCTAssertEqual(reopened.phrase("main-theek-hoon")?.skills.speaking, 1)
        XCTAssertEqual(reopened.home.totals.attemptsToday, 1)
    }

    @MainActor
    func testAnUnknownPhraseIsAnAlertNotACrash() async throws {
        let (_, model) = try await open(try makeDirectory())
        let phrase = await model.record("nope", skill: .recall, outcome: .confident)
        XCTAssertNil(phrase)
        XCTAssertEqual(model.errorMessage, "That phrase isn't in this version of the app.")
    }

    @MainActor
    func testTypedAnswersForgiveSpelling() async throws {
        let (_, model) = try await open(try makeDirectory())
        XCTAssertEqual(model.check("mujhe-nahi-pata", typed: "mujhe nahin pata"), .match)
        XCTAssertEqual(model.check("mujhe-nahi-pata", typed: "kya haal hai"), .different)
    }

    @MainActor
    func testTypedAnswersDontMergeWordsThatSoundAlike() async throws {
        let (_, model) = try await open(try makeDirectory())
        // kam (less) is not kaam (work): close, so you see the answer.
        XCTAssertEqual(model.check(sentence: "Main kaam kar raha hoon.", typed: "main kam kar raha hoon"), .close)
        XCTAssertEqual(model.check(sentence: "Main kaam kar raha hoon.", typed: "main kaam kar raha hun"), .match)
    }

    @MainActor
    func testAPatternIsPractisedWithItsExamples() async throws {
        let (_, model) = try await open(try makeDirectory())
        let pattern = try XCTUnwrap(model.phrase("pattern-can"))
        XCTAssertEqual(pattern.kind, .pattern)
        XCTAssertGreaterThanOrEqual(pattern.combos.count, 2)
        XCTAssertTrue(pattern.combos.allSatisfy { $0.speech != nil })
        XCTAssertGreaterThan(model.home.totals.patterns, 0)
    }
}
