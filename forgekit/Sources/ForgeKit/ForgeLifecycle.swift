#if canImport(UIKit)
@preconcurrency import ForgeApp
import UIKit

/// Reports the *application* lifecycle (not a scene's) to ForgeHosts, from
/// UIApplication notifications. Call once per host from the App's init;
/// attaching the same host again replaces its earlier attachment. An app
/// that runs several cores (the Device Lab showcase) attaches each host.
@MainActor
public enum ForgeLifecycle {
    private static var observers: [ObjectIdentifier: [NSObjectProtocol]] = [:]

    /// Also gives the host UIKit's background time (`beginBackgroundTask`)
    /// for commands and finishing work still running when the app leaves
    /// the screen, unless it already has a `backgroundTime`.
    public static func attach<Core: ForgeCore>(_ host: ForgeHost<Core>) {
        let key = ObjectIdentifier(host)
        detach(key)
        if host.backgroundTime == nil { host.backgroundTime = ForgeUIKitBackgroundTime() }
        let center = NotificationCenter.default
        let pairs: [(Notification.Name, AppPhase)] = [
            (UIApplication.didBecomeActiveNotification, .active),
            (UIApplication.willResignActiveNotification, .inactive),
            (UIApplication.didEnterBackgroundNotification, .background),
            (UIApplication.willTerminateNotification, .terminated),
        ]
        var added: [NSObjectProtocol] = []
        for (name, phase) in pairs {
            added.append(center.addObserver(forName: name, object: nil, queue: .main) { [weak host] _ in
                MainActor.assumeIsolated {
                    host?.report(phase)
                    if phase == .terminated { detach(key) }
                }
            })
        }
        observers[key] = added
        // The state at attach time: SwiftUI apps start inactive, then active.
        if UIApplication.shared.applicationState == .active { host.report(.active) }
    }

    /// Stops reporting to one host.
    public static func detach<Core: ForgeCore>(_ host: ForgeHost<Core>) {
        detach(ObjectIdentifier(host))
    }

    /// Stops reporting to every host.
    public static func detach() {
        for key in Array(observers.keys) { detach(key) }
    }

    private static func detach(_ key: ObjectIdentifier) {
        for observer in observers.removeValue(forKey: key) ?? [] {
            NotificationCenter.default.removeObserver(observer)
        }
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
