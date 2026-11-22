@testable import ForgeUI
import XCTest

/// The rules `ForgeButton` and `forgeButtonLook` draw.
final class ForgeButtonTests: XCTestCase {
    func testAnIdleButtonRunsItsActionAndIsEnabled() {
        let state = ForgeButtonState(isWorking: false)
        XCTAssertTrue(state.runsAction)
        XCTAssertFalse(state.isDisabled)
        XCTAssertTrue(state.fadesWhenDisabled)
        XCTAssertFalse(state.showsSpinner)
        XCTAssertFalse(state.announcesProgress)
    }

    func testAWorkingButtonIsDisabledButNotFaded() {
        let state = ForgeButtonState(isWorking: true)
        XCTAssertFalse(state.runsAction)
        XCTAssertTrue(state.isDisabled)
        XCTAssertFalse(state.fadesWhenDisabled)
        XCTAssertTrue(state.showsSpinner)
        XCTAssertTrue(state.announcesProgress)
    }

    func testAsyncWorkIsDisabledAtOnceButSilentUntilTheSpinnerIsDue() {
        let early = ForgeButtonState(isWorking: true, showsSpinner: false)
        XCTAssertFalse(early.runsAction)
        XCTAssertTrue(early.isDisabled)
        XCTAssertFalse(early.fadesWhenDisabled)
        XCTAssertFalse(early.showsSpinner)
        XCTAssertFalse(early.announcesProgress)

        let late = ForgeButtonState(isWorking: true, showsSpinner: true)
        XCTAssertTrue(late.showsSpinner)
        XCTAssertTrue(late.announcesProgress)
    }

    func testTheSpinnerIsNeverShownWithoutWork() {
        XCTAssertFalse(ForgeButtonState(isWorking: false, showsSpinner: true).showsSpinner)
    }

    func testTheSpinnerDelayIsShortEnoughToFeelImmediate() {
        XCTAssertEqual(ForgeButtonState.spinnerDelay, .milliseconds(400))
    }

    private func look(
        enabled: Bool = true, working: Bool = false, pressed: Bool = false, reduceMotion: Bool = false
    ) -> ForgeButtonLook {
        ForgeButtonLook(
            isEnabled: enabled, isWorking: working, isPressed: pressed,
            isCompact: false, reduceMotion: reduceMotion
        )
    }

    func testALookIsFullColourAtRestAndDimsWhenPressed() {
        XCTAssertEqual(look().opacity, 1)
        XCTAssertEqual(look(pressed: true).opacity, 0.7)
        XCTAssertEqual(look(pressed: true).scale, 0.98)
    }

    func testADisabledLookFadesButAWorkingOneDoesNot() {
        XCTAssertEqual(look(enabled: false).opacity, 0.4)
        XCTAssertEqual(look(enabled: false, working: true).opacity, 1)
    }

    func testReduceMotionKeepsThePressStill() {
        let still = look(pressed: true, reduceMotion: true)
        XCTAssertEqual(still.scale, 1)
        XCTAssertFalse(still.animatesPress)
        XCTAssertEqual(still.opacity, 0.7)
    }
}
