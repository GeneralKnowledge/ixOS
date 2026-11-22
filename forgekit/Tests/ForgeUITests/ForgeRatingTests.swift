@testable import ForgeUI
import XCTest

/// The rules `ForgeRating` draws.
final class ForgeRatingTests: XCTestCase {
    func testTappingAStarSetsItAndTappingItAgainClears() {
        let rated = ForgeRatingState(value: 3)
        XCTAssertEqual(rated.tapping(4), 4)
        XCTAssertEqual(rated.tapping(1), 1)
        XCTAssertNil(rated.tapping(3))
        XCTAssertEqual(ForgeRatingState(value: 3, allowsClearing: false).tapping(3), 3)
        XCTAssertEqual(ForgeRatingState(value: nil).tapping(2), 2)
    }

    func testVoiceOverSwipesStayInRange() {
        XCTAssertEqual(ForgeRatingState(value: nil).incremented, 1)
        XCTAssertEqual(ForgeRatingState(value: 5).incremented, 5)
        XCTAssertEqual(ForgeRatingState(value: 3).decremented, 2)
        XCTAssertNil(ForgeRatingState(value: 1).decremented)
        XCTAssertEqual(ForgeRatingState(value: 1, allowsClearing: false).decremented, 1)
        XCTAssertNil(ForgeRatingState(value: nil).decremented)
    }

    func testVoiceOverValue() {
        XCTAssertEqual(ForgeRatingState(value: 3).spokenValue, "3 of 5 stars")
        XCTAssertEqual(ForgeRatingState(value: nil).spokenValue, "Not rated")
        XCTAssertEqual(ForgeRatingState(value: 2, maximum: 3).spokenValue, "2 of 3 stars")
    }

    func testOutOfRangeValuesDrawAsUnrated() {
        XCTAssertNil(ForgeRatingState(value: 0).value)
        XCTAssertNil(ForgeRatingState(value: 9).value)
        let state = ForgeRatingState(value: 2)
        XCTAssertTrue(state.isFilled(2))
        XCTAssertFalse(state.isFilled(3))
    }
}
