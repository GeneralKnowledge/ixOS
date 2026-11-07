@testable import AppHost
@preconcurrency import ForgeFFI
import XCTest

/// The deep link on a message notification leads back to that message.
final class ChatLinksTests: XCTestCase {
    private func message(id: String, in conversation: String) -> ChatMessage {
        ChatMessage(
            id: id, conversationId: conversation, author: "Ana", authorId: "id-ana",
            isMine: false, body: "hi", sentAt: 0, status: .sent,
            replyTo: nil, edited: false, deleted: false, reactions: [], attachments: [])
    }

    func testANotificationLinkRoutesToItsMessage() throws {
        let link = try XCTUnwrap(ChatLinks.message(message(id: "m-1", in: "room 7")))
        XCTAssertTrue(link.hasPrefix("forgechat://conversation/"), link)
        let route = ChatLinks.route(try parseDeepLink(url: link))
        XCTAssertEqual(route, ChatRoute(conversationId: "room 7", messageId: "m-1"))

        let slashed = try XCTUnwrap(ChatLinks.message(message(id: "m-2", in: "a/b")))
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
