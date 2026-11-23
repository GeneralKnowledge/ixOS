@testable import SoundscapeApp
import XCTest

/// Layered Navigation's rule (LayeredNavigation.swift): an open surface is
/// returned to, never opened twice.
final class LayeredNavigationTests: XCTestCase {
    private func settle(_ path: [String]) -> [String] {
        // "list" stands for a route with no identity.
        LayeredPath.settle(path) { $0 == "list" ? nil : $0 }
    }

    func testOpeningAnOpenSurfaceGoesBackToIt() {
        XCTAssertEqual(settle(["artist:a", "album:x", "artist:a"]), ["artist:a"])
        XCTAssertEqual(settle(["albums", "album:x", "artist:a", "album:x"]), ["albums", "album:x"])
    }

    func testNewSurfacesStack() {
        XCTAssertEqual(settle(["artist:a", "album:x", "album:y"]), ["artist:a", "album:x", "album:y"])
        XCTAssertEqual(settle([]), [])
    }

    func testTheSameSurfaceTwiceInARowIsOne() {
        XCTAssertEqual(settle(["album:x", "album:x", "album:x"]), ["album:x"])
    }

    func testRoutesWithNoIdentityAlwaysStack() {
        XCTAssertEqual(settle(["list", "list"]), ["list", "list"])
    }

    func testIdentitiesAboveTheOneReturnedToAreForgotten() {
        // Back to A closes X; opening X again is a new surface on A.
        XCTAssertEqual(settle(["artist:a", "album:x", "artist:a", "album:x"]), ["artist:a", "album:x"])
    }
}
