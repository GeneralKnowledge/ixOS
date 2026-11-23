@testable import ForgeUI
import XCTest

/// The rules ForgeUI's choice style draws.
final class ForgeChoiceTests: XCTestCase {
    func testAChosenChoiceDiffersByMoreThanColour() {
        let chosen = ForgeChoiceLook(isSelected: true, shape: .chip)
        XCTAssertTrue(chosen.usesToneFill)
        XCTAssertTrue(chosen.showsOutline)
        XCTAssertTrue(chosen.isBold)
        XCTAssertTrue(chosen.showsCheckmark)

        let other = ForgeChoiceLook(isSelected: false, shape: .chip)
        XCTAssertFalse(other.usesToneFill || other.showsOutline || other.isBold || other.showsCheckmark)
    }

    func testOnlyChipsGetACheckmark() {
        XCTAssertFalse(ForgeChoiceLook(isSelected: true, shape: .tile).showsCheckmark)
        XCTAssertFalse(ForgeChoiceLook(isSelected: true, shape: .circle).showsCheckmark)
    }

    func testEveryChoiceIsBigEnoughToTap() {
        XCTAssertGreaterThanOrEqual(ForgeChoiceLook.minimumTapSize, 44)
    }
}
