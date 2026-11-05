import Foundation

#if canImport(UserNotifications)
    import UserNotifications

    /// Posts `ChatNotification`s as local notifications, banners included
    /// while the app is in the foreground.
    ///
    /// Local only: the app must be running, because the Rust client holds the
    /// room connections. There is no push server (see docs/chat-demo.md).
    @MainActor
    public final class LocalChatNotifier: NSObject, ChatNotifier, UNUserNotificationCenterDelegate
    {
        /// The app's notifier, or `nil` when the process has no bundle
        /// identifier (`swift run`, `swift test`), where the notification
        /// center is unavailable.
        public static let shared: LocalChatNotifier? =
            Bundle.main.bundleIdentifier == nil ? nil : LocalChatNotifier()

        private override init() {
            super.init()
            let center = UNUserNotificationCenter.current()
            center.delegate = self
            // Asks once; later launches return the stored answer. If the
            // person says no, `post` still runs and the system drops it.
            center.requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in }
        }

        public func post(_ notification: ChatNotification) {
            let content = UNMutableNotificationContent()
            content.title = notification.roomTitle
            content.subtitle = notification.author
            content.body = notification.body
            content.sound = .default
            // Groups a room's notifications together.
            content.threadIdentifier = notification.conversationId
            content.userInfo = ["conversationId": notification.conversationId]
            let request = UNNotificationRequest(
                identifier: notification.messageId, content: content, trigger: nil)
            UNUserNotificationCenter.current().add(request, withCompletionHandler: nil)
        }

        /// Shows the banner while the app is open too (by default iOS only
        /// shows notifications for apps in the background).
        public nonisolated func userNotificationCenter(
            _ center: UNUserNotificationCenter, willPresent notification: UNNotification,
            withCompletionHandler completionHandler:
                @escaping (UNNotificationPresentationOptions) -> Void
        ) {
            completionHandler([.banner, .list, .sound])
        }
    }
#endif
