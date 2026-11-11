@testable import ChatApp
@preconcurrency import ChatFFI
import XCTest

/// Records notifications instead of showing them.
@MainActor
final class RecordingNotifier: ChatNotifier {
    var posted: [(ChatMessage, String)] = []
    func post(_ message: ChatMessage, roomTitle: String) {
        posted.append((message, roomTitle))
    }
}

/// The model against the real Rust core and the real chat server: no mocks.
@MainActor
final class ChatModelTests: XCTestCase {
    private func model(notifier: ChatNotifier? = nil) throws -> ChatModel {
        ChatModel(
            storageDirectory: try temporaryDirectory(self), notifier: notifier, useKeychain: false)
    }

    func testFirstLaunchAsksForAnAccount() async throws {
        let model = try model()
        await model.open()
        XCTAssertEqual(model.phase, .signedOut)
        XCTAssertNil(model.account)
        model.close()
    }

    func testSignUpThenSendAndReceive() async throws {
        let server = try ChatServer.start()
        defer { server.stop() }
        let notifier = RecordingNotifier()
        let me = try model(notifier: notifier)
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

        // A watched room that is not open notifies.
        await waitUntil("a notification for random") {
            notifier.posted.contains { $0.0.body == "hello from the laptop" && $0.1 == "Random" }
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
