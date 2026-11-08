@testable import AppHost
@preconcurrency import ForgeFFI
import XCTest

/// A scripted Swift engine: pushes `pieces` with a pause between them, and
/// stops when Rust's sink says so, as a real engine must.
final class ScriptedEngine: LocalModelEngine, Sendable {
    let pieces: [String]
    let pause: Duration

    init(pieces: [String], pause: Duration = .zero) {
        self.pieces = pieces
        self.pause = pause
    }

    func describe() -> EngineInfo {
        EngineInfo(
            id: "scripted", name: "Scripted", available: true, unavailableReason: nil,
            contextTokens: 4096)
    }

    func generate(request: GenerateRequest, sink: TokenSink) async throws {
        for piece in pieces {
            try? await Task.sleep(for: pause)
            if !sink.push(text: piece) { return }
        }
    }
}

/// The assistant through the model and the real Rust core, no account.
@MainActor
final class AssistantModelTests: XCTestCase {
    private func open(engine: LocalModelEngine?) async throws -> ChatModel {
        let model = ChatModel(
            storageDirectory: try temporaryDirectory(self), useKeychain: false,
            assistantEngine: engine)
        await model.open()
        return model
    }

    func testTheAssistantWorksSignedOutAndStreamsAnAnswer() async throws {
        let chat = try await open(engine: ScriptedEngine(pieces: ["Hi", " there", "!"]))
        defer { chat.close() }
        XCTAssertEqual(chat.phase, .signedOut)
        let assistant = try XCTUnwrap(chat.assistant)
        XCTAssertEqual(assistant.characters.map(\.name), ["Assistant", "Leo", "Mia"])

        let id = try XCTUnwrap(assistant.start(characterId: "girlfriend"))
        assistant.open(id)
        XCTAssertEqual(assistant.messages[id]?.count, 1, "Mia opens the conversation")
        XCTAssertTrue(assistant.send("Hello Mia", in: id))
        await waitUntil("the answer is saved") {
            assistant.messages[id]?.last?.state == .done
        }
        XCTAssertEqual(assistant.messages[id]?.last?.text, "Hi there!")
        XCTAssertNil(assistant.writingConversationId)
        XCTAssertEqual(assistant.conversations.first?.lastMessage?.text, "Hi there!")
    }

    func testStopKeepsTheText() async throws {
        let engine = ScriptedEngine(
            pieces: (0..<200).map { "\($0) " }, pause: .milliseconds(10))
        let chat = try await open(engine: engine)
        defer { chat.close() }
        let assistant = try XCTUnwrap(chat.assistant)
        let id = try XCTUnwrap(assistant.start(characterId: "assistant"))
        assistant.send("Count", in: id)
        XCTAssertTrue(assistant.isWriting(id))
        XCTAssertFalse(assistant.send("Again", in: id), "one answer at a time")
        XCTAssertNotNil(assistant.actionError)

        await waitUntil("some text") {
            (assistant.liveText.values.first?.count ?? 0) > 10
        }
        assistant.stop()
        await waitUntil("stopped") { assistant.messages[id]?.last?.state == .stopped }
        let text = try XCTUnwrap(assistant.messages[id]?.last?.text)
        XCTAssertFalse(text.isEmpty)
        XCTAssertLessThan(text.count, (0..<200).map { "\($0) " }.joined().count)
    }

    func testWithoutAnEngineSendingSaysSo() async throws {
        let chat = try await open(engine: nil)
        defer { chat.close() }
        let assistant = try XCTUnwrap(chat.assistant)
        XCTAssertNil(assistant.engine)
        let id = try XCTUnwrap(assistant.start(characterId: "boyfriend"))
        XCTAssertFalse(assistant.send("Hi Leo", in: id))
        XCTAssertEqual(assistant.actionError, "No on-device model is installed in this build.")
    }
}
