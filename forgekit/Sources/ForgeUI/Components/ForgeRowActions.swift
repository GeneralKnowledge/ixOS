#if canImport(SwiftUI)
import SwiftUI

extension View {
    /// A row's actions, as swipe actions and as the menu a long press opens.
    /// A swipe is hidden: someone who doesn't know to swipe, or uses Voice
    /// Control or a pointer, finds the menu instead. One list of buttons
    /// gives both, so they can't drift apart.
    ///
    ///     TripRow(trip: trip)
    ///         .forgeRowActions {
    ///             Button("Delete", systemImage: "trash", role: .destructive) { model.askToDelete(trip) }
    ///                 .accessibilityIdentifier("trips.delete")
    ///         }
    ///
    /// The trailing actions are the ones a swipe from the right shows (Delete
    /// goes here); the menu lists them after the leading ones.
    public func forgeRowActions<Trailing: View>(
        @ViewBuilder _ trailing: @escaping () -> Trailing
    ) -> some View {
        swipeActions(edge: .trailing) { trailing() }
            .contextMenu { trailing() }
    }

    /// Row actions on both edges: `leading` for a swipe from the left (Mark
    /// as Read, Done), `trailing` for a swipe from the right (Delete). The
    /// menu lists the leading ones first.
    public func forgeRowActions<Leading: View, Trailing: View>(
        @ViewBuilder leading: @escaping () -> Leading,
        @ViewBuilder trailing: @escaping () -> Trailing
    ) -> some View {
        swipeActions(edge: .leading) { leading() }
            .swipeActions(edge: .trailing) { trailing() }
            .contextMenu {
                leading()
                trailing()
            }
    }
}
#endif
