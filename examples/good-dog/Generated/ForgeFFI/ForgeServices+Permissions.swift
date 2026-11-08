// Forge native services: permissions. Source of truth: crates/forge-services/swift/.

import Foundation
import UserNotifications

#if canImport(UIKit)
    import UIKit
#endif

/// One permission's status and request. Implement it to add a permission
/// (camera, photos, location, ...) and register it with
/// `ForgePermissions.shared.register(_:for:)`; see docs/native-services.md.
public protocol ForgePermissionAdapter: Sendable {
    func status() async -> PermissionStatus
    /// Shows the system prompt if the user has not been asked yet.
    func request() async -> PermissionStatus
}

/// `PermissionProvider` that dispatches to one adapter per permission.
/// Permissions with no adapter report `.unsupported`.
public final class ForgePermissions: PermissionProvider, @unchecked Sendable {
    public static let shared = ForgePermissions()

    private let lock = NSLock()
    private var adapters: [NativePermission: ForgePermissionAdapter] = [:]

    public init() {
        if ForgeServices.isAppBundle {
            adapters[.notifications] = NotificationPermissionAdapter()
        }
    }

    /// Adds or replaces the adapter for `permission`.
    public func register(_ adapter: ForgePermissionAdapter, for permission: NativePermission) {
        lock.lock()
        defer { lock.unlock() }
        adapters[permission] = adapter
    }

    public func status(permission: NativePermission) async -> PermissionStatus {
        guard let adapter = adapter(for: permission) else { return .unsupported }
        return await adapter.status()
    }

    public func request(permission: NativePermission) async -> PermissionStatus {
        guard let adapter = adapter(for: permission) else { return .unsupported }
        return await adapter.request()
    }

    private func adapter(for permission: NativePermission) -> ForgePermissionAdapter? {
        lock.lock()
        defer { lock.unlock() }
        return adapters[permission]
    }

    #if canImport(UIKit) && os(iOS)
        /// Opens this app's page in the Settings app: the only place a user
        /// can undo `.denied`.
        @MainActor
        public static func openSettings() {
            if let url = URL(string: UIApplication.openSettingsURLString) {
                UIApplication.shared.open(url)
            }
        }
    #endif
}

/// Notifications: alerts, sounds and badges.
struct NotificationPermissionAdapter: ForgePermissionAdapter {
    func status() async -> PermissionStatus {
        let settings = await UNUserNotificationCenter.current().notificationSettings()
        return Self.map(settings.authorizationStatus)
    }

    func request() async -> PermissionStatus {
        // Throws only for invalid options; the answer is in the status.
        _ = try? await UNUserNotificationCenter.current()
            .requestAuthorization(options: [.alert, .sound, .badge])
        return await status()
    }

    static func map(_ status: UNAuthorizationStatus) -> PermissionStatus {
        switch status {
        case .notDetermined: return .notDetermined
        case .denied: return .denied
        case .authorized: return .granted
        case .provisional: return .provisional
        #if os(iOS)
            case .ephemeral: return .granted
        #endif
        @unknown default: return .unsupported
        }
    }
}
