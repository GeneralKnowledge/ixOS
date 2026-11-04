import Foundation
@preconcurrency import ForgeFFI

/// The Swift face of one Rust chat instance (`ForgeChatApp`).
///
/// Contract (see `crates/forge-ffi/src/chat/app.rs` and docs/ffi-policy.md):
/// - Safe from any task or actor, including the main actor. Nothing here
///   blocks: network work runs on Rust worker threads.
/// - `enter(_:)` / `leave(_:)` open and close a room's live connection and
///   return at once. After `leave` returns, no more events for that room's
///   connection arrive.
/// - Commands are not cancelled with the calling task, on purpose: leaving a
///   room must not cancel a message that is already being sent. Its outcome
///   still arrives on `events()` and in `cachedMessages(in:)`.
/// - One event stream per consumer (`events()`), covering every room.
public final class ChatClient: Sendable {
    private let app: ForgeChatApp

    /// The server this client talks to.
    public let serverURL: String

    /// - Parameters:
    ///   - storageDirectory: holds the chat store; created if missing. An
    ///     empty string keeps everything in memory.
    ///   - serverURL: e.g. `http://127.0.0.1:8787` (`make mock-chat`).
    ///   - user: the name sent as `author`; defaults to `"me"`.
    public init(
        storageDirectory: String, serverURL: String, user: String? = nil,
        requestTimeoutMs: UInt32? = nil
    ) throws {
        app = try bridged {
            try ForgeChatApp(
                storageDirectory: storageDirectory,
                config: ChatAppConfig(
                    baseUrl: serverURL, user: user, requestTimeoutMs: requestTimeoutMs))
        }
        self.serverURL = serverURL
    }

    // MARK: Events

    /// A new stream of every event from now on. Iterate it from one task;
    /// cancelling that task ends it.
    public func events() throws -> ChatEvents {
        ChatEvents(subscription: try bridged { try app.events() })
    }

    // MARK: Queries (local, immediate)

    public var cachedConversations: [ChatConversation] { app.cachedConversations() }

    public func cachedMessages(in conversationId: String) -> [ChatMessage] {
        app.cachedMessages(conversationId: conversationId)
    }

    public func isInside(_ conversationId: String) -> Bool {
        app.isSubscribed(conversationId: conversationId)
    }

    /// Room connection threads still running in Rust. Reaches 0 shortly
    /// after every room is left.
    public var liveStreams: UInt32 { app.liveStreams() }

    /// The last persistence problem, if any (never fails a command).
    public var storeProblem: String? { app.storeProblem() }

    public var isShutDown: Bool { app.isShutDown() }

    // MARK: Commands

    /// Lists rooms (and prefetches news for rooms that are not open).
    public func conversations() async throws -> [ChatConversation] {
        try await bridgedAsync { try await app.listConversations() }
    }

    /// Fetches what is new in a room and returns everything stored for it,
    /// oldest first, including unsent messages.
    public func syncMessages(in conversationId: String) async throws -> [ChatMessage] {
        try await bridgedAsync { try await app.syncMessages(conversationId: conversationId) }
    }

    /// Sends a message. It appears at once as `.sending` (a
    /// `messageReceived` event), then turns `.sent` or `.failed`.
    @discardableResult
    public func send(_ body: String, to conversationId: String) async throws -> ChatMessage {
        try await bridgedAsync {
            try await app.sendMessage(conversationId: conversationId, body: body)
        }
    }

    /// Sends a `.failed` message again (same id, so never twice).
    @discardableResult
    public func retry(messageId: String) async throws -> ChatMessage {
        try await bridgedAsync { try await app.retryMessage(messageId: messageId) }
    }

    /// Opens the room's live connection and marks it read. Idempotent.
    public func enter(_ conversationId: String) throws {
        try bridged { try app.subscribe(conversationId: conversationId) }
    }

    /// Closes the room's live connection. Idempotent; never fails.
    public func leave(_ conversationId: String) {
        try? app.unsubscribe(conversationId: conversationId)
    }

    // MARK: Lifecycle

    /// Closes every room and event stream; later commands throw `.closed`.
    public func shutdown() {
        app.shutdown()
    }
}

/// Rust chat events as an `AsyncSequence`.
///
/// Each `next()` awaits the next event from Rust, so a slow consumer slows
/// delivery instead of piling events up in Swift: Rust keeps up to 256 per
/// stream and reports overflow with `.eventsDropped`, after which the
/// consumer re-reads `cachedConversations` / `cachedMessages(in:)`.
public struct ChatEvents: AsyncSequence, Sendable {
    public typealias Element = ChatEvent

    private let subscription: ChatEventSubscription

    init(subscription: ChatEventSubscription) {
        self.subscription = subscription
    }

    /// Ends the stream now. A pending `next()` returns `nil`.
    public func cancel() {
        subscription.cancel()
    }

    public func makeAsyncIterator() -> AsyncIterator {
        AsyncIterator(subscription: subscription)
    }

    public struct AsyncIterator: AsyncIteratorProtocol, Sendable {
        let subscription: ChatEventSubscription

        public mutating func next() async -> ChatEvent? {
            // UniFFI does not forward Swift task cancellation to Rust, so do
            // it here: cancelling the subscription ends the pending `next()`.
            let subscription = self.subscription
            if Task.isCancelled {
                subscription.cancel()
                return nil
            }
            return await withTaskCancellationHandler {
                await subscription.next()
            } onCancel: {
                subscription.cancel()
            }
        }
    }
}
