#if canImport(UIKit)
@preconcurrency import ForgeApp
import UIKit

/// Reports the *application* lifecycle (not a scene's) to a ForgeHost, from
/// UIApplication notifications. Call once from the App's init; calling it
/// again replaces the previous attachment instead of adding to it.
@MainActor
public enum ForgeLifecycle {
    private static var observers: [NSObjectProtocol] = []

    /// Also gives the host UIKit's background time (`beginBackgroundTask`)
    /// for commands and finishing work still running when the app leaves
    /// the screen, unless it already has a `backgroundTime`.
    public static func attach<Core: ForgeCore>(_ host: ForgeHost<Core>) {
        detach()
        if host.backgroundTime == nil { host.backgroundTime = ForgeUIKitBackgroundTime() }
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
        switch UIApplication.shared.applicationState {
        case .active:
            host.report(.active)
        case .background:
            // iOS launched the app without a screen (a background task, a
            // finished background download, a location event). No view will
            // start the core, and that work needs it, so open it now.
            host.report(.background)
            Task { await host.start() }
        default:
            break
        }
    }

    /// Stops reporting. `attach` and termination call it.
    public static func detach() {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        observers.removeAll()
    }
}

/// Background time from UIKit: one `beginBackgroundTask` at a time, ended
/// when the work is done or the time runs out.
@MainActor
public final class ForgeUIKitBackgroundTime: ForgeBackgroundTime {
    private var task: UIBackgroundTaskIdentifier = .invalid

    public init() {}

    public func begin(onExpiry: @escaping @MainActor () -> Void) {
        guard task == .invalid else { return }
        // The expiration handler runs on the main thread.
        task = UIApplication.shared.beginBackgroundTask(withName: "Forge: finishing work") {
            MainActor.assumeIsolated { onExpiry() }
        }
    }

    public func end() {
        guard task != .invalid else { return }
        UIApplication.shared.endBackgroundTask(task)
        task = .invalid
    }
}
#endif
