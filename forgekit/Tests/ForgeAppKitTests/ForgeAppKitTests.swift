import ForgeAppKit
import ForgeUI
import XCTest

/// ForgeAppKit's model values are plain Swift, so the behaviour a screen
/// relies on is checked here on Linux. The SwiftUI side is compiled on
/// macOS CI and with xtool.
final class ForgeAppKitTests: XCTestCase {
    enum TestError: Error { case plain }

    enum LibraryError: ForgeDescribedError {
        case offline
        case cancelled

        var failure: ForgeFailure {
            switch self {
            case .offline: ForgeFailure(title: "You're Offline", message: "Try again when you're connected.", isRetryable: true)
            case .cancelled: ForgeFailure(title: "Stopped", message: "Nothing changed.")
            }
        }

        var isCancellation: Bool { self == .cancelled }
    }

    enum Action: Equatable { case delete(Int) }

    // MARK: Failures

    func testDescribedErrorsUseTheAppsWords() {
        let failure = ForgeFailure(LibraryError.offline)
        XCTAssertEqual(failure.title, "You're Offline")
        XCTAssertTrue(failure.isRetryable)
    }

    func testOtherErrorsGetAGenericTitle() {
        let failure = ForgeFailure(TestError.plain)
        XCTAssertEqual(failure.title, "Something Went Wrong")
        XCTAssertFalse(failure.isRetryable)
    }

    func testOtherErrorsNeverShowSwiftsErrorNumber() {
        let failure = ForgeFailure(TestError.plain)
        XCTAssertFalse(failure.message.contains("couldn"), failure.message)
    }

    func testURLSessionOfflineIsRetryable() {
        let failure = ForgeFailure(URLError(.notConnectedToInternet))
        XCTAssertEqual(failure.title, "You're Offline")
        XCTAssertTrue(failure.isRetryable)
        XCTAssertTrue(ForgeFailure(URLError(.timedOut)).isRetryable)
    }

    func testCancellationIsNotAFailure() {
        XCTAssertTrue(ForgeFailure.isCancellation(CancellationError()))
        XCTAssertTrue(ForgeFailure.isCancellation(LibraryError.cancelled))
        XCTAssertFalse(ForgeFailure.isCancellation(LibraryError.offline))
        XCTAssertNil(ForgeAlert<Never>.failure(LibraryError.cancelled as Error))
    }

    // MARK: Alerts

    func testErrorAlertHasOneOKButtonTestsCanFind() throws {
        let alert = try XCTUnwrap(ForgeAlert<Never>.failure(LibraryError.offline as Error))
        XCTAssertEqual(alert.title, "You're Offline")
        XCTAssertEqual(alert.message, "Try again when you're connected.")
        XCTAssertEqual(alert.style, .alert)
        XCTAssertEqual(alert.buttons.map(\.identifier), ["alert.ok"])
        XCTAssertEqual(alert.buttons.first?.role, .cancel)
    }

    func testRetryableFailureAlertOffersTryAgain() throws {
        let alert = try XCTUnwrap(ForgeAlert.failure(LibraryError.offline as Error, retry: Action.delete(3)))
        XCTAssertEqual(alert.buttons.map(\.title), ["Try Again", "Cancel"])
        XCTAssertEqual(alert.buttons.map(\.identifier), ["alert.retry", "alert.ok"])
        XCTAssertEqual(alert.action(for: "Try Again"), .delete(3))
    }

    func testFailureThatWontRecoverHasOnlyOK() {
        let alert = ForgeAlert.failure(ForgeFailure(title: "No", message: "Nope."), retry: Action.delete(3))
        XCTAssertEqual(alert.buttons.map(\.identifier), ["alert.ok"])
        XCTAssertNil(ForgeAlert.failure(LibraryError.cancelled as Error, retry: Action.delete(3)))
    }

    func testDestructiveConfirmationIsADialogWithCancel() {
        let alert = ForgeAlert.confirm(
            "Delete “Dune”?", message: "This can't be undone.",
            confirm: "Delete Book", action: Action.delete(7),
            identifier: "book.delete", destructive: true
        )
        XCTAssertEqual(alert.style, .confirmationDialog)
        XCTAssertEqual(alert.buttons.map(\.title), ["Delete Book", "Cancel"])
        XCTAssertEqual(alert.buttons.map(\.identifier), ["book.delete.confirm", "book.delete.cancel"])
        XCTAssertEqual(alert.buttons.map(\.role), [.destructive, .cancel])
        XCTAssertEqual(alert.action(for: "Delete Book"), .delete(7))
        XCTAssertNil(alert.action(for: "Cancel"))
    }

    func testAlertsCompareByContentNotIdentity() {
        let a = ForgeAlert.confirm("Erase?", confirm: "Erase", action: Action.delete(1), identifier: "x")
        let b = ForgeAlert.confirm("Erase?", confirm: "Erase", action: Action.delete(1), identifier: "x")
        XCTAssertNotEqual(a.id, b.id)
        XCTAssertEqual(a, b)
        XCTAssertEqual(a.style, .alert, "a non-destructive question is an alert")
    }

    // MARK: Load state

    func testCaptureMapsOutcomes() async {
        let loaded = await ForgeLoad<Int>.capture { 3 }
        XCTAssertEqual(loaded, .loaded(3))
        XCTAssertEqual(loaded.value, 3)
        XCTAssertEqual(loaded.map { $0 * 2 }, .loaded(6))

        let failed = await ForgeLoad<Int>.capture { throw LibraryError.offline }
        XCTAssertEqual(failed.failure?.title, "You're Offline")
        XCTAssertNil(failed.value)

        let stopped = await ForgeLoad<Int>.capture { throw CancellationError() }
        XCTAssertEqual(stopped, .idle)
        XCTAssertTrue(ForgeLoad<Int>.loading.isLoading)
    }

    func testReloadKeepsContentAndNeverStrandsASpinner() async {
        let shown = ForgeLoad<Int>.loaded(1)
        XCTAssertEqual(shown.whileReloading, .loaded(1), "loaded content stays while reloading")
        XCTAssertEqual(ForgeLoad<Int>.idle.whileReloading, .loading)

        let fresh = await shown.reloaded { 2 }
        XCTAssertEqual(fresh, .loaded(2))
        let stopped = await shown.reloaded { throw CancellationError() }
        XCTAssertEqual(stopped, .loaded(1), "a stopped reload changes nothing")
        let stoppedSpinner = await ForgeLoad<Int>.loading.reloaded { throw CancellationError() }
        XCTAssertEqual(stoppedSpinner, .idle)
        let failed = await shown.reloaded { throw LibraryError.offline }
        XCTAssertEqual(failed.failure?.title, "You're Offline")
    }

    // MARK: Submission

    @MainActor
    func testSubmissionReportsSuccessAndFailure() async {
        let saving = ForgeSubmission()
        var ran = 0
        let ok = await saving.run { ran += 1 }
        XCTAssertTrue(ok)
        XCTAssertEqual(ran, 1)
        XCTAssertFalse(saving.isWorking)
        XCTAssertNil(saving.failure)

        let failed = await saving.run { throw LibraryError.offline }
        XCTAssertFalse(failed)
        XCTAssertEqual(saving.failure?.title, "You're Offline")

        // The next run starts clean; a cancellation shows nothing.
        let stopped = await saving.run { throw CancellationError() }
        XCTAssertFalse(stopped)
        XCTAssertNil(saving.failure)
    }

    @MainActor
    func testSubmissionIgnoresASecondTapWhileWorking() async {
        let saving = ForgeSubmission()
        var runs = 0
        var inner: Bool?
        await saving.run {
            runs += 1
            XCTAssertTrue(saving.isWorking)
            inner = await saving.run { runs += 1 }
        }
        XCTAssertEqual(runs, 1)
        XCTAssertEqual(inner, false)
    }

    // MARK: Form errors

    func testFieldErrorsKeepOrderAndFirstMessage() {
        var errors = ForgeFieldErrors([("title", "Enter a title."), ("pages", "Too many."), ("title", "Second.")])
        XCTAssertEqual(errors.fields, ["title", "pages"])
        XCTAssertEqual(errors["title"], .error("Enter a title."))
        XCTAssertNil(errors["author"])
        XCTAssertEqual(errors.message(for: "author", otherwise: .help("Optional.")), .help("Optional."))
        XCTAssertEqual(errors.summary, "Enter a title.\nToo many.")

        errors.clear("title")
        XCTAssertEqual(errors.fields, ["pages"])
        XCTAssertFalse(errors.isEmpty)
        errors.clear("pages")
        XCTAssertTrue(errors.isEmpty)
        XCTAssertEqual(ForgeFieldErrors(), [:])
    }

    // MARK: Picked files

    func testPickedFileCopyKeepsTheNameAndIsReleased() async throws {
        let base = FileManager.default.temporaryDirectory.appendingPathComponent("picked-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: base) }
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        let source = base.appendingPathComponent("Holiday video.mov")
        try Data("frames".utf8).write(to: source)
        let inbox = base.appendingPathComponent("inbox")

        let first = try await ForgePickedFile.copy(of: source, into: inbox)
        let second = try await ForgePickedFile.copy(of: source, into: inbox)
        XCTAssertEqual(first.name, "Holiday video.mov")
        XCTAssertNotEqual(first.url, second.url, "each copy has a folder of its own")
        XCTAssertEqual(try Data(contentsOf: first.url), Data("frames".utf8))

        first.release()
        first.release()
        XCTAssertFalse(FileManager.default.fileExists(atPath: first.url.deletingLastPathComponent().path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: second.url.path), "releasing one copy leaves the other")
    }

    func testPickedFileThatCantBeReadLeavesNothingBehind() async throws {
        let inbox = FileManager.default.temporaryDirectory.appendingPathComponent("picked-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: inbox) }
        let missing = inbox.appendingPathComponent("gone.pdf")
        do {
            _ = try await ForgePickedFile.copy(of: missing, into: inbox)
            XCTFail("a missing file can't be copied")
        } catch {}
        let left = (try? FileManager.default.contentsOfDirectory(atPath: inbox.path)) ?? []
        XCTAssertEqual(left, [])
    }

    // MARK: Feedback and banners

    func testFeedbackPlaysEachTimeItIsAskedFor() {
        var feedback = ForgeFeedback()
        XCTAssertNil(feedback.kind)
        let before = feedback
        feedback.play(.success)
        XCTAssertEqual(feedback.kind, .success)
        XCTAssertNotEqual(feedback, before, "a change is what makes the view play it")
        let once = feedback
        feedback.play(.success)
        XCTAssertNotEqual(feedback, once, "the same kind twice plays twice")
        XCTAssertEqual(feedback.count, 2)
    }

    func testUndoBannerCarriesTheModelsAction() {
        enum Undo: Equatable { case restore(String) }
        let banner = ForgeBanner.undo("Deleted “Milk”", action: Undo.restore("Milk"), identifier: "list.undo")
        XCTAssertEqual(banner.button?.title, "Undo")
        XCTAssertEqual(banner.button?.action, .restore("Milk"))
        XCTAssertEqual(banner.button?.identifier, "list.undo")
        XCTAssertNotNil(banner.duration, "undo goes by itself")
        XCTAssertEqual(banner.announcement, "Deleted “Milk”. Undo is available.")
        XCTAssertFalse(banner.showsActivity)
    }

    func testWorkingBannerStaysAndUpdatesInPlace() {
        let banner = ForgeBanner<Never>.working("Sending 3 files")
        XCTAssertNil(banner.duration, "running work stays until the model clears it")
        XCTAssertTrue(banner.showsActivity)
        let later = banner.updated(message: "Sending 2 of 3", progress: 1.4)
        XCTAssertEqual(later.id, banner.id, "an update isn't shown as a new banner")
        XCTAssertEqual(later.progress, 1, "progress is clamped")
        XCTAssertEqual(later.message, "Sending 2 of 3")
        XCTAssertFalse(later.showsActivity, "measured progress shows a bar, not a spinner")
        XCTAssertEqual(ForgeBanner<Never>.message("Saved to Files").announcement, "Saved to Files")
    }
}
