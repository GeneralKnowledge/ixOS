@preconcurrency import ChatFFI
import Foundation

/// The deep link a message notification opens:
/// `forgechat://conversation/<id>?message=<id>`. Rust writes it on each
/// notification (`chatMessageLink`, rust/src/alerts.rs); `MainTabs` routes
/// it to the room (forge.toml registers the scheme).
enum ChatLinks {
    static let scheme = "forgechat"

    /// The room (and message) a link points at, or nil if it is not a Chat link.
    static func route(_ link: DeepLink) -> ChatRoute? {
        guard link.scheme == scheme, link.route.count == 2, link.route[0] == "conversation" else {
            return nil
        }
        let message = link.query.first { $0.name == "message" }?.value
        return ChatRoute(conversationId: link.route[1], messageId: message)
    }
}
