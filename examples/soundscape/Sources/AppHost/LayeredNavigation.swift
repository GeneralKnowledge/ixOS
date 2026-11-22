// Layered Navigation (docs/design.md): a surface already open is returned
// to, not opened again. Foundation only, so the rule is tested on Linux;
// each tab's stack applies it to its path (ListenView.swift).

import Foundation

/// How a destination is shown.
enum SurfaceStyle: Equatable, Sendable {
    /// A major area or a list (Albums, Playlists): an ordinary push.
    case fullScreen
    /// A thing (an artist, an album, a playlist): it grows out of the tile
    /// it was opened from, and the surface under it stays where it was.
    case stacked
}

enum LayeredPath {
    /// The path a stack should hold after a change: when a surface comes
    /// up whose identity is already further down, the stack goes back to
    /// that one (keeping its scroll position and state) instead of adding
    /// a copy. Routes with no identity are always added.
    ///
    ///     [Artist A, Album X] + Artist A  ->  [Artist A]
    ///     [Artist A, Album X] + Album Y   ->  [Artist A, Album X, Album Y]
    static func settle<Route>(_ path: [Route], identity: (Route) -> String?) -> [Route] {
        var settled: [Route] = []
        var places: [String: Int] = [:]
        for route in path {
            if let id = identity(route), let at = places[id] {
                // Back to the one already open: what was above it closes.
                settled.removeSubrange((at + 1)...)
                places = places.filter { $0.value <= at }
                continue
            }
            if let id = identity(route) { places[id] = settled.count }
            settled.append(route)
        }
        return settled
    }
}
