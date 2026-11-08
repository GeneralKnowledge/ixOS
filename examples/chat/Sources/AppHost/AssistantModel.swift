@preconcurrency import ForgeFFI
import Foundation
import Observation

/// The on-device assistant's presentation state. Rust (`rust/src/assistant`)
/// owns the conversations, the characters, the prompt and when an answer
/// stops; this model mirrors them and follows the answer being written.
@Observable
@MainActor
public final class AssistantModel {
    public private(set) var conversations: [AssistantConversation] = []
    /// Messages per conversation, oldest first, for conversations opened.
    public private(set) var messages: [String: [AssistantMessage]] = [:]
    /// The live text of the answer being written, by message id.
    public private(set) var liveText: [String: String] = [:]
    /// The conversation whose answer is being written, if any.
    public private(set) var writingConversationId: String?
    /// The engine and whether it can run now.
    public private(set) var engine: EngineInfo?
    /// A failed action, for an alert. The view clears it when dismissed.
    public var actionError: String?

    public let characters: [AssistantCharacter]
    private let core: AssistantCore
    @ObservationIgnored private var generation: AssistantGeneration?
    @ObservationIgnored private var followTask: Task<Void, Never>?

    /// - Parameter engine: the model engine; the app passes
    ///   `FoundationModelsEngine()`, tests a fake. `nil` keeps what Rust has.
    public init(core: AssistantCore, engine: LocalModelEngine?) {
        self.core = core
        if let engine { core.setEngine(engine: engine) }
        characters = core.characters()
        refresh()
    }

    /// Re-reads the list and the engine's availability (on appear, and when
    /// the app comes back, since Apple Intelligence may have been turned on).
    public func refresh() {
        engine = core.engineInfo()
        do {
            conversations = try core.conversations()
        } catch {
            actionError = Self.describe(error)
        }
        // An answer started before this screen existed: follow it.
        if generation == nil, let current = core.currentGeneration() {
            follow(current, catchUp: true)
        }
    }

    public func conversation(_ id: String) -> AssistantConversation? {
        conversations.first { $0.id == id }
    }

    /// Loads a conversation's messages for its screen.
    public func open(_ conversationId: String) {
        do {
            messages[conversationId] = try core.messages(conversationId: conversationId)
        } catch {
            actionError = Self.describe(error)
        }
    }

    /// Starts a conversation and returns its id, ready to open.
    public func start(characterId: String) -> String? {
        do {
            let conversation = try core.startConversation(characterId: characterId)
            conversations.insert(conversation, at: 0)
            return conversation.id
        } catch {
            actionError = Self.describe(error)
            return nil
        }
    }

    public func delete(_ conversationId: String) {
        do {
            try core.deleteConversation(conversationId: conversationId)
            conversations.removeAll { $0.id == conversationId }
            messages[conversationId] = nil
        } catch {
            actionError = Self.describe(error)
        }
    }

    /// Sends a message and follows the answer. Returns false (and sets
    /// `actionError`) when Rust refused it, so the composer keeps the text.
    @discardableResult
    public func send(_ text: String, in conversationId: String) -> Bool {
        do {
            let generation = try core.send(conversationId: conversationId, text: text)
            open(conversationId)
            follow(generation, catchUp: false)
            touch(conversationId)
            return true
        } catch {
            engine = core.engineInfo()
            actionError = Self.describe(error)
            return false
        }
    }

    /// Stops the answer being written; its text so far is kept.
    public func stop() {
        generation?.cancel()
    }

    public func isWriting(_ conversationId: String) -> Bool {
        writingConversationId == conversationId
    }

    /// Words for the user, for every error Rust's assistant reports.
    public static func describe(_ error: Error) -> String {
        guard let error = error as? AssistantError else { return error.localizedDescription }
        switch error {
        case .NoEngine: return "No on-device model is installed in this build."
        case .Unavailable(let reason): return reason
        case .Busy: return "Wait for the answer to finish, or stop it."
        case .NotFound: return "That conversation no longer exists."
        case .InvalidInput(let reason): return "Check your message: \(reason)."
        case .Storage(let reason): return "Your assistant chats couldn't be read or saved (\(reason))."
        case .ShutDown: return "The app is closing."
        }
    }

    private func follow(_ generation: AssistantGeneration, catchUp: Bool) {
        self.generation = generation
        let conversationId = generation.conversationId()
        let messageId = generation.messageId()
        writingConversationId = conversationId
        liveText[messageId] = catchUp ? generation.catchUp() : ""
        followTask = Task { [weak self] in
            while let update = await generation.next() {
                guard let self else { return }
                switch update {
                case .text(let text):
                    self.liveText[messageId, default: ""] += text
                case .finished(let message):
                    self.finished(message)
                }
            }
        }
    }

    private func finished(_ message: AssistantMessage) {
        let conversationId = message.conversationId
        if var list = messages[conversationId],
            let index = list.firstIndex(where: { $0.id == message.id })
        {
            list[index] = message
            messages[conversationId] = list
        }
        liveText[message.id] = nil
        generation = nil
        writingConversationId = nil
        touch(conversationId)
    }

    /// Re-reads one conversation's row (its preview line) and moves it to
    /// the top.
    private func touch(_ conversationId: String) {
        guard let fresh = try? core.conversations() else { return }
        conversations = fresh
    }
}
