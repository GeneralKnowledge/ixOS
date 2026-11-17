@testable import RelayApp
@preconcurrency import RelayFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import XCTest

/// The model against the real Rust core and the real chat server: no mocks.
@MainActor
final class ChatModelTests: XCTestCase {
    private func model() throws -> ChatModel {
        ChatModel(storageDirectory: try temporaryDirectory(self), useKeychain: false)
    }

    func testFirstLaunchAsksForAnAccount() async throws {
        let model = try model()
        await model.open()
        XCTAssertEqual(model.phase, .signedOut)
        XCTAssertNil(model.account)
        model.close()
    }

    /// Someone else's message in a chat that isn't open taps once; Rust
    /// decides which messages those are (not mine, not muted, not open).
    func testAMessageInAnotherChatTaps() throws {
        let model = try model()
        let message = ChatMessage(
            id: "m1", conversationId: "general", author: "Laptop", authorId: "id-laptop",
            isMine: false, body: "hi", sentAt: 0, status: .sent,
            replyTo: nil, edited: false, deleted: false, reactions: [], attachments: [])
        model.apply(.messageReceived(message: message))
        XCTAssertNil(model.feedback.kind)
        model.apply(.notify(message: message))
        XCTAssertEqual(model.feedback.kind, .refresh)
        XCTAssertEqual(model.feedback.count, 1)
    }

    func testSignUpThenSendAndReceive() async throws {
        let server = try ChatServer.start()
        defer { server.stop() }
        let me = try model()
        let other = try model()
        await me.open()
        await other.open()

        try await me.signUp(serverURL: server.url, displayName: "Reuben")
        try await other.signUp(serverURL: server.url, displayName: "Laptop")
        XCTAssertEqual(me.phase, .ready)
        XCTAssertEqual(me.account?.displayName, "Reuben")
        await waitUntil("rooms listed") { me.conversations.map(\.id).contains("general") }

        // Mine, decided by Rust from the account id.
        await me.enter("general")
        await waitUntil("connected") { me.connection["general"] == .connected }
        await me.send("hello from the phone", to: "general")
        await waitUntil("sent") { me.messages(in: "general").first?.status == .sent }
        XCTAssertEqual(me.messages(in: "general").first?.isMine, true)

        // Theirs, live while the room is open.
        await other.refresh()
        await other.send("hello from the laptop", to: "random")
        await other.send("in general too", to: "general")
        await waitUntil("their message arrives") {
            me.messages(in: "general").contains { $0.body == "in general too" && !$0.isMine }
        }

        // A room that is not open counts it as unread. (Notifications are
        // Rust's: rust/src/tests.rs, `message_notifications_come_from_rust`.)
        await me.refresh()
        await waitUntil("random shows unread") {
            me.conversation("random")?.showsUnread == true
        }
        me.leave("general")
        me.close()
        other.close()
    }

    func testBackgroundClosesConnectionsAndResumeRefreshes() async throws {
        let server = try ChatServer.start()
        defer { server.stop() }
        let me = try model()
        let other = try model()
        await me.open()
        await other.open()
        me.setPhase(.active)
        try await me.signUp(serverURL: server.url, displayName: "Me")
        try await other.signUp(serverURL: server.url, displayName: "Other")
        await me.enter("general")
        await waitUntil("connected") { me.connection["general"] == .connected }

        me.setPhase(.background)
        await waitUntil("streams stop") { me.core?.liveStreams() == 0 }
        await other.refresh()
        await other.send("while you were away", to: "general")

        me.setPhase(.active)
        await waitUntil("caught up") {
            me.messages(in: "general").contains { $0.body == "while you were away" }
        }
        me.close()
        other.close()
    }

    func testSignOutReturnsToSignUp() async throws {
        let server = try ChatServer.start()
        defer { server.stop() }
        let me = try model()
        await me.open()
        try await me.signUp(serverURL: server.url, displayName: "Me")
        me.signOut()
        XCTAssertEqual(me.phase, .signedOut)
        XCTAssertNil(me.account)
        XCTAssertTrue(me.conversations.isEmpty)
        me.close()
    }

    /// One core per data folder: a second model on the same folder can't
    /// open while the first is open, says why, and Try Again opens it once
    /// the first has closed.
    func testALoadFailureShowsWhyAndTryAgainOpens() async throws {
        let directory = try temporaryDirectory(self)
        let first = ChatModel(storageDirectory: directory, useKeychain: false)
        await first.open()
        XCTAssertEqual(first.phase, .signedOut)

        let second = ChatModel(storageDirectory: directory, useKeychain: false)
        await second.open()
        guard case .failed(let message) = second.phase else {
            return XCTFail("a second core opened the same data: \(second.phase)")
        }
        XCTAssertFalse(message.isEmpty)
        XCTAssertNil(second.core)
        XCTAssertEqual(chatErrorKind(error: ChatError.InUse(reason: "")), .unavailable)

        first.close()
        // What the recovery screen's Try Again does.
        await second.open()
        XCTAssertEqual(second.phase, .signedOut)
        XCTAssertNotNil(second.core)
        second.close()
    }

    func testBadServerURLIsExplainedNotThrownAway() async throws {
        let me = try model()
        await me.open()
        do {
            try await me.signUp(serverURL: "localhost:8787", displayName: "Me")
            XCTFail("expected an error")
        } catch {
            XCTAssertTrue(ChatModel.describe(error).hasPrefix("Check your input"))
        }
        XCTAssertEqual(me.phase, .signedOut)
        me.close()
    }
}
