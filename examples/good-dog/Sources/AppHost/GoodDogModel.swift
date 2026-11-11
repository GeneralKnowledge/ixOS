@preconcurrency import ForgeFFI
import Foundation
import Observation

private let log = ForgeLog("GoodDog")

/// The app's presentation state. It holds no training rules: every change
/// goes to the Rust `AppCore` (rust/src/lib.rs), which builds plans, records
/// sessions and moves skills on. This model keeps the latest answers for the
/// screens and re-reads them when Rust says the data changed.
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
    /// A failed action, for an alert. The view clears it when dismissed.
    public var actionError: String?
    /// The alert's title for `actionError`: what kind of problem it was.
    public private(set) var actionErrorTitle = "Something Went Wrong"
    /// Why today's plan couldn't be built. The Today tab shows it with Try
    /// Again; the other tabs keep working.
    public private(set) var todayError: String?
    /// Increases after each recorded session; views use it as a haptic trigger.
    public private(set) var sessionCount = 0

    public private(set) var core: AppCore?
    private let storageDirectory: String?
    private let clock: @Sendable () -> LocalTime
    /// Set up before the first read when the store has no dog yet.
    private let sampleDog: DogProfile?
    @ObservationIgnored private var eventTask: Task<Void, Never>?
    @ObservationIgnored private var events: AppEvents?
    @ObservationIgnored private var lifecyclePhase: AppPhase = .launching

    /// - Parameters:
    ///   - storageDirectory: where Rust keeps its data. Defaults to
    ///     Application Support (a throwaway folder under UI tests); tests
    ///     pass a temporary one.
    ///   - clock: the time now with the phone's UTC offset; tests fix it.
    public init(
        storageDirectory: String? = nil,
        clock: (@Sendable () -> LocalTime)? = nil,
        sampleDog: DogProfile? = GoodDogModel.launchSampleDog
    ) {
        self.storageDirectory = storageDirectory
        self.sampleDog = sampleDog
        if let clock {
            self.clock = clock
        } else if Self.usesTestData {
            self.clock = { LocalTime.uiTest }
        } else {
            self.clock = { LocalTime.now() }
        }
    }

    /// Whether this launch uses throwaway test data (`-ForgeUITestData`, as
    /// `forge ui run` and `forge ui explore` pass it). Such a launch also
    /// runs on a fixed clock, so dates and times on screen don't change
    /// between runs and screenshots stay comparable.
    static let usesTestData = ProcessInfo.processInfo.arguments.contains("-ForgeUITestData")

    /// With `-ForgeUITestData`, `-ForgeUITestSampleDog` starts on a dog
    /// that is already set up, so `forge ui explore` can reach Today and My
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

    /// Opens the Rust core and starts listening for its events. Calling it
    /// again while open does nothing.
    public func open() async {
        guard core == nil else { return }
        phase = .loading
        do {
            let directory = try storageDirectory ?? Self.defaultDirectory()
            let core = try await Task.detached { try AppCore(storageDirectory: directory) }.value
            let events = try core.events()
            self.core = core
            self.events = events
            listen(to: events)
            if lifecyclePhase != .launching { core.setPhase(phase: lifecyclePhase) }
            if let sampleDog, core.dog() == nil {
                _ = try await core.saveDog(profile: sampleDog, now: clock())
            }
            await reload()
        } catch {
            log.error("could not open: \(error)")
            phase = .failed(Self.describe(error))
        }
    }

    public func close() {
        events?.cancel()
        eventTask?.cancel()
        core?.shutdown()
        core = nil
        events = nil
    }

    public func setPhase(_ phase: AppPhase) {
        lifecyclePhase = phase
        if phase == .terminated {
            close()
        } else {
            core?.setPhase(phase: phase)
        }
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

    /// Deletes everything and returns to onboarding.
    public func eraseAll() async {
        await perform { core, _ in try await core.eraseAll() }
        await reload()
    }

    // MARK: Private

    private func perform(_ command: (AppCore, LocalTime) async throws -> Void) async {
        guard let core else { return }
        do {
            try await command(core, clock())
        } catch {
            log.warn("command failed: \(error)")
            actionErrorTitle = Self.title(error)
            actionError = Self.describe(error)
        }
    }

    private func listen(to events: AppEvents) {
        eventTask = Task { [weak self] in
            while let event = await events.next() {
                guard let self else { return }
                switch event {
                case .dataChanged, .eventsDropped:
                    await self.reload()
                case .phaseChanged(_, let resumed):
                    // Back from the background, possibly on a new day.
                    if resumed { await self.reload() }
                }
            }
        }
    }

    /// A short alert title for each error, so the alert says what kind of
    /// problem it was rather than "Something Went Wrong" every time.
    public static func title(_ error: Error) -> String {
        guard let error = error as? AppError else { return "Something Went Wrong" }
        switch error {
        case .InvalidInput: return "Check the Details"
        case .NotFound: return "Not Found"
        case .NoDog: return "No Dog Yet"
        case .NoAlternative: return "Nothing Else Suits Today"
        case .Storage: return "Couldn't Save"
        case .ShutDown: return "Good Dog Is Closing"
        }
    }

    public static func describe(_ error: Error) -> String {
        guard let error = error as? AppError else { return "Try again. If it keeps happening, close the app and open it again." }
        switch error {
        case .InvalidInput(let reason): return "Please check: \(reason)."
        case .NotFound: return "That's no longer available."
        case .NoDog: return "Tell us about your dog first."
        case .NoAlternative: return "There's nothing else that suits today. Try this one, or keep it short."
        case .Storage(let reason): return "Good Dog couldn't read or save its data (\(reason))."
        case .ShutDown: return "Good Dog is closing."
        }
    }

    /// Application Support/GoodDog, or a fresh GoodDog-UITest folder when
    /// launched with `-ForgeUITestData` (forge.toml), so UI tests never touch
    /// the owner's real data.
    private static func defaultDirectory() throws -> String {
        let testData = usesTestData
        let base = try FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent(testData ? "GoodDog-UITest" : "GoodDog", isDirectory: true)
        if testData { try? FileManager.default.removeItem(at: base) }
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base.path
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
