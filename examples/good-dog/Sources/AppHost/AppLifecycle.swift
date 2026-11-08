@preconcurrency import ForgeFFI
import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

// Reports the iOS lifecycle to Rust. The rules for what each phase means
// (which work stops in the background, what to refresh on resume) live in
// `rust/src/lib.rs` (`AppCore::set_phase`), not here.

extension AppPhase {
    /// SwiftUI's scene phase in the app's terms.
    public init(_ scenePhase: ScenePhase) {
        switch scenePhase {
        case .active: self = .active
        case .inactive: self = .inactive
        case .background: self = .background
        @unknown default: self = .inactive
        }
    }
}

private struct ForgeLifecycle: ViewModifier {
    @Environment(\.scenePhase) private var scenePhase
    let report: @MainActor (AppPhase) -> Void

    func body(content: Content) -> some View {
        content
            // `initial: true` reports the phase the view appears in, so Rust
            // leaves `Launching` without waiting for the first change.
            .onChange(of: scenePhase, initial: true) { _, phase in
                report(AppPhase(phase))
            }
            #if canImport(UIKit)
            // iOS sends this only when it ends a running (not suspended) app,
            // so nothing may depend on it: every command already saved.
            .onReceive(
                NotificationCenter.default.publisher(for: UIApplication.willTerminateNotification)
            ) { _ in
                report(.terminated)
            }
            #endif
    }
}

extension View {
    /// Calls `report` with each lifecycle phase of the scene this view is in:
    /// the phase it appears in, every change after that, and `.terminated`
    /// when iOS says the app is ending. Attach it once, near the root:
    ///
    ///     ContentView().forgeLifecycle { model.setPhase($0) }
    public func forgeLifecycle(_ report: @escaping @MainActor (AppPhase) -> Void) -> some View {
        modifier(ForgeLifecycle(report: report))
    }
}
