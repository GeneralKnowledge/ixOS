@testable import RelayApp
@preconcurrency import RelayFFI
import XCTest

/// The deep link on a message notification (written by Rust) leads back to
/// that message.
final class ChatLinksTests: XCTestCase {
    func testANotificationLinkRoutesToItsMessage() throws {
        let link = chatMessageLink(conversationId: "room 7", messageId: "m-1")
        XCTAssertTrue(link.hasPrefix("forgechat://conversation/"), link)
        let route = ChatLinks.route(try parseDeepLink(url: link))
        XCTAssertEqual(route, ChatRoute(conversationId: "room 7", messageId: "m-1"))

        let slashed = chatMessageLink(conversationId: "a/b", messageId: "m-2")
        XCTAssertEqual(
            ChatLinks.route(try parseDeepLink(url: slashed)),
            ChatRoute(conversationId: "a/b", messageId: "m-2"))
    }

    func testOtherLinksAreIgnored() throws {
        for url in ["forgechat://settings", "otherapp://conversation/x", "forgechat://conversation/a/b"] {
            XCTAssertNil(ChatLinks.route(try parseDeepLink(url: url)), url)
        }
    }
}
