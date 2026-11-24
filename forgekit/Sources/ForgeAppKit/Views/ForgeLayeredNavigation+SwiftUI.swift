#if canImport(SwiftUI)
import SwiftUI

// Layered Navigation in a SwiftUI stack (ForgeLayeredPath.swift). One tab's
// stack, its pushed pages, and the tiles they grow out of:
//
//     @State private var path: [Route] = []
//     @Namespace private var surfaces
//
//     NavigationStack(path: $path.forgeLayered()) {
//         List(artists) { artist in
//             NavigationLink(value: Route.artist(id: artist.id)) { ArtistRow(artist) }
//                 .forgeSurfaceSource(Route.artist(id: artist.id))
//         }
//         .forgeLayeredDestination(for: Route.self) { route in RouteView(route) }
//     }
//     .forgeSurfaces(surfaces, path: $path)
//
// The rule sits on the path, so it holds however a push happens: a link, a
// button deeper in the stack (`@Environment(\.forgeOpenSurface)`), a menu's
// Go to Artist (`path.forgeOpen(route)`), or a deep link. Pushing beside the
// path (`navigationDestination(item:)` on a page) would skip it.

extension Binding {
    /// The stack's path with Layered Navigation applied to every change: a
    /// surface already open is gone back to, not pushed again.
    public func forgeLayered<Route: ForgeSurfaceRoute>() -> Binding<[Route]> where Value == [Route] {
        Binding<[Route]>(
            get: { wrappedValue },
            set: { wrappedValue = ForgeLayeredPath.settle($0) }
        )
    }
}

private struct ForgeSurfaceNamespaceKey: EnvironmentKey {
    static let defaultValue: Namespace.ID? = nil
}

/// Opens a surface on the enclosing layered stack, the layered way: on top
/// when it is new, back to it when it is already open. A page deeper in the
/// stack uses it instead of pushing a destination of its own:
///
///     @Environment(\.forgeOpenSurface) private var openSurface
///     Button("Details") { openSurface(Route.track(id)) }
public struct ForgeOpenSurfaceAction {
    let open: ((AnyHashable) -> Bool)?

    public func callAsFunction<Route: ForgeSurfaceRoute>(_ route: Route) {
        // A page outside a layered stack, or a route of another type, is a
        // programming mistake: the stack's `forgeSurfaces(_:path:)` is missing.
        let opened = open?(AnyHashable(route)) ?? false
        assert(opened, "forgeOpenSurface: no layered stack for \(Route.self); add .forgeSurfaces(_:path:) to the NavigationStack")
    }
}

/// A tile or a name that opens a surface: a plain button calling
/// `forgeOpenSurface`. Use it wherever one `List` row holds more than one
/// link (a grid of tiles, a shelf, "Artist · Album" under a title). A
/// `List` row with `NavigationLink`s in it pushes every one of them on a
/// single tap, whatever their button style, so a grid of genres in a row
/// opened all twenty; plain buttons are tapped one at a time.
///
///     ForgeSurfaceLink(Route.genre(genre)) { GenreTile(genre) }
///         .accessibilityIdentifier("search.genre")
///
/// A row that is one link as a whole (a song, an album in a list) keeps
/// `NavigationLink(value:)`, which draws the row's disclosure.
public struct ForgeSurfaceLink<Route: ForgeSurfaceRoute, Label: View>: View {
    private let route: Route
    private let label: Label
    @Environment(\.forgeOpenSurface) private var openSurface

    public init(_ route: Route, @ViewBuilder label: () -> Label) {
        self.route = route
        self.label = label()
    }

    public var body: some View {
        Button { openSurface(route) } label: { label }
            .buttonStyle(.plain)
    }
}

extension ForgeSurfaceLink where Label == Text {
    public init(_ title: LocalizedStringKey, route: Route) {
        self.init(route) { Text(title) }
    }

    @_disfavoredOverload
    public init<S: StringProtocol>(_ title: S, route: Route) {
        self.init(route) { Text(title) }
    }
}

private struct ForgeOpenSurfaceKey: EnvironmentKey {
    static let defaultValue = ForgeOpenSurfaceAction(open: nil)
}

extension EnvironmentValues {
    /// The stack's namespace that stacked surfaces grow out of their tiles in.
    public var forgeSurfaceNamespace: Namespace.ID? {
        get { self[ForgeSurfaceNamespaceKey.self] }
        set { self[ForgeSurfaceNamespaceKey.self] = newValue }
    }

    /// Opens a route on the enclosing layered stack (`ForgeOpenSurfaceAction`).
    public var forgeOpenSurface: ForgeOpenSurfaceAction {
        get { self[ForgeOpenSurfaceKey.self] }
        set { self[ForgeOpenSurfaceKey.self] = newValue }
    }
}

extension View {
    /// Gives the stack (and every page it pushes) the namespace that stacked
    /// surfaces zoom out of, and `forgeOpenSurface` for opening a route on
    /// `path` from anywhere in it. Attach to the `NavigationStack`.
    public func forgeSurfaces<Route: ForgeSurfaceRoute>(
        _ namespace: Namespace.ID,
        path: Binding<[Route]>
    ) -> some View {
        environment(\.forgeSurfaceNamespace, namespace)
            .environment(\.forgeOpenSurface, ForgeOpenSurfaceAction { value in
                guard let route = value.base as? Route else { return false }
                path.wrappedValue.forgeOpen(route)
                return true
            })
    }

    /// Marks a tile or row as where `route` grows from when it opens. Only
    /// stacked routes with an identity use it; others are unchanged.
    public func forgeSurfaceSource<Route: ForgeSurfaceRoute>(_ route: Route) -> some View {
        modifier(ForgeSurfaceSource(style: route.surfaceStyle, id: route.surfaceID))
    }

    /// The pages a stack pushes for `Route`, each coming in its own way: a
    /// stacked surface zooms out of its tile (iOS 18 and later, not with
    /// Reduce Motion), anything else is an ordinary push. Use with a path
    /// from `forgeLayered()`.
    public func forgeLayeredDestination<Route: ForgeSurfaceRoute, Destination: View>(
        for route: Route.Type,
        @ViewBuilder destination: @escaping (Route) -> Destination
    ) -> some View {
        navigationDestination(for: Route.self) { value in
            destination(value)
                .modifier(ForgeSurfaceTransition(style: value.surfaceStyle, id: value.surfaceID))
        }
    }
}

private struct ForgeSurfaceSource: ViewModifier {
    let style: ForgeSurfaceStyle
    let id: String?
    @Environment(\.forgeSurfaceNamespace) private var namespace

    @ViewBuilder func body(content: Content) -> some View {
        #if os(iOS)
        if #available(iOS 18.0, *), style == .stacked, let id, let namespace {
            content.matchedTransitionSource(id: id, in: namespace)
        } else {
            content
        }
        #else
        content
        #endif
    }
}

/// A stacked surface zooms out of its tile, and back into it when closed,
/// the surface under it showing as it goes; Reduce Motion keeps the ordinary
/// push.
private struct ForgeSurfaceTransition: ViewModifier {
    let style: ForgeSurfaceStyle
    let id: String?
    @Environment(\.forgeSurfaceNamespace) private var namespace
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    @ViewBuilder func body(content: Content) -> some View {
        #if os(iOS)
        if #available(iOS 18.0, *), style == .stacked, !reduceMotion, let id, let namespace {
            content.navigationTransition(.zoom(sourceID: id, in: namespace))
        } else {
            content
        }
        #else
        content
        #endif
    }
}
#endif
