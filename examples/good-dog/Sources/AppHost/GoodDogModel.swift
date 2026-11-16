@preconcurrency import GoodDogFFI
import ForgeAppKit
import ForgeKit
import Foundation
import Observation

private let log = ForgeLog("GoodDog")

// The Rust core runs on forge-app's runtime (`export_runtime!`), so ForgeKit
// hosts it: one core per process, the lifecycle, background time, shutdown.
extension AppCore: ForgeCore {}

extension AppError: ForgeKindedError {
    public var forgeKind: ErrorKind { appErrorKind(error: self) }
}

extension AppError: ForgeDescribedError {
    /// Good Dog's own words for its errors (`forgeAlert` shows them).
    public var failure: ForgeFailure {
        ForgeFailure(
            title: GoodDogModel.title(self), message: GoodDogModel.describe(self),
            isRetryable: ForgeErrors.isRetryable(forgeKind))
    }
}

/// The host for Good Dog's core, with its data where it has always been
/// (Application Support/GoodDog), or a throwaway folder on test data. The
/// App and Device Lab each make one for the life of the process and attach
/// it to the lifecycle (`ForgeLifecycle.attach`).
@MainActor
public func makeAppHost(storageDirectory: String? = nil) -> ForgeHost<AppCore> {
    ForgeHost(
        environment: {
            let directory = try storageDirectory ?? GoodDogModel.defaultDirectory()
            return ForgeEnvironment(dataDirectory: directory, cachesDirectory: directory)
        },
        open: { try AppCore(storageDirectory: $0.dataDirectory) }
    )
}

/// The app's presentation state. It holds no training rules: every change
/// goes to the Rust `AppCore` (rust/src/lib.rs), which builds plans, records
/// sessions and moves skills on. ForgeKit's host opens the core; this model
/// keeps the latest answers for the screens and re-reads them when the
/// host's `changes` moves (the root view calls `hostChanged()`).
@Observable
@MainActor
public final class GoodDogModel {
    public enum Phase: Equatable {
        case loading
        /// No dog yet: show onboarding.
        case needsDog
        case ready
        case failed(String)
    }

    public private(set) var phase: Phase = .loading
    public private(set) var dog: DogProfile?
    /// Today's plan, as Rust last reported it.
    public private(set) var today: TodayPlan?
    public private(set) var progress: ProgressSummary?
    public private(set) var history: [SessionRecord] = []
    public private(set) var topics: [LearnTopic] = []
    public private(set) var glossary: [GlossarySection] = []
    /// The Learn search field.
    public var searchText = ""
    /// A failed action, for an alert (`forgeAlert(failure:)`). The view
    /// clears it when dismissed.
    public var actionError: ForgeFailure?
    /// Why today's plan couldn't be built. The Today tab shows it with Try
    /// Again; the other tabs keep working.
    public private(set) var todayError: String?
    /// The daily reminder's time in minutes after midnight; nil when off.
    public private(set) var reminderMinute: UInt32?
    /// Increases after each recorded session; views use it as a haptic trigger.
    public private(set) var sessionCount = 0
    /// The owner's goals: active first, then paused, finished, set aside.
    public private(set) var goals: [GoalSummary] = []
    /// The goals that suit the dog, to choose from.
    public private(set) var pathways: [PathwaySummary] = []
    /// Increases after every reload, so a goal screen knows to read again.
    public private(set) var revision = 0
    /// A stage the owner just passed, for the celebration. Views clear it.
    public var celebration: StageCelebration?

    /// Opens the core, reports the lifecycle and shuts the core down.
    public let host: ForgeHost<AppCore>
    public var core: AppCore? { host.core }
    private let clock: @Sendable () -> LocalTime
    /// Set up before the first read when the store has no dog yet.
    private let sampleDog: DogProfile?

    /// - Parameters:
    ///   - host: the process's host (`makeAppHost()`).
    ///   - clock: the time now with the phone's UTC offset; tests fix it.
    public init(
        host: ForgeHost<AppCore>,
        clock: (@Sendable () -> LocalTime)? = nil,
        sampleDog: DogProfile? = GoodDogModel.launchSampleDog
    ) {
        self.host = host
        self.sampleDog = sampleDog
        if let clock {
            self.clock = clock
        } else if Self.usesTestData {
            self.clock = { LocalTime.uiTest }
        } else {
            self.clock = { LocalTime.now() }
        }
    }

    /// A model with a host of its own on `storageDirectory` (tests).
    public convenience init(
        storageDirectory: String,
        clock: (@Sendable () -> LocalTime)? = nil,
        sampleDog: DogProfile? = nil
    ) {
        self.init(host: makeAppHost(storageDirectory: storageDirectory), clock: clock, sampleDog: sampleDog)
    }

    /// Whether this launch uses throwaway test data (`-ForgeUITestData`, as
    /// `forge ui run` passes it). Such a launch also
    /// runs on a fixed clock, so dates and times on screen don't change
    /// between runs and screenshots stay comparable.
    /// ForgeKit's `-ForgeTemporaryData` does the same, so one argument
    /// starts every app on test data (Device Lab's Apps screen).
    nonisolated static let usesTestData = ProcessInfo.processInfo.arguments.contains("-ForgeUITestData")
        || ProcessInfo.processInfo.arguments.contains("-ForgeTemporaryData")

    /// With `-ForgeUITestData`, `-ForgeUITestSampleDog` starts on a dog
    /// that is already set up, so UI tests and walks can reach Today and My
    /// Dog without filling in onboarding (journey.toml covers onboarding).
    /// Never used on the owner's own data.
    public static let sampleDogArgument = "-ForgeUITestSampleDog"
    public static let launchSampleDog: DogProfile? =
        usesTestData && ProcessInfo.processInfo.arguments.contains(sampleDogArgument)
        ? DogProfile(name: "Moss", lifeStage: .adult, dailyTime: .aboutTenMinutes, goal: .recall, experience: .new)
        : nil

    public var maxNameLength: Int { Int(core?.maxNameLength() ?? 40) }

    /// Rust's matches for `searchText`, or nil when the field is empty.
    public var searchResults: LearnSearchResults? {
        guard let core, !searchText.trimmingCharacters(in: .whitespaces).isEmpty else { return nil }
        return core.searchLearn(query: searchText)
    }

    public func exercise(id: String) -> ExerciseDetail? {
        _ = history  // re-read when sessions change, so the skill level is current
        return try? core?.exercise(exerciseId: id)
    }

    public func term(id: String) -> TermDetail? {
        try? core?.term(termId: id)
    }

    // MARK: Lifecycle

    /// Try Again on the failure screen: opens the core if it isn't open,
    /// otherwise reads everything again (a failure after opening, such as
    /// today's plan, leaves the core open, where `open()` does nothing).
    public func retry() async {
        if core == nil {
            await open()
        } else {
            await reload()
        }
    }

    /// Opens the Rust core through the host, then reads what the screens
    /// show. The host may already be open: Device Lab keeps one host for
    /// the process and makes a new model each time Good Dog's sheet opens,
    /// so this model still has to read.
    public func open() async {
        if core == nil {
            phase = .loading
            await host.start()
        }
        guard let core else {
            if case .failed(let message) = host.status {
                log.error("could not open: \(message)")
                phase = .failed(message)
            }
            return
        }
        do {
            if let sampleDog, core.dog() == nil {
                _ = try await core.saveDog(profile: sampleDog, now: clock())
            }
        } catch {
            log.warn("sample dog not saved: \(error)")
        }
        await core.refreshReminders(now: clock())
        await reload()
    }

    /// Shuts the core down (tests, and the app ending).
    public func close() {
        host.close()
    }

    /// Reports a lifecycle phase to the host (ForgeLifecycle does this in
    /// the app; tests call it).
    public func setPhase(_ phase: AppPhase) {
        let resumed = host.phase == .background && phase != .background && phase != .terminated
        host.report(phase)
        if resumed { Task { await self.resumed() } }
    }

    /// The host's `changes` moved: a command committed, events were missed,
    /// or the app came back. Reads everything again.
    public func hostChanged() async {
        guard phase != .loading else { return }
        await reload()
    }

    /// Back from the background, possibly on a new day: the reminders'
    /// week moves on and today's plan may be a new one.
    public func resumed() async {
        await core?.refreshReminders(now: clock())
        await reload()
    }

    /// Re-reads everything the screens show from Rust. Building today's plan
    /// may save, so it is awaited off the main actor.
    public func reload() async {
        guard let core else { return }
        dog = core.dog()
        topics = core.learnTopics()
        glossary = core.glossary()
        history = core.history(limit: 50)
        progress = core.progress(now: clock())
        reminderMinute = core.reminderMinute()
        goals = core.goals()
        pathways = core.pathways()
        revision += 1
        guard dog != nil else {
            today = nil
            todayError = nil
            phase = .needsDog
            return
        }
        do {
            today = try await core.today(now: clock())
            todayError = nil
        } catch {
            // Only Today depends on the plan: Learn and My Dog stay usable.
            log.warn("today failed: \(error)")
            today = nil
            todayError = Self.describe(error)
        }
        phase = .ready
    }

    // MARK: Commands

    /// Creates or changes the dog's profile. Throws Rust's `AppError`, so a
    /// form can stay open and show it.
    public func saveDog(_ profile: DogProfile) async throws {
        guard let core else { throw AppError.ShutDown }
        _ = try await core.saveDog(profile: profile, now: clock())
        await reload()
    }

    public func shortenToday() async {
        await perform { core, now in self.today = try await core.shortenToday(now: now) }
    }

    public func swap(_ activity: PlanActivity) async {
        await perform { core, now in
            self.today = try await core.swapActivity(activityId: activity.id, now: now)
        }
    }

    /// Records a session and returns what to tell the owner. Throws, so the
    /// result sheet can stay open on failure.
    public func record(
        exerciseId: String,
        activityId: UInt64?,
        result: SessionResult,
        welfareConcern: Bool
    ) async throws -> SessionFeedback {
        guard let core else { throw AppError.ShutDown }
        let feedback = try await core.recordSession(
            exerciseId: exerciseId,
            activityId: activityId,
            result: result,
            welfareConcern: welfareConcern,
            now: clock()
        )
        sessionCount += 1
        await reload()
        return feedback
    }

    /// Starts a guided session of an exercise on its "get ready" card.
    /// Throws Rust's `AppError` when the session couldn't be recorded at
    /// the end (the activity is already done, for example).
    public func startGuidedSession(exerciseId: String, activityId: UInt64?) throws -> GuidedSessionModel {
        guard let core else { throw AppError.ShutDown }
        let session = try core.startGuidedSession(exerciseId: exerciseId, activityId: activityId, now: clock())
        return GuidedSessionModel(session: session, app: self)
    }

    /// Records a guided session from its outcome card. Rust applies the
    /// same rules and writes the same record as `record`.
    func recordGuided(
        _ session: GuidedSession,
        result: SessionResult,
        welfareConcern: Bool
    ) async throws -> SessionFeedback {
        guard let core else { throw AppError.ShutDown }
        let feedback = try await core.recordGuidedSession(
            session: session,
            result: result,
            welfareConcern: welfareConcern,
            now: clock()
        )
        sessionCount += 1
        await reload()
        return feedback
    }

    // MARK: Goals

    public var activeGoals: [GoalSummary] { goals.filter { $0.status == .active } }

    public func goal(id: UInt64) -> GoalDetail? {
        try? core?.goal(goalId: id, now: clock())
    }

    /// Goals matching what the owner typed, best first; empty when nothing
    /// matches (show the whole list then).
    public func suggestGoals(_ ownWords: String) -> [PathwaySummary] {
        guard !ownWords.trimmingCharacters(in: .whitespaces).isEmpty else { return [] }
        return core?.suggestGoals(ownWords: ownWords) ?? []
    }

    /// Starts a goal (or picks it up again). Throws Rust's refusal, such as
    /// already working on three.
    @discardableResult
    public func startGoal(pathwayId: String, ownWords: String?) async throws -> GoalDetail {
        guard let core else { throw AppError.ShutDown }
        let words = ownWords?.trimmingCharacters(in: .whitespacesAndNewlines)
        let detail = try await core.startGoal(
            pathwayId: pathwayId,
            ownWords: (words?.isEmpty ?? true) ? nil : words,
            now: clock()
        )
        await reload()
        return detail
    }

    public func pauseGoal(_ id: UInt64) async {
        await perform { core, now in _ = try await core.pauseGoal(goalId: id, now: now) }
        await reload()
    }

    public func resumeGoal(_ id: UInt64) async {
        await perform { core, now in _ = try await core.resumeGoal(goalId: id, now: now) }
        await reload()
    }

    public func setGoalAside(_ id: UInt64) async {
        await perform { core, now in _ = try await core.setGoalAside(goalId: id, now: now) }
        await reload()
    }

    public func stepBack(_ id: UInt64) async {
        await perform { core, now in _ = try await core.stepBack(goalId: id, now: now) }
        await reload()
    }

    /// Meets a stage's safety rule: confirms how the owner practises, or
    /// says they've had advice after a worry.
    public func noteSafety(_ goalId: UInt64, ruleId: String) async {
        await perform { core, now in
            _ = try await core.noteSafety(goalId: goalId, ruleId: ruleId, now: now)
        }
        await reload()
    }

    /// Marks a learn or everyday lesson done.
    public func completeLesson(_ goalId: UInt64, lessonId: String) async {
        await perform { core, now in
            _ = try await core.completeLesson(goalId: goalId, lessonId: lessonId, now: now)
        }
        await reload()
    }

    /// Answers a quick check. Nil when Rust refused it (`actionError` says
    /// why); otherwise right or not, with the explanation.
    public func answerLesson(_ goalId: UInt64, lessonId: String, choice: UInt32) async -> LessonAnswer? {
        var answer: LessonAnswer?
        await perform { core, now in
            answer = try await core.answerLesson(goalId: goalId, lessonId: lessonId, choice: choice, now: now)
        }
        await reload()
        return answer
    }

    /// The owner's answer to the current stage's question. A "yes" that
    /// moves the goal on sets `celebration`.
    public func answerGate(_ id: UInt64, met: Bool) async {
        let before = goal(id: id)
        await perform { core, now in
            let after = try await core.answerGate(goalId: id, met: met, now: now)
            if let before, let celebration = StageCelebration(before: before, after: after) {
                self.celebration = celebration
            }
        }
        await reload()
    }

    /// "Not today" on Today, or `false` to take it back.
    public func setNotToday(_ notToday: Bool) async {
        await perform { core, now in self.today = try await core.setNotToday(notToday: notToday, now: now) }
    }

    /// Turns the daily reminder on at `minute` after midnight, or off with
    /// nil. iOS asks for permission the first time; if the owner declines
    /// it stays off.
    public func setReminder(_ minute: UInt32?) async {
        await perform { core, now in
            _ = try await core.setReminder(atMinute: minute, now: now)
        }
        reminderMinute = core?.reminderMinute()
    }

    /// Deletes everything and returns to onboarding.
    public func eraseAll() async {
        await perform { core, _ in try await core.eraseAll() }
        await reload()
    }

    // MARK: Private

    /// Runs a command, keeping Rust's refusal for the screen. A command
    /// whose effect the caller reads straight away reloads after it: the
    /// `dataChanged` event reloads too, but later, on its own task.
    private func perform(_ command: (AppCore, LocalTime) async throws -> Void) async {
        guard let core else { return }
        do {
            try await command(core, clock())
        } catch {
            log.warn("command failed: \(error)")
            actionError = ForgeFailure(error)
        }
    }

    /// `error` as an alert titled `title` ("Couldn't Save"), in Good Dog's
    /// words.
    nonisolated public static func failure(_ error: Error, title: String) -> ForgeFailure {
        ForgeFailure(title: title, message: describe(error))
    }

    nonisolated public static func title(_ error: Error) -> String {
        guard let error = error as? AppError else { return "Something Went Wrong" }
        switch error {
        case .InvalidInput: return "Check the Details"
        case .NotFound: return "Not Found"
        case .NoDog: return "No Dog Yet"
        case .NoAlternative: return "Nothing Else Suits Today"
        case .Storage: return "Couldn't Save"
        case .InUse: return "Good Dog Is Already Open"
        case .ShutDown: return "Good Dog Is Closing"
        }
    }

    nonisolated public static func describe(_ error: Error) -> String {
        guard let error = error as? AppError else { return "Try again. If it keeps happening, close the app and open it again." }
        switch error {
        case .InvalidInput(let reason): return "Please check: \(reason)."
        case .NotFound: return "That's no longer available."
        case .NoDog: return "Tell us about your dog first."
        case .NoAlternative: return "There's nothing else that suits today. Try this one, or keep it short."
        case .Storage(let reason): return "Good Dog couldn't read or save its data (\(reason))."
        case .InUse: return "Good Dog's data is open somewhere else in this app. Close it there and try again."
        case .ShutDown: return "Good Dog is closing."
        }
    }

    /// Application Support/GoodDog, or a fresh GoodDog-UITest folder when
    /// launched with `-ForgeUITestData` (forge.toml), so UI tests never touch
    /// the owner's real data.
    nonisolated static func defaultDirectory() throws -> String {
        let testData = usesTestData
        let base = try FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent(testData ? "GoodDog-UITest" : "GoodDog", isDirectory: true)
        if testData { try? FileManager.default.removeItem(at: base) }
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base.path
    }
}

/// What to celebrate when a stage is passed.
public struct StageCelebration: Equatable, Sendable {
    public let goalTitle: String
    public let passedStage: String
    /// The next stage, or nil when the goal is finished.
    public let nextStage: String?
    public let stageNumber: UInt32
    public let stageCount: UInt32

    /// Nil unless `after` is further on than `before`.
    public init?(before: GoalDetail, after: GoalDetail) {
        let finished = after.summary.status == .finished && before.summary.status != .finished
        guard finished || after.summary.stageNumber > before.summary.stageNumber else { return nil }
        goalTitle = after.summary.title
        passedStage = before.summary.stageTitle
        nextStage = finished ? nil : after.summary.stageTitle
        stageNumber = before.summary.stageNumber
        stageCount = after.summary.stageCount
    }
}

extension LocalTime {
    /// The fixed time of a launch on test data: Monday 9 November 2026,
    /// 09:30 UTC, with no offset.
    public static let uiTest = LocalTime(ms: 1794216600000, utcOffsetMinutes: 0)

    /// The time now, with the phone's current offset from UTC.
    public static func now() -> LocalTime {
        LocalTime(
            ms: Int64(Date().timeIntervalSince1970 * 1000),
            utcOffsetMinutes: Int32(TimeZone.current.secondsFromGMT() / 60)
        )
    }
}
