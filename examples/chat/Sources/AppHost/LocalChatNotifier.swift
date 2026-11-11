@preconcurrency import ChatFFI
import Foundation

/// The deep link a message notification opens: `forgechat://conversation/<id>?message=<id>`.
/// `MainTabs` routes it to the room (forge.toml registers the scheme).
enum ChatLinks {
    static let scheme = "forgechat"

    static func message(_ message: ChatMessage) -> String? {
        var components = URLComponents()
        components.scheme = scheme
        components.host = "conversation"
        // One path segment, even if the id contains "/".
        var segment = CharacterSet.urlPathAllowed
        segment.remove("/")
        guard let id = message.conversationId.addingPercentEncoding(withAllowedCharacters: segment)
        else { return nil }
        components.percentEncodedPath = "/" + id
        components.queryItems = [URLQueryItem(name: "message", value: message.id)]
        return components.string
    }

    /// The room (and message) a link points at, or nil if it is not a Chat link.
    static func route(_ link: DeepLink) -> ChatRoute? {
        guard link.scheme == scheme, link.route.count == 2, link.route[0] == "conversation" else {
            return nil
        }
        let message = link.query.first { $0.name == "message" }?.value
        return ChatRoute(conversationId: link.route[1], messageId: message)
    }
}

/// Posts local notifications for new messages in rooms that are not open,
/// through Forge's notifier (`ForgeNotifications`): grouped by room, with
/// the author as subtitle, and a deep link that opens the room when tapped.
/// Banners also show while the app is in the foreground, because Forge's
/// notifier is the notification centre's delegate.
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

    private let notifier = ForgeNotifications.makeNotifier()

    private init() {
        // Asks once; later launches return the stored answer.
        Task { _ = await ForgePermissions.shared.request(permission: .notifications) }
    }

    public func post(_ message: ChatMessage, roomTitle: String) {
        let notification = LocalNotification(
            id: message.id, title: roomTitle, body: message.body, subtitle: message.author,
            threadId: message.conversationId, deepLink: ChatLinks.message(message), trigger: .now,
            // Reply and Mark as Read (handled in Rust, rust/src/lib.rs).
            category: messageNotificationCategory())
        let notifier = self.notifier
        Task {
            // Refused when notifications are off; nothing to tell the user then.
            try? await notifier.schedule(notification: notification)
        }
    }
}
