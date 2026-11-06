import ChatBridge
import Foundation
import XCTest

@testable import ChatApp

@MainActor
private final class RecordingNotifier: ChatNotifier {
    var posted: [ChatNotification] = []
    func post(_ notification: ChatNotification) { posted.append(notification) }
}

@MainActor
final class ChatSessionTests: XCTestCase {
    private func offlineSession() throws -> ChatSession {
        let client = try ChatClient(
            storageDirectory: try temporaryDirectory(self), serverURL: "http://127.0.0.1:9",
            requestTimeoutMs: 2_000)
        return ChatSession(client: client)
    }

    private func message(_ id: String, at sentAt: Int64, _ status: ChatMessageStatus = .sent)
        -> ChatMessage
    {
        ChatMessage(
            id: id, conversationId: "general", author: "ana", body: id, sentAt: sentAt,
            status: status)
    }

    func testServerDownShowsAListErrorAndKeepsGoing() async throws {
        let session = try offlineSession()
        await session.start()
        XCTAssertNotNil(session.listError)
        XCTAssertTrue(session.conversations.isEmpty)
        XCTAssertFalse(session.isRefreshing)
    }

    func testFailedSendShowsAFailedBubbleAndAnError() async throws {
        let session = try offlineSession()
        await session.start()
        await session.send("hello", to: "general")
        XCTAssertNotNil(session.sendError["general"])
        await waitUntil("the failed bubble is shown") {
            session.messages(in: "general").first?.status == .failed
        }
        XCTAssertEqual(session.messages(in: "general").first?.body, "hello")
    }

    func testBlankDraftsAreNotSent() async throws {
        let session = try offlineSession()
        await session.send("  \n ", to: "general")
        XCTAssertNil(session.sendError["general"])
        XCTAssertTrue(session.client.cachedMessages(in: "general").isEmpty)
    }

    func testEventsUpsertAndOrderMessages() throws {
        let session = try offlineSession()
        session.apply(.messageReceived(message: message("b", at: 2)))
        session.apply(.messageReceived(message: message("a", at: 1)))
        session.apply(.messageReceived(message: message("a", at: 1)))
        XCTAssertEqual(session.messages(in: "general").map(\.id), ["a", "b"])

        session.apply(.messageReceived(message: message("c", at: 3, .sending)))
        session.apply(.messageStatusChanged(conversationId: "general", id: "c", status: .failed))
        XCTAssertEqual(session.messages(in: "general").last?.status, .failed)

        // A status change names its room: another room's event leaves this one alone.
        session.apply(.messageStatusChanged(conversationId: "random", id: "c", status: .sent))
        XCTAssertEqual(session.messages(in: "general").last?.status, .failed)
    }

    func testConnectionStateFollowsStreamEvents() throws {
        let session = try offlineSession()
        session.apply(.streamConnected(conversationId: "general"))
        XCTAssertEqual(session.connection["general"], .connected)
        session.apply(
            .streamDisconnected(conversationId: "general", reason: "reset", willRetry: true))
        XCTAssertEqual(session.connection["general"], .reconnecting(reason: "reset"))
        session.apply(
            .streamDisconnected(conversationId: "general", reason: "unsubscribed", willRetry: false))
        XCTAssertEqual(session.connection["general"], .closed)
    }

    func testConversationUpdatesReplaceByID() throws {
        let session = try offlineSession()
        let room = ChatConversation(id: "general", title: "General", lastMessageAt: nil, unreadCount: 0)
        session.apply(.conversationUpdated(conversation: room))
        var updated = room
        updated.unreadCount = 3
        session.apply(.conversationUpdated(conversation: updated))
        XCTAssertEqual(session.conversations, [updated])
    }

    func testFullFlowAgainstTheMockServer() async throws {
        let server = try MockServer.start()
        let client = try ChatClient(
            storageDirectory: try temporaryDirectory(self), serverURL: server.url,
            requestTimeoutMs: 2_000)
        let session = ChatSession(client: client)
        await session.start()
        XCTAssertNil(session.listError)
        XCTAssertEqual(session.conversations.map(\.id), ["general", "random", "forge"])

        await session.open("general")
        await waitUntil("connected") { session.connection["general"] == .connected }
        await session.send("hi there", to: "general")
        await waitUntil("the message is sent") {
            session.messages(in: "general").contains { $0.body == "hi there" && $0.status == .sent }
        }
        session.close("general")
        await waitUntil("disconnected") { session.connection["general"] == .closed }
        server.stop()
    }

    // MARK: Notifications

    func testNotifyEventsBecomeNotificationsWithTheRoomTitle() throws {
        let notifier = RecordingNotifier()
        let client = try ChatClient(
            storageDirectory: try temporaryDirectory(self), serverURL: "http://127.0.0.1:9",
            requestTimeoutMs: 2_000)
        let session = ChatSession(client: client, notifier: notifier)
        session.apply(
            .conversationUpdated(
                conversation: ChatConversation(
                    id: "random", title: "Random", lastMessageAt: nil, unreadCount: 0)))
        XCTAssertTrue(client.isWatched("random"), "new rooms are watched")

        let incoming = ChatMessage(
            id: "m1", conversationId: "random", author: "ana", body: "psst", sentAt: 1,
            status: .sent)
        session.apply(.messageReceived(message: incoming))
        session.apply(.notify(message: incoming))
        XCTAssertEqual(
            notifier.posted,
            [
                ChatNotification(
                    messageId: "m1", conversationId: "random", roomTitle: "Random",
                    author: "ana", body: "psst")
            ])
    }

    func testWithoutANotifierRoomsAreNotWatched() throws {
        let session = try offlineSession()
        session.apply(
            .conversationUpdated(
                conversation: ChatConversation(
                    id: "random", title: "Random", lastMessageAt: nil, unreadCount: 0)))
        XCTAssertFalse(session.client.isWatched("random"))
    }

    func testOthersMessagesInRoomsYouAreNotInNotify() async throws {
        let server = try MockServer.start()
        let notifier = RecordingNotifier()
        let client = try ChatClient(
            storageDirectory: try temporaryDirectory(self), serverURL: server.url,
            requestTimeoutMs: 2_000)
        let session = ChatSession(client: client, notifier: notifier)
        // Someone else, posting like curl would.
        let other = try ChatClient(storageDirectory: "", serverURL: server.url, user: "curl")
        defer {
            other.shutdown()
            server.stop()
        }

        await session.start()
        XCTAssertEqual(session.conversations.map(\.id), ["general", "random", "forge"])
        for room in session.conversations {
            XCTAssertTrue(client.isWatched(room.id), "\(room.id) is watched")
        }
        await session.open("general")
        await waitUntil("connected") { session.connection["general"] == .connected }

        // A room you are not in: a notification.
        try await other.send("over here", to: "random")
        await waitUntil("the notification") { notifier.posted.count == 1 }
        XCTAssertEqual(notifier.posted[0].roomTitle, "Random")
        XCTAssertEqual(notifier.posted[0].author, "curl")
        XCTAssertEqual(notifier.posted[0].body, "over here")

        // The open room, and your own messages: none.
        try await other.send("in the open room", to: "general")
        await waitUntil("the message shows") {
            session.messages(in: "general").contains { $0.body == "in the open room" }
        }
        await session.send("mine", to: "random")
        await waitUntil("your message is sent") {
            client.cachedMessages(in: "random").contains { $0.body == "mine" && $0.status == .sent }
        }
        XCTAssertEqual(notifier.posted.count, 1)

        // Leave the room: it stays live, and its news notifies again.
        session.close("general")
        XCTAssertTrue(client.isWatched("general"))
        try await other.send("after you left", to: "general")
        await waitUntil("the second notification") { notifier.posted.count == 2 }
        XCTAssertEqual(notifier.posted[1].conversationId, "general")
    }
}
