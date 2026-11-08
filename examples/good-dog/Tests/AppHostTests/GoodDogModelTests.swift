import AppHost
@preconcurrency import ForgeFFI
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
        XCTAssertEqual(model.sessionCount, 1)
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
}
