@preconcurrency import ForgeFFI
import Foundation

#if canImport(UserNotifications)
    import UserNotifications

    /// Posts local notifications for new messages in rooms that are not
    /// open. Banners also show while the app is in the foreground, because
    /// `ForgeServices.install()` makes Forge's notification delegate present
    /// them (this class must not replace that delegate).
    ///
    /// Local only: they appear while the app runs, because Rust holds the
    /// room connections and iOS suspends the app soon after it leaves the
    /// screen. Alerts while suspended or closed need remote push (APNs),
    /// which this app does not have (see docs/chat-demo.md).
    @MainActor
    public final class LocalChatNotifier: ChatNotifier {
        /// The app's notifier, or nil outside an app bundle (`swift run`,
        /// `swift test`), where the notification center is unavailable.
        public static let shared: LocalChatNotifier? =
            ForgeServices.isAppBundle ? LocalChatNotifier() : nil

        private init() {
            // Asks once; later launches return the stored answer.
            UNUserNotificationCenter.current()
                .requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in }
        }

        public func post(_ message: ChatMessage, roomTitle: String) {
            let content = UNMutableNotificationContent()
            content.title = roomTitle
            content.subtitle = message.author
            content.body = message.body
            content.sound = .default
            // Groups a room's notifications together.
            content.threadIdentifier = message.conversationId
            content.userInfo = ["conversationId": message.conversationId]
            let request = UNNotificationRequest(
                identifier: message.id, content: content, trigger: nil)
            UNUserNotificationCenter.current().add(request, withCompletionHandler: nil)
        }
    }
#else
    /// No notification center on this platform.
    public enum LocalChatNotifier {
        public static let shared: ChatNotifier? = nil
    }
#endif
