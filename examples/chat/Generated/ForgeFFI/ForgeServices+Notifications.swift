// Forge native services: local notifications. Source of truth: crates/forge-services/swift/.

import Foundation
import UserNotifications

public enum ForgeNotifications {
    /// The `userInfo` key holding a notification's deep link.
    public static let deepLinkKey = "forgeDeepLink"

    /// The notifier for this process: `UserNotificationsNotifier` in an app,
    /// `UnavailableNotifier` under `swift test` / `swift run`, where the
    /// notification centre does not exist.
    public static func makeNotifier() -> LocalNotifier {
        ForgeServices.isAppBundle ? UserNotificationsNotifier.shared : UnavailableNotifier()
    }
}

/// `LocalNotifier` backed by `UNUserNotificationCenter`. It is also the
/// centre's delegate: banners show while the app is open, and tapping a
/// notification that carries a deep link opens it through `ForgeDeepLinks`.
public final class UserNotificationsNotifier: NSObject, LocalNotifier, UNUserNotificationCenterDelegate,
    @unchecked Sendable
{
    public static let shared = UserNotificationsNotifier()

    private let center: UNUserNotificationCenter

    private override init() {
        center = UNUserNotificationCenter.current()
        super.init()
        center.delegate = self
    }

    public func schedule(notification: LocalNotification) async throws {
        let settings = await center.notificationSettings()
        switch settings.authorizationStatus {
        case .denied:
            throw NativeServiceError.PermissionDenied(
                reason: "notifications are turned off for this app in Settings")
        case .notDetermined:
            throw NativeServiceError.PermissionDenied(
                reason: "the user has not been asked yet: request NativePermission.notifications first")
        default:
            break
        }

        let content = UNMutableNotificationContent()
        content.title = notification.title
        content.body = notification.body
        content.sound = .default
        if let subtitle = notification.subtitle { content.subtitle = subtitle }
        if let thread = notification.threadId { content.threadIdentifier = thread }
        if let link = notification.deepLink { content.userInfo = [ForgeNotifications.deepLinkKey: link] }

        let trigger: UNNotificationTrigger?
        switch notification.trigger {
        case .now:
            trigger = nil
        case .after(let seconds, let repeats):
            trigger = UNTimeIntervalNotificationTrigger(
                timeInterval: TimeInterval(seconds), repeats: repeats)
        case .at(let unixMs):
            let date = Date(timeIntervalSince1970: TimeInterval(unixMs) / 1000)
            let components = Calendar.current.dateComponents(
                [.year, .month, .day, .hour, .minute, .second], from: date)
            trigger = UNCalendarNotificationTrigger(dateMatching: components, repeats: false)
        }

        let request = UNNotificationRequest(
            identifier: notification.id, content: content, trigger: trigger)
        do {
            try await center.add(request)
        } catch {
            let ns = error as NSError
            throw NativeServiceError.Platform(code: Int64(ns.code), reason: ns.localizedDescription)
        }
    }

    public func cancel(id: String) {
        center.removePendingNotificationRequests(withIdentifiers: [id])
        center.removeDeliveredNotifications(withIdentifiers: [id])
    }

    public func pending() async -> [String] {
        await center.pendingNotificationRequests().map { $0.identifier }
    }

    // MARK: UNUserNotificationCenterDelegate

    /// Shows banners while the app is open too (iOS hides them by default).
    public func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .list, .sound])
    }

    /// A tap on a notification with a deep link opens that link.
    public func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let userInfo = response.notification.request.content.userInfo
        if let link = userInfo[ForgeNotifications.deepLinkKey] as? String, let url = URL(string: link) {
            Task { @MainActor in ForgeDeepLinks.shared.open(url) }
        }
        completionHandler()
    }
}

/// `LocalNotifier` for processes without a notification centre.
public final class UnavailableNotifier: LocalNotifier, @unchecked Sendable {
    public init() {}

    public func schedule(notification: LocalNotification) async throws {
        throw NativeServiceError.Unavailable(
            reason: "no notification centre in this process (not an installed app)")
    }

    public func cancel(id: String) {}

    public func pending() async -> [String] { [] }
}
