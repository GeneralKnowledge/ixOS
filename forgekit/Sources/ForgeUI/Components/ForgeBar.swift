#if canImport(SwiftUI)
import SwiftUI

extension View {
    /// Floats `bar` over this view's bottom (or top) edge, and gives every
    /// scroll view below it that much room, so a list's last row can always
    /// scroll clear of the bar. Put it on the container (a `NavigationStack`,
    /// or a tab's content): the room reaches the screens pushed onto the
    /// stack too, which a `.safeAreaInset` on the stack does not reliably do
    /// (Soundscape's mini-player hid a pushed album's last song,
    /// docs/research/screen-space.md).
    ///
    ///     NavigationStack { … }
    ///         .forgeBar { if player.current != nil { MiniPlayer(player: player) } }
    ///
    /// The room is the bar's measured height, 0 while the bar is empty. A
    /// horizontal scroller inside a list gets the same room under it while
    /// the bar shows; give it `.contentMargins(.bottom, 0, for: .scrollContent)`
    /// if that shows.
    public func forgeBar<Bar: View>(
        edge: VerticalEdge = .bottom,
        @ViewBuilder _ bar: () -> Bar
    ) -> some View {
        modifier(ForgeBarModifier(edge: edge, bar: bar()))
    }
}

private struct ForgeBarModifier<Bar: View>: ViewModifier {
    let edge: VerticalEdge
    let bar: Bar
    @State private var room: CGFloat = 0

    func body(content: Content) -> some View {
        let edges: Edge.Set = edge == .bottom ? .bottom : .top
        content
            // Content margins pass down through the environment, to the
            // stack's pushed screens as well as its first one.
            .contentMargins(edges, room, for: .scrollContent)
            .contentMargins(edges, room, for: .scrollIndicators)
            .overlay(alignment: edge == .bottom ? .bottom : .top) {
                // A stack that is always there, so an empty bar measures 0
                // and gives the room back.
                VStack(spacing: 0) { bar }
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { room = $0 }
            }
    }
}
#endif
