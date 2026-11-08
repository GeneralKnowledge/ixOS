@preconcurrency import ForgeFFI
import Foundation

// Identifiable conformances and the owner-facing names of Rust's enums.
// Wording is presentation, so it lives in Swift; rules stay in Rust.

extension PlanActivity: Identifiable {}
extension SessionRecord: Identifiable {}
extension ExerciseSummary: Identifiable {}
extension TermSummary: Identifiable {}
extension LearnTopic: Identifiable {}
extension SkillProgress: Identifiable {
    public var id: String { objectiveId }
}
extension GlossarySection: Identifiable {
    public var id: String { title }
}

extension DogLifeStage: CaseIterable, Identifiable {
    public static var allCases: [DogLifeStage] { [.youngPuppy, .olderPuppy, .adolescent, .adult, .senior] }
    public var id: Self { self }
    public var title: String {
        switch self {
        case .youngPuppy: "Young puppy (under 4 months)"
        case .olderPuppy: "Older puppy (4 to 6 months)"
        case .adolescent: "Adolescent (6 months to 2 years)"
        case .adult: "Adult"
        case .senior: "Senior"
        }
    }
}

extension DailyTime: CaseIterable, Identifiable {
    public static var allCases: [DailyTime] { [.fewMinutes, .aboutTenMinutes, .moreThanTen] }
    public var id: Self { self }
    public var title: String {
        switch self {
        case .fewMinutes: "A few minutes"
        case .aboutTenMinutes: "About 10 minutes"
        case .moreThanTen: "More than 10 minutes"
        }
    }
}

extension TrainingGoal: CaseIterable, Identifiable {
    public static var allCases: [TrainingGoal] { [.general, .recall, .looseLead, .settle, .manners, .puppyBasics] }
    public var id: Self { self }
    public var title: String {
        switch self {
        case .general: "A bit of everything"
        case .recall: "Coming back when called"
        case .looseLead: "Walking nicely on the lead"
        case .settle: "Settling at home"
        case .manners: "Everyday manners"
        case .puppyBasics: "Puppy basics"
        }
    }
}

extension OwnerExperience: CaseIterable, Identifiable {
    public static var allCases: [OwnerExperience] { [.new, .some] }
    public var id: Self { self }
    public var title: String {
        switch self {
        case .new: "New to training"
        case .some: "Done some training"
        }
    }
}

extension SessionResult: CaseIterable, Identifiable {
    public static var allCases: [SessionResult] { [.easy, .gettingThere, .tooDifficult] }
    public var id: Self { self }
    public var title: String {
        switch self {
        case .easy: "Went well"
        case .gettingThere: "Getting there"
        case .tooDifficult: "Too difficult"
        }
    }
    public var systemImage: String {
        switch self {
        case .easy: "hand.thumbsup"
        case .gettingThere: "arrow.triangle.2.circlepath"
        case .tooDifficult: "tortoise"
        }
    }
    /// For accessibility identifiers.
    public var key: String {
        switch self {
        case .easy: "easy"
        case .gettingThere: "gettingThere"
        case .tooDifficult: "tooDifficult"
        }
    }
}

extension PlanRole {
    public var title: String {
        switch self {
        case .engage: "Warm-up"
        case .skill: "Main skill"
        case .everyday: "Everyday"
        }
    }
}

extension SkillLevel {
    public var title: String {
        switch self {
        case .notStarted: "Not started"
        case .started: "Started"
        case .practising: "Practising"
        case .becomingConsistent: "Becoming consistent"
        case .readyForMore: "Ready for more"
        case .makingEasier: "Keeping it easier"
        }
    }
}

extension LinkedText {
    /// The text without links.
    public var plain: String { pieces.map(\.text).joined() }
}

extension SessionRecord {
    public var completedAt: Date { Date(timeIntervalSince1970: TimeInterval(completedAtMs) / 1000) }
}
