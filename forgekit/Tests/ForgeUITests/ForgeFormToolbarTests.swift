@testable import ForgeUI
import XCTest

/// The rules `forgeFormToolbar` draws: when Cancel and the confirm button
/// work, and when a swipe down may close the sheet.
final class ForgeFormToolbarTests: XCTestCase {
    func testAnUntouchedFormCanBeSwipedAway() {
        let state = ForgeFormToolbarState(canConfirm: false, isWorking: false, hasChanges: false)
        XCTAssertFalse(state.blocksSwipeToDismiss)
        XCTAssertTrue(state.cancelEnabled)
        XCTAssertFalse(state.confirmEnabled)
    }

    func testUnsavedChangesStopTheSwipeButNotCancel() {
        let state = ForgeFormToolbarState(canConfirm: true, isWorking: false, hasChanges: true)
        XCTAssertTrue(state.blocksSwipeToDismiss)
        XCTAssertTrue(state.cancelEnabled)
        XCTAssertTrue(state.confirmEnabled)
        XCTAssertFalse(state.showsSpinner)
    }

    func testSavingStopsTheSwipeAndCancelAndShowsTheSpinner() {
        let state = ForgeFormToolbarState(canConfirm: true, isWorking: true, hasChanges: false)
        XCTAssertTrue(state.blocksSwipeToDismiss)
        XCTAssertFalse(state.cancelEnabled)
        XCTAssertFalse(state.confirmEnabled)
        XCTAssertTrue(state.showsSpinner)
    }

    /// Callers that don't pass `hasChanges` keep the old behaviour: only
    /// saving stops the swipe.
    func testWithoutChangesOnlySavingStopsTheSwipe() {
        XCTAssertFalse(ForgeFormToolbarState(canConfirm: true, isWorking: false, hasChanges: false).blocksSwipeToDismiss)
        XCTAssertTrue(ForgeFormToolbarState(canConfirm: true, isWorking: true, hasChanges: false).blocksSwipeToDismiss)
    }
}
