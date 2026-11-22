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

    /// The Settings field stops at the length Rust keeps, so a long name is
    /// never refused when leaving the screen (defect hunt finding 7).
    @MainActor
    func testTheNameFieldStopsWhereRustDoes() async throws {
        let (_, model) = try await open(try makeDirectory())
        let longest = String(repeating: "a", count: UrduModel.maxNameChars)
        XCTAssertTrue(UrduModel.fitsName(longest))
        XCTAssertFalse(UrduModel.fitsName(longest + "b"))
        // Spaces round the name don't count, as Rust trims them.
        XCTAssertTrue(UrduModel.fitsName(" " + longest + " "))
        await model.setName(" " + longest + " ")
        XCTAssertNil(model.errorMessage)
        XCTAssertEqual(model.settings.name, longest)
        // Every scalar counts: one emoji of five scalars, not one character.
        let family = "👨‍👩‍👧"
        XCTAssertFalse(UrduModel.fitsName(String(repeating: "a", count: UrduModel.maxNameChars - 2) + family))
        XCTAssertTrue(UrduModel.fitsName("Sam"))
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

    @MainActor
    func testAConversationBringsItsWords() async throws {
        let (_, model) = try await open(try makeDirectory())
        let tea = try XCTUnwrap(model.scenario("tea-at-khalas"))
        XCTAssertTrue(tea.words.contains { $0.id == "nahi-shukriya" })
        XCTAssertGreaterThanOrEqual(model.home.scenarios.count, 14)
    }

    @MainActor
    func testAFreeChatAnswersWhatYouSay() async throws {
        let (_, model) = try await open(try makeDirectory())
        XCTAssertEqual(model.characters().count, 4)
        let started = await model.chatStart("ali")
        let opened = try XCTUnwrap(started)
        // The opener is random: some are one line, some two.
        XCTAssertFalse(opened.lines.isEmpty)
        XCTAssertTrue(opened.lines.allSatisfy { $0.speaker == .character })
        let line = try XCTUnwrap(opened.suggestions.first)
        // No phone model on Linux: saying one of your lines still works.
        let said = await model.chatSay(line.roman.lowercased(), model: nil)
        let chat = try XCTUnwrap(said)
        let mine = try XCTUnwrap(chat.lines.first { $0.speaker == .you })
        XCTAssertEqual(mine.heard, .said)
        XCTAssertEqual(mine.phrase?.id, line.id)
        XCTAssertGreaterThan(model.phrase(line.id)?.skills.speaking ?? 0, 0)
        let unheard = await model.chatSay("qwzx", model: nil)
        let missed = try XCTUnwrap(unheard)
        XCTAssertEqual(missed.lines.last?.heard, .missed)

        // Too long: refused with the limit in the alert, never cut short.
        let long = String(repeating: "a", count: model.chatMaxTyped + 1)
        let refused = await model.chatSay(long, model: nil)
        XCTAssertNil(refused)
        XCTAssertEqual(model.errorMessage, "That's too long to say in one go. Keep it to \(model.chatMaxTyped) characters.")
    }
}
