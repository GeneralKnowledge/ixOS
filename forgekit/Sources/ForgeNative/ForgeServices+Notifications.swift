// Forge native services: local notifications. Part of ForgeKit (ForgeNative).

#if canImport(Darwin)

import Foundation
import UserNotifications

public enum ForgeNotifications {
    /// The `userInfo` key holding a notification's deep link.
    public static let deepLinkKey = "forgeDeepLink"
    /// The `userInfo` key holding the notification's own id (iOS can change
    /// the request identifier of a repeating one).
    public static let idKey = "forgeNotificationID"
    /// The `userInfo` key naming the module whose Rust scheduled it, so a
    /// response reaches that app's Rust when several share the process.
    public static let ownerKey = "forgeOwner"

    /// The notifier for this process: `UserNotificationsNotifier` in an app,
    /// `UnavailableNotifier` under `swift test` / `swift run`, where the
    /// notification centre does not exist.
    public static func makeNotifier() -> LocalNotifier {
        ForgeServices.isAppBundle ? UserNotificationsNotifier.shared : UnavailableNotifier()
    }
}

/// `LocalNotifier` backed by `UNUserNotificationCenter`. It is also the
/// centre's delegate: banners show while the app is open, tapping a
/// notification that carries a deep link opens it through `ForgeDeepLinks`,
/// and every response (a tap, an action button, a quick reply, a dismissal)
/// goes to Rust (`deliverNotificationResponse`).
///
/// Several apps can share one process (Device Lab hosts them), each with
/// its own copy of this class. The centre has one delegate and one set of
/// categories, so the copies share them (`ForgeProcessTable`): each copy's
/// categories are kept apart and set together, and a response goes to the
/// copy whose app scheduled the notification.
public final class UserNotificationsNotifier: NSObject, LocalNotifier, UNUserNotificationCenterDelegate,
    @unchecked Sendable
{
    public static let shared = UserNotificationsNotifier()

    private let center: UNUserNotificationCenter

    /// This copy's module, written on its notifications.
    private let owner = String(reflecting: UserNotificationsNotifier.self)

    private override init() {
        center = UNUserNotificationCenter.current()
        super.init()
        center.delegate = self
        #if canImport(ObjectiveC)
            let handle: (UNNotificationResponse) -> Void = { [weak self] response in
                self?.handle(response)
            }
            ForgeProcessTable.with(center) { table in
                ForgeProcessTable.section(table, "responders")[owner] = handle
            }
        #endif
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
        var userInfo: [String: Any] = [
            ForgeNotifications.idKey: notification.id, ForgeNotifications.ownerKey: owner,
        ]
        if let link = notification.deepLink { userInfo[ForgeNotifications.deepLinkKey] = link }
        content.userInfo = userInfo
        if let category = notification.category { content.categoryIdentifier = category }

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

    /// "category/action" for the actions that open the app, which also open
    /// the notification's deep link.
    private let opensAppActions = ForgeLocked<Set<String>>([])

    public func setCategories(categories: [NotificationCategory]) {
        opensAppActions.withLock { ids in
            ids = Set(categories.flatMap { category in
                category.actions.compactMap { action -> String? in
                    if case .opensApp = action.kind { return "\(category.id)/\(action.id)" }
                    return nil
                }
            })
        }
        let native = Set(categories.map { category in
            UNNotificationCategory(
                identifier: category.id,
                actions: category.actions.map(Self.action),
                intentIdentifiers: [],
                // Dismissals reach Rust too (NOTIFICATION_DISMISSED).
                options: [.customDismissAction])
        })
        #if canImport(ObjectiveC)
            // Every app's categories in the process, ours replaced.
            let all: Set<UNNotificationCategory> = ForgeProcessTable.with(center) { table in
                let byOwner = ForgeProcessTable.section(table, "categories")
                byOwner[owner] = native
                var all = Set<UNNotificationCategory>()
                for case let set as Set<UNNotificationCategory> in byOwner.allValues {
                    all.formUnion(set)
                }
                return all
            }
            center.setNotificationCategories(all)
        #else
            center.setNotificationCategories(native)
        #endif
    }

    private static func action(_ action: NotificationAction) -> UNNotificationAction {
        var options: UNNotificationActionOptions = []
        if action.requiresUnlock { options.insert(.authenticationRequired) }
        switch action.kind {
        case .button(let destructive):
            if destructive { options.insert(.destructive) }
            return UNNotificationAction(identifier: action.id, title: action.title, options: options)
        case .opensApp:
            options.insert(.foreground)
            return UNNotificationAction(identifier: action.id, title: action.title, options: options)
        case .textInput(let sendTitle, let placeholder):
            return UNTextInputNotificationAction(
                identifier: action.id, title: action.title, options: options,
                textInputButtonTitle: sendTitle, textInputPlaceholder: placeholder)
        }
    }

    // MARK: UNUserNotificationCenterDelegate

    /// Shows banners while the app is open too (iOS hides them by default).
    public func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .list, .sound])
    }

    /// Every response goes to Rust: this app's, or the app's that
    /// scheduled it when several share the process. A tap on the
    /// notification, or an action that opens the app, also opens its deep
    /// link.
    public func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        #if canImport(ObjectiveC)
            let scheduledBy = response.notification.request.content.userInfo[ForgeNotifications.ownerKey] as? String
            if let scheduledBy, scheduledBy != owner {
                let other = ForgeProcessTable.with(center) { table in
                    ForgeProcessTable.section(table, "responders")[scheduledBy]
                }
                if let other = other as? (UNNotificationResponse) -> Void {
                    other(response)
                    completionHandler()
                    return
                }
            }
        #endif
        handle(response)
        completionHandler()
    }

    /// Hands a response to this copy's Rust, and opens its deep link through
    /// this copy's `ForgeDeepLinks` when it opens the app.
    private func handle(_ response: UNNotificationResponse) {
        let request = response.notification.request
        let userInfo = request.content.userInfo
        let link = userInfo[ForgeNotifications.deepLinkKey] as? String
        let actionID: String
        switch response.actionIdentifier {
        case UNNotificationDefaultActionIdentifier: actionID = "forge.opened"
        case UNNotificationDismissActionIdentifier: actionID = "forge.dismissed"
        default: actionID = response.actionIdentifier
        }
        let category = request.content.categoryIdentifier
        deliverNotificationResponse(
            response: NotificationResponse(
                notificationId: userInfo[ForgeNotifications.idKey] as? String ?? request.identifier,
                actionId: actionID,
                text: (response as? UNTextInputNotificationResponse)?.userText,
                category: category.isEmpty ? nil : category,
                threadId: request.content.threadIdentifier.isEmpty ? nil : request.content.threadIdentifier,
                deepLink: link))
        let opensApp = response.actionIdentifier == UNNotificationDefaultActionIdentifier
            || opensAppActions.withLock { $0.contains("\(category)/\(response.actionIdentifier)") }
        if opensApp, let link, let url = URL(string: link) {
            Task { @MainActor in ForgeDeepLinks.shared.open(url) }
        }
    }
}

/// A value behind a lock (Swift 5 mode, iOS 17: no `Mutex` yet).
final class ForgeLocked<Value>: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Value

    init(_ value: Value) { self.value = value }

    func withLock<T>(_ body: (inout Value) -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return body(&value)
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

    public func setCategories(categories: [NotificationCategory]) {}
}

#endif
