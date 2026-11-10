#if canImport(SwiftUI)
import SwiftUI

// Destinations as one optional enum the model owns, instead of a Bool per
// sheet. The model says where the screen is going; a Linux test checks it:
//
//     enum Destination: Equatable {
//         case add
//         case edit(Book)
//         var edit: Book? { if case .edit(let book) = self { book } else { nil } }
//     }
//     public var destination: Destination?
//
// and the view presents each case:
//
//     .forgeSheet($model.destination, equals: .add) { AddBookView() }
//     .forgeSheet($model.destination, \.edit) { book in EditBookView(book: book) }
//
// Closing the sheet (Cancel, swipe down, `dismiss()`) sets the destination
// back to nil.

extension Binding {
    /// A binding to one case of an optional enum: it reads the case's value
    /// (nil for other cases), and writing nil clears the whole destination.
    /// Writing a value is ignored; the model sets destinations.
    public func forgeCase<Destination, Case>(
        _ extract: KeyPath<Destination, Case?>
    ) -> Binding<Case?> where Value == Destination? {
        Binding<Case?>(
            get: { wrappedValue?[keyPath: extract] },
            set: { if $0 == nil, wrappedValue?[keyPath: extract] != nil { wrappedValue = nil } }
        )
    }

    /// True while the destination is `value`; setting false clears it.
    public func forgeIs<Destination: Equatable>(_ value: Destination) -> Binding<Bool> where Value == Destination? {
        Binding<Bool>(
            get: { wrappedValue == value },
            set: { if !$0, wrappedValue == value { wrappedValue = nil } }
        )
    }
}

extension View {
    /// A sheet for the destination case `extract` picks out; its value is
    /// passed to `content`.
    public func forgeSheet<Destination, Case: Identifiable, Content: View>(
        _ destination: Binding<Destination?>,
        _ extract: KeyPath<Destination, Case?>,
        onDismiss: (() -> Void)? = nil,
        @ViewBuilder content: @escaping (Case) -> Content
    ) -> some View {
        sheet(item: destination.forgeCase(extract), onDismiss: onDismiss, content: content)
    }

    /// A sheet while the destination equals `value` (a case with no data).
    public func forgeSheet<Destination: Equatable, Content: View>(
        _ destination: Binding<Destination?>,
        equals value: Destination,
        onDismiss: (() -> Void)? = nil,
        @ViewBuilder content: @escaping () -> Content
    ) -> some View {
        sheet(isPresented: destination.forgeIs(value), onDismiss: onDismiss, content: content)
    }

    #if os(iOS)
    /// A full-screen cover for the destination case `extract` picks out.
    public func forgeFullScreenCover<Destination, Case: Identifiable, Content: View>(
        _ destination: Binding<Destination?>,
        _ extract: KeyPath<Destination, Case?>,
        onDismiss: (() -> Void)? = nil,
        @ViewBuilder content: @escaping (Case) -> Content
    ) -> some View {
        fullScreenCover(item: destination.forgeCase(extract), onDismiss: onDismiss, content: content)
    }
    #endif

    /// Pushes a screen onto the enclosing `NavigationStack` for the
    /// destination case `extract` picks out; going back clears it.
    public func forgeNavigationDestination<Destination, Case: Hashable, Content: View>(
        _ destination: Binding<Destination?>,
        _ extract: KeyPath<Destination, Case?>,
        @ViewBuilder content: @escaping (Case) -> Content
    ) -> some View {
        navigationDestination(item: destination.forgeCase(extract), destination: content)
    }
}
#endif
