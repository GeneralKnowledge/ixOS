#if canImport(UIKit)
@preconcurrency import ForgeApp
import UIKit

/// Reports the *application* lifecycle (not a scene's) to a ForgeHost, from
/// UIApplication notifications. Call once from the App's init.
@MainActor
public enum ForgeLifecycle {
    private static var observers: [NSObjectProtocol] = []

    public static func attach<Core: ForgeCore>(_ host: ForgeHost<Core>) {
        let center = NotificationCenter.default
        let pairs: [(Notification.Name, AppPhase)] = [
            (UIApplication.didBecomeActiveNotification, .active),
            (UIApplication.willResignActiveNotification, .inactive),
            (UIApplication.didEnterBackgroundNotification, .background),
            (UIApplication.willTerminateNotification, .terminated),
        ]
        for (name, phase) in pairs {
            observers.append(center.addObserver(forName: name, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { host.report(phase) }
            })
        }
        // The state at attach time: SwiftUI apps start inactive, then active.
        if UIApplication.shared.applicationState == .active { host.report(.active) }
    }
}
#endif
