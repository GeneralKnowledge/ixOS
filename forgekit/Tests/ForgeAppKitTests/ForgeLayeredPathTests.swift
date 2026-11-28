import ForgeAppKit
import XCTest

/// Layered Navigation's rule (ForgeLayeredPath.swift): an open surface is
/// gone back to, never opened twice, whichever way it was reached.
final class ForgeLayeredPathTests: XCTestCase {
    enum Route: ForgeSurfaceRoute {
        case artist(String)
        case album(String)
        case search(String)

        var surfaceID: String? {
            switch self {
            case .artist(let id): "artist:\(id)"
            case .album(let id): "album:\(id)"
            case .search: nil
            }
        }

        var surfaceStyle: ForgeSurfaceStyle {
            if case .search = self { .fullScreen } else { .stacked }
        }
    }

    func testOpeningAnOpenSurfaceGoesBackToIt() {
        XCTAssertEqual(ForgeLayeredPath.settle([.artist("a"), .album("x"), .artist("a")] as [Route]), [.artist("a")])
        XCTAssertEqual(
            ForgeLayeredPath.settle([.search("q"), .album("x"), .artist("a"), .album("x")] as [Route]),
            [.search("q"), .album("x")]
        )
    }

    func testNewSurfacesStack() {
        XCTAssertEqual(
            ForgeLayeredPath.settle([.artist("a"), .album("x"), .album("y")] as [Route]),
            [.artist("a"), .album("x"), .album("y")]
        )
        XCTAssertEqual(ForgeLayeredPath.settle([] as [Route]), [])
    }

    func testTheSameSurfaceTwiceInARowIsOne() {
        XCTAssertEqual(ForgeLayeredPath.settle([.album("x"), .album("x"), .album("x")] as [Route]), [.album("x")])
    }

    func testRoutesWithNoIdentityAlwaysStack() {
        XCTAssertEqual(ForgeLayeredPath.settle([.search("q"), .search("q")] as [Route]), [.search("q"), .search("q")])
    }

    func testIdentitiesAboveTheOneReturnedToAreForgotten() {
        // Back to A closes X; opening X again is a new surface on A.
        XCTAssertEqual(
            ForgeLayeredPath.settle([.artist("a"), .album("x"), .artist("a"), .album("x")] as [Route]),
            [.artist("a"), .album("x")]
        )
    }

    func testOpenInCodeFollowsTheSameRule() {
        var path: [Route] = [.artist("a"), .album("x")]
        path.forgeOpen(.album("y"))
        XCTAssertEqual(path, [.artist("a"), .album("x"), .album("y")])
        // Artist -> release -> artist reveals the first artist page.
        path.forgeOpen(.artist("a"))
        XCTAssertEqual(path, [.artist("a")])
    }

    func testCustomIdentityClosure() {
        let settled = ForgeLayeredPath.settle(["artist:a", "list", "album:x", "artist:a"]) { $0 == "list" ? nil : $0 }
        XCTAssertEqual(settled, ["artist:a"])
    }

    func testStyleDefaultsToAnOrdinaryPush() {
        struct Plain: ForgeSurfaceRoute { var surfaceID: String? { nil } }
        XCTAssertEqual(Plain().surfaceStyle, .fullScreen)
        XCTAssertEqual(Route.artist("a").surfaceStyle, .stacked)
    }
}
