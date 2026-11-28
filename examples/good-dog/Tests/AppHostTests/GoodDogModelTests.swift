import GoodDogApp
@preconcurrency import ForgeKit  // ForgeKit builds in Swift 5 mode; `forge check swift` builds tests in 6
@preconcurrency import GoodDogFFI
import Foundation
import XCTest

/// Drives the view model against the app's real Rust core through the
/// generated bindings. No SwiftUI, so `forge check swift` runs it on Linux.
final class GoodDogModelTests: XCTestCase {
    /// 2026-01-15 09:00 UTC.
    private static let nineAM: Int64 = 1_768_467_600_000

    private func makeDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("good-dog-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    @MainActor
    private func openModel(_ dir: URL, dayOffset: Int64 = 0) async -> GoodDogModel {
        let ms = Self.nineAM + dayOffset * 86_400_000
        let model = GoodDogModel(storageDirectory: dir.path) { LocalTime(ms: ms, utcOffsetMinutes: 0) }
        await model.open()
        return model
    }

    private let moss = DogProfile(
        name: " Moss ", lifeStage: .adult, dailyTime: .aboutTenMinutes, goal: .recall, experience: .new
    )

    /// Data the core can't read: the host fails to open it, the app shows
    /// why, and Try Again opens it once it can be read.
    @MainActor
    func testALoadFailureShowsWhyAndTryAgainOpens() async throws {
        let dir = try makeDirectory()
        let file = dir.appendingPathComponent("gooddog.json")
        try Data(#"{"schema_version": 99, "data": {}}"#.utf8).write(to: file)
        let model = await openModel(dir)
        guard case .failed(let message) = model.phase else {
            return XCTFail("opened unreadable data: \(model.phase)")
        }
        XCTAssertFalse(message.isEmpty)
        XCTAssertNil(model.core)

        try FileManager.default.removeItem(at: file)
        await model.retry()
        XCTAssertEqual(model.phase, .needsDog)
        XCTAssertNotNil(model.core)
        model.close()
    }

    /// Device Lab keeps one host for the process and makes a new model each
    /// time Good Dog opens: a model on a host that is already open shows
    /// the data, not a spinner.
    @MainActor
    func testANewModelOnAnOpenHostShowsTheData() async throws {
        let dir = try makeDirectory()
        let host = makeAppHost(storageDirectory: dir.path)
        let clock: @Sendable () -> LocalTime = { LocalTime(ms: Self.nineAM, utcOffsetMinutes: 0) }
        let first = GoodDogModel(host: host, clock: clock, sampleDog: nil)
        await first.open()
        XCTAssertEqual(first.phase, .needsDog)

        let second = GoodDogModel(host: host, clock: clock, sampleDog: nil)
        await second.open()
        XCTAssertEqual(second.phase, .needsDog)
        host.close()
    }

    /// One core per data folder: a second host on the same folder is
    /// refused until the first closes.
    @MainActor
    func testASecondCoreOnTheSameDataIsRefused() async throws {
        let dir = try makeDirectory()
        let first = await openModel(dir)
        XCTAssertEqual(first.phase, .needsDog)
        let second = await openModel(dir)
        guard case .failed = second.phase else {
            return XCTFail("a second core opened the same data")
        }
        XCTAssertEqual(
            appErrorKind(error: AppError.InUse(reason: "")), .unavailable)
        first.close()
        await second.retry()
        XCTAssertEqual(second.phase, .needsDog)
        second.close()
    }

    @MainActor
    func testTryAgainWorksWhileTheCoreIsOpen() async throws {
        let model = await openModel(try makeDirectory())
        try await model.saveDog(moss)
        // A failure after opening leaves the core open, where open() does
        // nothing: Try Again must still read everything again.
        await model.retry()
        XCTAssertEqual(model.phase, .ready)
        XCTAssertNotNil(model.today)
        XCTAssertNil(model.todayError)
        XCTAssertEqual(GoodDogModel.title(AppError.NoAlternative), "Nothing Else Suits Today")
        model.close()
    }

    @MainActor
    func testOnboardingLeadsToATodayPlanThatSurvivesARelaunch() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        XCTAssertEqual(model.phase, .needsDog)
        XCTAssertNil(model.today)

        try await model.saveDog(moss)
        XCTAssertEqual(model.phase, .ready)
        XCTAssertEqual(model.dog?.name, "Moss")
        let plan = try XCTUnwrap(model.today)
        XCTAssertTrue(plan.starter)
        XCTAssertFalse(plan.activities.isEmpty)
        model.close()

        let reopened = await openModel(dir)
        XCTAssertEqual(reopened.phase, .ready)
        XCTAssertEqual(reopened.today, plan)
    }

    @MainActor
    func testRecordingASessionUpdatesTodayProgressAndHistory() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        try await model.saveDog(moss)
        let activity = try XCTUnwrap(model.today?.activities.first)

        let feedback = try await model.record(
            exerciseId: activity.exerciseId, activityId: activity.id, result: .easy, welfareConcern: false
        )
        XCTAssertTrue(feedback.message.contains("Moss"))
        XCTAssertEqual(model.today?.doneCount, 1)
        XCTAssertEqual(model.today?.activities.first?.result, .easy)
        XCTAssertEqual(model.history.first?.exerciseId, activity.exerciseId)
        XCTAssertEqual(model.progress?.sessionsThisWeek, 1)
        XCTAssertEqual(model.haptics.count, 1)
        XCTAssertEqual(model.exercise(id: activity.exerciseId)?.skill.level, .started)

        // Recording it again is refused, and Rust's error arrives typed.
        do {
            _ = try await model.record(
                exerciseId: activity.exerciseId, activityId: activity.id, result: .easy, welfareConcern: false
            )
            XCTFail("a second record of the same activity should fail")
        } catch let error as AppError {
            guard case .InvalidInput = error else { return XCTFail("unexpected \(error)") }
        }
    }

    @MainActor
    func testInvalidNamesAreRefusedAndChangeNothing() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        var blank = moss
        blank.name = "  "
        do {
            try await model.saveDog(blank)
            XCTFail("a blank name should be refused")
        } catch {
            XCTAssertTrue(GoodDogModel.describe(error).hasPrefix("Please check"))
        }
        XCTAssertEqual(model.phase, .needsDog)
        XCTAssertNil(model.dog)
    }

    @MainActor
    func testSwapShortenAndErase() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        try await model.saveDog(moss)
        let first = try XCTUnwrap(model.today?.activities.first)
        await model.swap(first)
        XCTAssertNil(model.actionError)
        XCTAssertNotEqual(model.today?.activities.first?.exerciseId, first.exerciseId)

        await model.shortenToday()
        XCTAssertEqual(model.today?.shortPlan, true)
        XCTAssertEqual(model.today?.activities.count, 1)

        await model.eraseAll()
        XCTAssertEqual(model.phase, .needsDog)
        XCTAssertNil(model.dog)
        XCTAssertTrue(model.history.isEmpty)
    }

    @MainActor
    func testLearnSearchAndGlossaryComeFromRust() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        XCTAssertFalse(model.topics.isEmpty)
        XCTAssertFalse(model.glossary.isEmpty)
        XCTAssertNil(model.searchResults)
        model.searchText = "recall"
        XCTAssertFalse(model.searchResults?.exercises.isEmpty ?? true)
        let term = try XCTUnwrap(model.term(id: "positive-reinforcement"))
        XCTAssertFalse(term.explanation.plain.contains("[["))
    }

    @MainActor
    func testANewDayBringsANewPlan() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        try await model.saveDog(moss)
        let today = try XCTUnwrap(model.today)
        model.close()
        let tomorrow = await openModel(dir, dayOffset: 1)
        XCTAssertNotEqual(tomorrow.today?.activities.first?.id, today.activities.first?.id)
    }

    /// GD-7: a guided session shows one card at a time, moves only where
    /// Rust allows, and records exactly as the exercise screen does.
    @MainActor
    func testAGuidedSessionStepsThroughAndRecordsLikeTheExerciseScreen() async throws {
        let guidedModel = await openModel(try makeDirectory())
        let plainModel = await openModel(try makeDirectory())
        try await guidedModel.saveDog(moss)
        try await plainModel.saveDog(moss)
        let activity = try XCTUnwrap(guidedModel.today?.activities.first)
        XCTAssertEqual(plainModel.today?.activities.first, activity)
        let exercise = try XCTUnwrap(guidedModel.exercise(id: activity.exerciseId))
        let stepCount = exercise.steps.count
        XCTAssertGreaterThan(stepCount, 1)

        let guided = try guidedModel.startGuidedSession(exerciseId: activity.exerciseId, activityId: activity.id)
        XCTAssertEqual(guided.title, exercise.title)
        XCTAssertEqual(guided.ifDifficult, exercise.ifDifficult)
        XCTAssertEqual(guided.card.kind, .getReady)
        XCTAssertEqual(guided.card.heading, "Get ready")
        XCTAssertEqual(guided.card.text, exercise.preparation)
        XCTAssertFalse(guided.card.canGoBack)
        guided.back()  // refused at the start, and harmless
        XCTAssertEqual(guided.card.kind, .getReady)

        for number in 1...stepCount {
            guided.next()
            XCTAssertEqual(guided.card.heading, "Step \(number) of \(stepCount)")
            XCTAssertEqual(guided.card.text, exercise.steps[number - 1])
            XCTAssertFalse(guided.card.canRecord)
        }
        // Too early to record on the last step: Rust refuses, nothing saved.
        do {
            _ = try await guided.record(result: .gettingThere, welfareConcern: false)
            XCTFail("recording before the outcome card should fail")
        } catch let error as AppError {
            guard case .InvalidInput = error else { return XCTFail("unexpected \(error)") }
        }
        XCTAssertTrue(guidedModel.history.isEmpty)

        guided.next()
        XCTAssertEqual(guided.card.kind, .outcome)
        XCTAssertEqual(guided.card.text, exercise.lookFor)
        guided.back()
        XCTAssertEqual(guided.card.heading, "Step \(stepCount) of \(stepCount)")
        guided.next()
        XCTAssertTrue(guided.card.canRecord)
        XCTAssertFalse(guided.card.canGoNext)

        let fromGuided = try await guided.record(result: .gettingThere, welfareConcern: false)
        let fromScreen = try await plainModel.record(
            exerciseId: activity.exerciseId, activityId: activity.id, result: .gettingThere, welfareConcern: false
        )
        XCTAssertEqual(fromGuided, fromScreen)
        XCTAssertEqual(guidedModel.today, plainModel.today)
        XCTAssertEqual(guidedModel.today?.activities.first?.result, .gettingThere)
        XCTAssertEqual(guidedModel.history, plainModel.history)
        XCTAssertEqual(guidedModel.progress, plainModel.progress)
        XCTAssertEqual(guidedModel.haptics.count, 1)
        XCTAssertTrue(guided.card.recorded)
        XCTAssertFalse(guided.card.canRecord)

        // Once only, and a done activity starts no new session.
        do {
            _ = try await guided.record(result: .easy, welfareConcern: false)
            XCTFail("a second record should fail")
        } catch is AppError {}
        XCTAssertEqual(guidedModel.history.count, 1)
        XCTAssertThrowsError(
            try guidedModel.startGuidedSession(exerciseId: activity.exerciseId, activityId: activity.id)
        )
    }

    /// GD-7: closing a guided session part way, or the app ending, records
    /// nothing, and the next session starts from "get ready".
    @MainActor
    func testLeavingAGuidedSessionPartWayRecordsNothing() async throws {
        let dir = try makeDirectory()
        let model = await openModel(dir)
        try await model.saveDog(moss)
        let activity = try XCTUnwrap(model.today?.activities.first)

        var guided: GuidedSessionModel? = try model.startGuidedSession(
            exerciseId: activity.exerciseId, activityId: activity.id
        )
        guided?.next()
        guided?.next()
        XCTAssertEqual(guided?.card.kind, .step)
        guided = nil  // closed
        XCTAssertTrue(model.history.isEmpty)
        XCTAssertNil(model.today?.activities.first?.result)

        let again = try model.startGuidedSession(exerciseId: activity.exerciseId, activityId: activity.id)
        again.next()
        model.setPhase(.terminated)  // the app ends part way

        let reopened = await openModel(dir)
        XCTAssertTrue(reopened.history.isEmpty)
        XCTAssertEqual(reopened.progress?.sessionsTotal, 0)
        XCTAssertNil(reopened.today?.activities.first?.result)
        let fresh = try reopened.startGuidedSession(exerciseId: activity.exerciseId, activityId: activity.id)
        XCTAssertEqual(fresh.card.kind, .getReady)
        XCTAssertEqual(fresh.card.position, 0)
    }

    /// `-ForgeUITestSampleDog` starts past onboarding, and never replaces a
    /// dog that already exists.
    @MainActor
    func testASampleDogSkipsOnboardingButNeverReplacesADog() async throws {
        let dir = try makeDirectory()
        let sample = DogProfile(name: "Sample", lifeStage: .adult, dailyTime: .fewMinutes, goal: .general, experience: .new)
        let clock: @Sendable () -> LocalTime = { LocalTime(ms: Self.nineAM, utcOffsetMinutes: 0) }

        let first = GoodDogModel(storageDirectory: dir.path, clock: clock, sampleDog: sample)
        await first.open()
        XCTAssertEqual(first.phase, .ready)
        XCTAssertEqual(first.dog?.name, "Sample")
        XCTAssertNotNil(first.today)
        try await first.saveDog(moss)
        first.close()

        let second = GoodDogModel(storageDirectory: dir.path, clock: clock, sampleDog: sample)
        await second.open()
        XCTAssertEqual(second.dog?.name, "Moss")
        second.close()
    }

    @MainActor
    func testOnboardingStartsTheChosenGoalAndTodayLeadsWithIt() async throws {
        let model = await openModel(try makeDirectory())
        try await model.saveDog(moss)
        XCTAssertEqual(model.pathways.count, 4)
        XCTAssertEqual(model.activeGoals.map(\.pathwayId), ["path-recall"])
        let focus = try XCTUnwrap(model.today?.goalFocus)
        XCTAssertEqual(focus.stageNumber, 1)
        XCTAssertFalse(focus.everydayPractice.isEmpty)
        XCTAssertEqual(model.suggestGoals("he pulls on the lead").first?.id, "path-loose-lead")
        XCTAssertTrue(model.suggestGoals("   ").isEmpty)
    }

    @MainActor
    func testPassingAStageCelebratesAndPauseKeepsTheGoal() async throws {
        let model = await openModel(try makeDirectory())
        try await model.saveDog(moss)
        let goal = try XCTUnwrap(model.activeGoals.first)
        await model.answerGate(goal.id, met: false)
        XCTAssertNil(model.celebration, "not yet is not a celebration")
        await model.answerGate(goal.id, met: true)
        XCTAssertNil(model.actionError)
        let celebration = try XCTUnwrap(model.celebration)
        XCTAssertEqual(celebration.stageNumber, 1)
        XCTAssertNotNil(celebration.nextStage)
        XCTAssertEqual(model.goal(id: goal.id)?.summary.stageNumber, 2)

        await model.pauseGoal(goal.id)
        XCTAssertTrue(model.activeGoals.isEmpty)
        await model.resumeGoal(goal.id)
        XCTAssertEqual(model.activeGoals.first?.stageNumber, 2)
        let second = try await model.startGoal(pathwayId: "path-settle", ownWords: "  can't relax  ")
        XCTAssertEqual(second.summary.ownWords, "can't relax")
        XCTAssertEqual(model.activeGoals.count, 2)
    }

    @MainActor
    func testNotTodayIsARestDayAndTheWeekCountsDaysNotStreaks() async throws {
        let model = await openModel(try makeDirectory())
        try await model.saveDog(moss)
        XCTAssertEqual(model.today?.canSayNotToday, true)
        await model.setNotToday(true)
        XCTAssertEqual(model.today?.notToday, true)
        await model.setNotToday(false)
        XCTAssertEqual(model.today?.notToday, false)

        let rhythm = try XCTUnwrap(model.progress?.rhythm)
        XCTAssertEqual(rhythm.targetDays, 3)
        XCTAssertEqual(rhythm.daysPractised, 0)
        XCTAssertEqual(rhythm.todayIndex, 3, "15 January 2026 is a Thursday")
        XCTAssertFalse(rhythm.summary.isEmpty)
    }

    @MainActor
    func testLessonsUnlockOneByOneAndAQuickCheckExplainsEitherWay() async throws {
        let model = await openModel(try makeDirectory())
        try await model.saveDog(moss)
        let goal = try XCTUnwrap(model.activeGoals.first)
        let lessons = try XCTUnwrap(model.goal(id: goal.id)?.stages.first?.lessons)
        XCTAssertEqual(lessons.first?.state, .next)
        XCTAssertEqual(model.today?.goalFocus?.nextLesson?.id, lessons.first?.id)

        await model.completeLesson(goal.id, lessonId: lessons[2].id)
        XCTAssertNotNil(model.actionError, "a locked lesson waits its turn")
        model.actionError = nil
        await model.completeLesson(goal.id, lessonId: lessons[0].id)
        XCTAssertNil(model.actionError)
        XCTAssertEqual(model.today?.goalFocus?.lessonsDone, 1)

        let check = try XCTUnwrap(lessons.last)
        XCTAssertEqual(check.kind, .check)
        let early = await model.answerLesson(goal.id, lessonId: check.id, choice: 1)
        XCTAssertNil(early, "the check is locked until the lessons before it are done")
    }

    @MainActor
    func testASafetyRuleIsConfirmedBeforeTheStageCanMoveOn() async throws {
        let model = await openModel(try makeDirectory())
        try await model.saveDog(moss)
        let goal = try XCTUnwrap(model.activeGoals.first)
        await model.answerGate(goal.id, met: true)
        model.celebration = nil
        let gate = try XCTUnwrap(model.goal(id: goal.id)?.gate)
        let check = try XCTUnwrap(gate.safety.first, "coming back on a long line names the recall safety rule")
        XCTAssertEqual(check.kind, .confirm)
        XCTAssertFalse(check.met)

        await model.answerGate(goal.id, met: true)
        XCTAssertNotNil(model.actionError, "moving on waits for the confirmation")
        XCTAssertNil(model.celebration)
        model.actionError = nil

        await model.noteSafety(goal.id, ruleId: check.ruleId)
        XCTAssertNil(model.actionError)
        XCTAssertEqual(model.goal(id: goal.id)?.gate?.safety.first?.met, true)
    }

    /// Defect hunt finding 5: the reminders' week learns of the clock
    /// going forward, at 01:00 UTC on Sunday 28 March 2027 in London.
    func testOffsetChangesFindTheClockGoingForward() throws {
        let london = try XCTUnwrap(TimeZone(identifier: "Europe/London"))
        let saturday = Date(timeIntervalSince1970: 1_806_138_000) // 09:00 GMT
        let changes = LocalTime.offsetChanges(in: london, from: saturday)
        XCTAssertEqual(changes, [OffsetChange(atMs: 1_806_195_600_000, utcOffsetMinutes: 60)])
        XCTAssertEqual(LocalTime.offsetChanges(in: london, from: saturday, days: 0), [])
    }
}
