#if canImport(UIKit)
@preconcurrency import ForgeApp
import UIKit

/// Reports the *application* lifecycle (not a scene's) to a ForgeHost, from
/// UIApplication notifications. Call once from the App's init; calling it
/// again replaces the previous attachment instead of adding to it.
@MainActor
public enum ForgeLifecycle {
    private static var observers: [NSObjectProtocol] = []

    public static func attach<Core: ForgeCore>(_ host: ForgeHost<Core>) {
        detach()
        let center = NotificationCenter.default
        let pairs: [(Notification.Name, AppPhase)] = [
            (UIApplication.didBecomeActiveNotification, .active),
            (UIApplication.willResignActiveNotification, .inactive),
            (UIApplication.didEnterBackgroundNotification, .background),
            (UIApplication.willTerminateNotification, .terminated),
        ]
        for (name, phase) in pairs {
            observers.append(center.addObserver(forName: name, object: nil, queue: .main) { [weak host] _ in
                MainActor.assumeIsolated {
                    host?.report(phase)
                    if phase == .terminated { detach() }
                }
            })
        }
        // The state at attach time: SwiftUI apps start inactive, then active.
        if UIApplication.shared.applicationState == .active { host.report(.active) }
    }

    /// Stops reporting. `attach` and termination call it.
    public static func detach() {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        observers.removeAll()
    }
}
#endif
