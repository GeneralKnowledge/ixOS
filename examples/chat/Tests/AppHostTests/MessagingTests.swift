@testable import AppHost
@preconcurrency import ForgeFFI
import XCTest

/// M1 against the real Rust core and the real chat server: read receipts,
/// previews, unread state, mute and new conversations.
@MainActor
final class MessagingTests: XCTestCase {
    private func signedIn(_ name: String, on server: ChatServer) async throws -> ChatModel {
        let model = ChatModel(
            storageDirectory: try temporaryDirectory(self), notifier: nil, useKeychain: false)
        await model.open()
        try await model.signUp(serverURL: server.url, displayName: name)
        await waitUntil("\(name)'s rooms listed") { !model.conversations.isEmpty }
        return model
    }

    func testReadReceiptsPreviewsAndUnreadBetweenTwoPeople() async throws {
        let server = try ChatServer.start()
        defer { server.stop() }
        let me = try await signedIn("Reuben", on: server)
        let other = try await signedIn("Laptop", on: server)

        // I write in General; it is Delivered until the laptop reads it.
        await me.enter("general")
        await waitUntil("connected") { me.connection["general"] == .connected }
        await me.send("are you there?", to: "general")
        await waitUntil("sent") { me.messages(in: "general").first?.status == .sent }
        let mine = try XCTUnwrap(me.messages(in: "general").first)
        XCTAssertEqual(me.delivery(of: mine), .sent)
        XCTAssertEqual(me.conversation("general")?.lastMessage?.body, "are you there?")
        XCTAssertEqual(me.sortedConversations.first?.id, "general", "newest activity first")

        // The laptop sees it unread, with a preview, then opens the room.
        await other.refresh()
        let unread = try XCTUnwrap(other.conversation("general"))
        XCTAssertTrue(unread.showsUnread)
        XCTAssertEqual(unread.unreadCount, 1)
        XCTAssertEqual(other.unreadConversationCount, 1)
        XCTAssertEqual(unread.lastMessage?.author, "Reuben")
        XCTAssertEqual(unread.lastMessage?.isMine, false)
        await other.enter("general")
        XCTAssertEqual(other.conversation("general")?.showsUnread, false)

        // ...and on my side the message turns Read.
        await waitUntil("read by the laptop") { me.delivery(of: mine) == .read }
        me.leave("general")
        other.leave("general")
        me.close()
        other.close()
    }

    func testSwipesMuteAndNewConversations() async throws {
        let server = try ChatServer.start()
        defer { server.stop() }
        let me = try await signedIn("Me", on: server)
        let other = try await signedIn("Other", on: server)

        // A new conversation, found by the other account.
        let id = try await me.createConversation(title: "Weekend")
        XCTAssertEqual(me.title(of: id), "Weekend")
        await other.refresh()
        XCTAssertNotNil(other.conversation(id))
        do {
            _ = try await me.createConversation(title: "   ")
            XCTFail("expected an error")
        } catch {
            XCTAssertTrue(ChatModel.describe(error).hasPrefix("Check your input"))
        }

        // Mark unread, then read, by swipe.
        me.markUnread(id)
        await waitUntil("marked unread") { me.conversation(id)?.markedUnread == true }
        XCTAssertEqual(me.unreadConversationCount, 1)
        await me.markRead(id)
        await waitUntil("read again") { me.conversation(id)?.showsUnread == false }

        // Muted rooms keep this device's setting.
        me.setMuted(id, true)
        await waitUntil("muted") { me.conversation(id)?.muted == true }
        XCTAssertNil(me.actionError)
        me.setMuted("no-such-room", true)
        XCTAssertNotNil(me.actionError)
        me.close()
        other.close()
    }

    /// M2: replies, reactions, edits and deletes reach the other person
    /// live; drafts and search come from Rust's SQLite store.
    func testRepliesReactionsEditsDeletesDraftsAndSearch() async throws {
        let server = try ChatServer.start()
        defer { server.stop() }
        let me = try await signedIn("Reuben", on: server)
        let other = try await signedIn("Laptop", on: server)
        await me.enter("general")
        await other.enter("general")
        await waitUntil("both connected") {
            me.connection["general"] == .connected && other.connection["general"] == .connected
        }

        // I ask; the laptop replies to it.
        await me.send("Café at noon?", to: "general")
        await waitUntil("the laptop has it") { other.messages(in: "general").count == 1 }
        let question = try XCTUnwrap(other.messages(in: "general").first)
        await other.send("Yes", to: "general", replyTo: question.id)
        await waitUntil("the reply arrives") { me.messages(in: "general").count == 2 }
        let reply = try XCTUnwrap(me.messages(in: "general").last)
        XCTAssertEqual(reply.replyTo, question.id)
        XCTAssertEqual(me.message(question.id, in: "general")?.body, "Café at noon?")

        // The laptop reacts; I see it, and toggling mine adds then removes it.
        await other.toggleReaction("👍", on: question)
        await waitUntil("the reaction shows") {
            me.message(question.id, in: "general")?.reactions.first?.count == 1
        }
        let reacted = try XCTUnwrap(me.message(question.id, in: "general"))
        XCTAssertEqual(reacted.reactions.first?.mine, false)
        await me.toggleReaction("👍", on: reacted)
        XCTAssertEqual(me.message(question.id, in: "general")?.reactions.first?.count, 2)
        XCTAssertEqual(me.message(question.id, in: "general")?.reactions.first?.mine, true)
        await me.toggleReaction("👍", on: try XCTUnwrap(me.message(question.id, in: "general")))
        XCTAssertEqual(me.message(question.id, in: "general")?.reactions.first?.count, 1)

        // I edit my question; the laptop sees it edited.
        await me.edit(try XCTUnwrap(me.message(question.id, in: "general")), to: "Café at one?")
        await waitUntil("the edit arrives") {
            other.message(question.id, in: "general")?.body == "Café at one?"
        }
        XCTAssertEqual(other.message(question.id, in: "general")?.edited, true)

        // The laptop deletes its reply; I see the placeholder.
        await other.delete(try XCTUnwrap(other.message(reply.id, in: "general")))
        await waitUntil("the deletion arrives") {
            me.message(reply.id, in: "general")?.deleted == true
        }
        XCTAssertEqual(me.message(reply.id, in: "general")?.body, "")
        XCTAssertNil(me.actionError)
        XCTAssertNil(other.actionError)

        // Search ignores case and accents; deleted messages are not found.
        let found = await me.search("cafe ONE")
        XCTAssertEqual(found.map(\.id), [question.id])
        let deleted = await me.search("yes", in: "general")
        XCTAssertEqual(deleted.count, 0)

        // Drafts are kept per room until sent.
        me.saveDraft("see you there", for: "general")
        XCTAssertEqual(me.draft(for: "general"), "see you there")
        XCTAssertEqual(me.draft(for: "random"), "")
        await me.send("see you there", to: "general")
        XCTAssertEqual(me.draft(for: "general"), "")
        XCTAssertEqual(me.waitingCount, 0)
        me.leave("general")
        other.leave("general")
        me.close()
        other.close()
    }
}

/// Grouping and separators in the transcript, without a server.
final class TranscriptTests: XCTestCase {
    private let minute: Int64 = 60_000
    private let base: Int64 = 1_794_000_000_000

    private func message(
        _ id: String, by author: String, at minutes: Int64, mine: Bool = false,
        status: ChatMessageStatus = .sent
    ) -> ChatMessage {
        ChatMessage(
            id: id, conversationId: "general", author: author, authorId: "id-\(author)",
            isMine: mine, body: id, sentAt: base + minutes * minute, status: status,
            replyTo: nil, edited: false, deleted: false, reactions: [])
    }

    private func places(_ rows: [Transcript.Row]) -> [String: Transcript.Place] {
        var places: [String: Transcript.Place] = [:]
        for row in rows {
            if case .message(let message, let place) = row.kind { places[message.id] = place }
        }
        return places
    }

    func testGroupsByAuthorAndTimeWithSeparators() {
        let rows = Transcript.rows([
            message("a1", by: "Ana", at: 0),
            message("a2", by: "Ana", at: 1),
            message("m1", by: "Me", at: 2, mine: true),
            message("m2", by: "Me", at: 3, mine: true),
            message("m3", by: "Me", at: 90, mine: true, status: .failed),
        ])
        let separators = rows.filter {
            if case .separator = $0.kind { return true }
            return false
        }
        XCTAssertEqual(separators.count, 2, "the first message and after an hour's quiet")
        let place = places(rows)
        XCTAssertEqual(place["a1"]?.showsAuthor, true)
        XCTAssertEqual(place["a2"]?.showsAuthor, false)
        XCTAssertEqual(place["a2"]?.endsGroup, true)
        XCTAssertEqual(place["m1"]?.startsGroup, true)
        XCTAssertEqual(place["m1"]?.showsAuthor, false, "never on our own messages")
        XCTAssertEqual(place["m1"]?.showsDelivery, false)
        XCTAssertEqual(place["m2"]?.showsDelivery, true, "the newest one we sent")
        XCTAssertEqual(place["m3"]?.startsGroup, true)
        XCTAssertEqual(place["m3"]?.showsDelivery, true, "failed ones always say so")
    }
}
