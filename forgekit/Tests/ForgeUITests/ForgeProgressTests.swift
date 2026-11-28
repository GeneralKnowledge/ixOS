@testable import ForgeUI
import XCTest

/// The numbers ForgeUI's progress style draws with.
final class ForgeProgressTests: XCTestCase {
    func testAFractionIsDrawnInRange() {
        XCTAssertEqual(ForgeProgress.clamped(0.4), 0.4)
        XCTAssertEqual(ForgeProgress.clamped(1.3), 1)
        XCTAssertEqual(ForgeProgress.clamped(-0.2), 0)
    }

    func testNoFractionIsIndeterminate() {
        XCTAssertNil(ForgeProgress.clamped(nil))
        XCTAssertNil(ForgeProgress.clamped(.nan))
    }
}
