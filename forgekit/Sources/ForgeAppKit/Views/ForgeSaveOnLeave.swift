#if canImport(SwiftUI)
import SwiftUI

extension View {
    /// Saves what the screen holds when the person leaves it: the screen
    /// closing, and also the app leaving the screen. `onDisappear` alone
    /// misses the second: going Home doesn't make a view disappear, and iOS
    /// may then end the app without warning, losing whatever was typed.
    ///
    ///     TextEditor(text: $text)
    ///         .forgeSaveOnLeave { save() }
    ///
    /// `save` may run more than once (leaving the app, then the screen), so
    /// make it do nothing when nothing changed.
    public func forgeSaveOnLeave(_ save: @escaping () -> Void) -> some View {
        modifier(ForgeSaveOnLeave(save: save))
    }
}

private struct ForgeSaveOnLeave: ViewModifier {
    let save: () -> Void
    @Environment(\.scenePhase) private var scenePhase

    func body(content: Content) -> some View {
        content
            .onDisappear(perform: save)
            .onChange(of: scenePhase) { _, phase in
                if phase != .active { save() }
            }
    }
}
#endif
