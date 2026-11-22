@preconcurrency import UrduNaturallyFFI
import Foundation
import Observation

/// What every screen reads. It holds no rules of its own: the curriculum,
/// the four skills, the review schedule and the answer check are all in Rust
/// (rust/src), and this presents what Rust returns.
///
/// It imports no SwiftUI, so `forge check swift` compiles and tests it on
/// Linux.
@Observable
@MainActor
public final class UrduModel {
    public let core: UrduCore
    public private(set) var home: Home
    public private(set) var settings: Settings
    /// A failed save, for an alert. The view clears it when dismissed.
    public var errorMessage: String?

    public init(core: UrduCore) {
        self.core = core
        home = core.home(dayStartMs: Self.dayStartMs())
        settings = core.settings()
    }

    /// Re-reads from Rust. Cheap: it never waits for a command.
    public func reload() {
        home = core.home(dayStartMs: Self.dayStartMs())
        settings = core.settings()
    }

    /// When today began on this phone, for "today" counts.
    public static func dayStartMs(now: Date = Date(), calendar: Calendar = .current) -> Int64 {
        Int64(calendar.startOfDay(for: now).timeIntervalSince1970 * 1000)
    }

    // MARK: Reads

    public func scenario(_ id: String) -> ScenarioView? { core.scenario(id: id) }
    public func phrase(_ id: String) -> PhraseView? { core.phrase(id: id) }
    public func phrases(topic: String?, query: String = "") -> [PhraseView] {
        core.phrases(topic: topic, query: query)
    }
    public func reviewQueue(limit: UInt32 = 20) -> [ReviewItem] { core.review(limit: limit) }
    public func listeningRound(count: UInt32 = 8) -> [ListeningItem] {
        core.listening(count: count, seed: UInt64.random(in: 0...UInt64.max))
    }
    public func check(_ phraseId: String, typed: String) -> Verdict {
        core.checkAnswer(phraseId: phraseId, typed: typed)
    }
    /// Checks a typed answer against one sentence (a pattern's example).
    public func check(sentence: String, typed: String) -> Verdict {
        core.checkSentence(sentence: sentence, typed: typed)
    }
    public func scenarioSummary(_ id: String) -> ScenarioSummary? {
        home.scenarios.first { $0.id == id }
    }
    public func topic(_ id: String) -> TopicView? {
        home.topics.first { $0.id == id }
    }

    // MARK: Commands

    /// Records an attempt; returns the phrase as it is now, or nil (and an
    /// alert) if it couldn't be saved.
    @discardableResult
    public func record(
        _ phraseId: String, skill: Skill, outcome: Outcome, latencyMs: UInt32? = nil
    ) async -> PhraseView? {
        do {
            let phrase = try await core.record(
                phraseId: phraseId, skill: skill, outcome: outcome, latencyMs: latencyMs)
            reload()
            return phrase
        } catch {
            errorMessage = Self.describe(error)
            return nil
        }
    }

    /// Records that phrases were shown (a card opened, a conversation line).
    public func markSeen(_ ids: [String]) async {
        guard !ids.isEmpty else { return }
        await run { try await self.core.seen(phraseIds: ids) }
    }

    // MARK: Free chat

    /// The people you can chat with.
    public func characters() -> [CharacterView] { core.characters() }

    /// Starts a new chat with a character; nil (and an alert) if it
    /// couldn't start.
    public func chatStart(_ characterId: String) async -> ChatView? {
        await chatRun {
            try await self.core.chatStart(characterId: characterId, seed: UInt64.random(in: 0...UInt64.max))
        }
    }

    /// Says something in the chat. `model` is the phone's model, which may
    /// work out which of your lines you meant; Rust decides everything else.
    public func chatSay(_ text: String, model: AiModel?) async -> ChatView? {
        await chatRun { try await self.core.chatSay(text: text, model: model) }
    }

    /// Picks a line from the help.
    public func chatPick(_ phraseId: String) async -> ChatView? {
        await chatRun { try await self.core.chatPick(phraseId: phraseId) }
    }

    /// Ends the chat; what you typed in it is forgotten.
    public func chatEnd() { core.chatEnd() }

    private func chatRun(_ work: () async throws -> ChatView) async -> ChatView? {
        do {
            let view = try await work()
            reload()
            return view
        } catch {
            errorMessage = Self.describe(error)
            return nil
        }
    }

    public func setName(_ name: String) async {
        await run { try await self.core.setName(name: name) }
    }

    public func setEnglish(_ english: EnglishSupport) async {
        await run { try await self.core.setEnglish(english: english) }
    }

    public func startOver() async {
        await run { try await self.core.startOver() }
    }

    private func run(_ work: () async throws -> Void) async {
        do {
            try await work()
            reload()
        } catch {
            errorMessage = Self.describe(error)
        }
    }

    /// Text for the alert. `reason` fields are diagnostics for the log,
    /// never the screen.
    public static func describe(_ error: Error) -> String {
        guard let error = error as? UrduError else { return error.localizedDescription }
        switch error {
        case .NotFound: return "That phrase isn't in this version of the app."
        case let .NameTooLong(max): return "Names can be up to \(max) characters."
        case .InUse: return "Urdu, Naturally is open in another copy of the app."
        case .Corrupt: return "Your saved progress couldn't be read."
        case .NewerData: return "Your progress was saved by a newer version of the app. Update it to carry on."
        case .Storage: return "Your progress couldn't be saved. Try again."
        case .Stopped: return "The app is closing."
        }
    }
}

// MARK: Words for the screen

extension Outcome {
    /// The buttons after an attempt, in order.
    public static let all: [Outcome] = [.missed, .struggled, .withHelp, .independent, .confident]

    public var label: String {
        switch self {
        case .missed: "Didn't get it"
        case .struggled: "Struggled"
        case .withHelp: "With a hint"
        case .independent: "Got it"
        case .confident: "Easy"
        }
    }

    public var identifier: String {
        switch self {
        case .missed: "missed"
        case .struggled: "struggled"
        case .withHelp: "withHelp"
        case .independent: "independent"
        case .confident: "confident"
        }
    }
}

extension Skill {
    public var label: String {
        switch self {
        case .recognition: "Recognise"
        case .recall: "Recall"
        case .speaking: "Use"
        case .listening: "Hear"
        }
    }
}

extension EntryKind {
    public var label: String {
        switch self {
        case .phrase: "Phrase"
        case .word: "Word"
        case .expression: "Expression"
        case .pattern: "Pattern"
        }
    }
}

extension Stage {
    public var label: String {
        switch self {
        case .new: "New"
        case .learning: "Learning"
        case .recognised: "Recognised"
        case .canRecall: "Can recall"
        case .canUse: "Can use"
        }
    }
}

extension Formality {
    public var label: String {
        switch self {
        case .informal: "Informal"
        case .neutral: "Neutral"
        case .polite: "Polite"
        case .formal: "Formal"
        case .contextDependent: "Depends on context"
        }
    }
}

extension VariantKind {
    public var label: String {
        switch self {
        case .femaleSpeaker: "A woman says"
        case .femaleAddressee: "To a woman"
        case .polite: "Polite"
        case .formal: "Formal"
        case .informal: "Informal"
        case .alternative: "Also"
        case .spelling: "Also spelt"
        case .feminine: "Feminine"
        case .plural: "Plural"
        }
    }
}

extension ContentStatus {
    public var label: String {
        switch self {
        case .draft: "Draft: not yet checked by a native speaker"
        case .reviewed: "Reviewed by a native speaker"
        case .fieldTested: "Field-tested in real conversation"
        }
    }
}

extension EnglishSupport {
    public static let all: [EnglishSupport] = [.gradual, .always, .minimal]

    public var label: String {
        switch self {
        case .gradual: "Fade as I learn"
        case .always: "Always show"
        case .minimal: "Keep to a minimum"
        }
    }
}
