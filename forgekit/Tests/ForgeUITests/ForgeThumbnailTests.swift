@testable import ForgeUI
import XCTest

/// The numbers `ForgeThumbnail` draws with.
final class ForgeThumbnailTests: XCTestCase {
    func testAFixedWidthGetsItsHeightFromTheAspectRatio() {
        let cover = ForgeThumbnailGeometry(width: 44, aspectRatio: 2.0 / 3.0, shape: .rounded)
        XCTAssertEqual(cover.height ?? 0, 66, accuracy: 0.001)
        XCTAssertNil(ForgeThumbnailGeometry(width: nil, aspectRatio: 0.75, shape: .rounded).height)
    }

    func testABadAspectRatioFallsBackToSquare() {
        XCTAssertEqual(ForgeThumbnailGeometry(width: 40, aspectRatio: 0, shape: .rounded).height, 40)
    }

    func testRoundedCornersGrowWithTheWidthUpToTheCardRadius() {
        let theme = ForgeRadius.standard
        func radius(_ width: Double?, _ shape: ForgeThumbnailShape = .rounded) -> Double {
            ForgeThumbnailGeometry(width: width, aspectRatio: 1, shape: shape)
                .cornerRadius(medium: theme.m, large: theme.l)
        }
        XCTAssertEqual(radius(44), 4.4, accuracy: 0.001)
        XCTAssertEqual(radius(300), theme.l)
        XCTAssertEqual(radius(nil), theme.m)
        XCTAssertEqual(radius(44, .circle), 0)
        XCTAssertEqual(radius(44, .square), 0)
    }

    func testOnlyLoadingShowsASpinner() {
        XCTAssertTrue(ForgeThumbnailPhase.loading.showsSpinner)
        XCTAssertFalse(ForgeThumbnailPhase.empty.showsSpinner)
        XCTAssertFalse(ForgeThumbnailPhase.failed.showsSpinner)
        XCTAssertEqual(ForgeThumbnailGeometry.symbolSize(width: 44, height: 66), 17.6, accuracy: 0.001)
    }
}
