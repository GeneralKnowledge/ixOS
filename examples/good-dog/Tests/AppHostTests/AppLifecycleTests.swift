import GoodDogApp
@preconcurrency import GoodDogFFI
import SwiftUI
import XCTest

/// SwiftUI's scene phases, as the model sees them. Needs SwiftUI, so it runs
/// on macOS (`forge test`) and not in `forge check swift` on Linux.
final class AppLifecycleTests: XCTestCase {
    func testScenePhasesMapOntoAppPhases() {
        XCTAssertEqual(AppPhase(.active), .active)
        XCTAssertEqual(AppPhase(.inactive), .inactive)
        XCTAssertEqual(AppPhase(.background), .background)
    }
}
