// Layered Navigation (docs/forgeappkit.md, docs/design/): a screen is a
// surface for one thing (an artist, a book, a conversation), not for the way
// it was reached, and a thing already open is gone back to, never opened a
// second time. Foundation only, so the rule is tested on Linux; a stack
// applies it with `NavigationStack(path: $path.forgeLayered())`.
//
// First used by Soundscape (November 2026); every app with pushed pages
// follows it (scripts/check-app-parity.sh, rule `layered-navigation`).

import Foundation

/// How a destination is shown.
public enum ForgeSurfaceStyle: Equatable, Sendable {
    /// A major area or a list (Albums, Settings): an ordinary push.
    case fullScreen
    /// A thing (an artist, an album, a trip): it grows out of the tile or row
    /// it was opened from, and the surface under it stays where it was.
    case stacked
}

/// A route in a navigation stack that knows which thing it shows.
///
///     enum Route: ForgeSurfaceRoute {
///         case artist(id: String)
///         case search(String)
///         var surfaceID: String? {
///             switch self {
///             case .artist(let id): "artist:\(id)"
///             case .search: nil   // a new search is a new screen
///             }
///         }
///     }
///
/// The identity names the entity, never the way it was reached: an artist
/// opened from a song and from a tile is `artist:<id>` both times, so resolve
/// a song to its artist before pushing rather than giving it its own identity.
public protocol ForgeSurfaceRoute: Hashable {
    /// The thing the screen shows, the same however it was reached; nil for
    /// a screen that is new each time (a search, a form).
    var surfaceID: String? { get }
    /// How the screen comes in. Defaults to an ordinary push.
    var surfaceStyle: ForgeSurfaceStyle { get }
}

extension ForgeSurfaceRoute {
    public var surfaceStyle: ForgeSurfaceStyle { .fullScreen }
}

public enum ForgeLayeredPath {
    /// The path a stack should hold after a change: when a surface comes up
    /// whose identity is already further down, the stack goes back to that
    /// one (keeping its scroll position and state) instead of adding a copy.
    /// Routes with no identity are always added.
    ///
    ///     [Artist A, Album X] + Artist A  ->  [Artist A]
    ///     [Artist A, Album X] + Album Y   ->  [Artist A, Album X, Album Y]
    public static func settle<Route>(_ path: [Route], identity: (Route) -> String?) -> [Route] {
        var settled: [Route] = []
        var places: [String: Int] = [:]
        for route in path {
            guard let id = identity(route) else {
                settled.append(route)
                continue
            }
            if let at = places[id] {
                // Back to the one already open: what was above it closes.
                settled.removeSubrange((at + 1)...)
                places = places.filter { $0.value <= at }
                continue
            }
            places[id] = settled.count
            settled.append(route)
        }
        return settled
    }

    /// `settle` with each route's own `surfaceID`.
    public static func settle<Route: ForgeSurfaceRoute>(_ path: [Route]) -> [Route] {
        settle(path, identity: \.surfaceID)
    }
}

extension Array where Element: ForgeSurfaceRoute {
    /// Opens `route` on this path the layered way: on top when it is new,
    /// back to it when it is already open. For pushes made in code (Now
    /// Playing's Go to Artist, a deep link); links go through the stack's
    /// `forgeLayered()` path.
    public mutating func forgeOpen(_ route: Element) {
        self = ForgeLayeredPath.settle(self + [route])
    }
}
