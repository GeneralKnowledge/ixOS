@testable import ForgeUI
import XCTest

/// The rules `ForgeButton` draws while its work runs.
final class ForgeButtonTests: XCTestCase {
    func testAnIdleButtonRunsItsActionAndIsEnabled() {
        let state = ForgeButtonState(isWorking: false)
        XCTAssertTrue(state.runsAction)
        XCTAssertFalse(state.isDisabled)
        XCTAssertTrue(state.fadesWhenDisabled)
    }

    func testAWorkingButtonIsDisabledButNotFaded() {
        let state = ForgeButtonState(isWorking: true)
        XCTAssertFalse(state.runsAction)
        XCTAssertTrue(state.isDisabled)
        XCTAssertFalse(state.fadesWhenDisabled)
    }
}
