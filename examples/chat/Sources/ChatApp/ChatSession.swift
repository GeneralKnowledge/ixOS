import ChatBridge
import Foundation

/// The state of a room's live connection, as the chat screen shows it.
public enum ConnectionState: Equatable, Sendable {
    case closed
    case connecting
    case connected
    /// The connection dropped; Rust is reconnecting with backoff.
    case reconnecting(reason: String)
}

/// A local notification for someone else's new message in a room that is
/// not open.
public struct ChatNotification: Equatable, Sendable {
    public var messageId: String
    public var conversationId: String
    public var roomTitle: String
    public var author: String
    public var body: String
}

/// Shows `ChatNotification`s. The app uses `LocalChatNotifier`; tests
/// record them.
@MainActor
public protocol ChatNotifier: AnyObject {
    func post(_ notification: ChatNotification)
}

/// Presentation state for both screens.
///
/// Holds no chat logic: the store, sync cursor, reconnects, retries and
/// message ids are all in Rust behind `ChatClient`. This class mirrors what
/// Rust reports through one event stream.
@MainActor
public final class ChatSession: ObservableObject {
    @Published public private(set) var conversations: [ChatConversation] = []
    /// Messages per room, oldest first, as last reported by Rust.
    @Published public private(set) var messages: [String: [ChatMessage]] = [:]
    @Published public private(set) var connection: [String: ConnectionState] = [:]
    @Published public private(set) var isRefreshing = false
    /// Why the last list refresh failed (cached conversations stay on screen).
    @Published public private(set) var listError: String?
    /// Why the last send or retry failed, per room. The bubble shows it too.
    @Published public private(set) var sendError: [String: String] = [:]

    public let client: ChatClient
    /// With a notifier, every room is watched while the session runs, and
    /// Rust's `.notify` events become local notifications. Without one,
    /// only the open room is live.
    private let notifier: ChatNotifier?
    private var eventTask: Task<Void, Never>?
    private var started = false

    public init(client: ChatClient, notifier: ChatNotifier? = nil) {
        self.client = client
        self.notifier = notifier
    }

    /// A session for `serverURL`, storing chats under Application Support.
    public convenience init(serverURL: String, notifier: ChatNotifier? = nil) throws {
        try self.init(
            client: ChatClient(
                storageDirectory: Self.storageDirectory(for: serverURL), serverURL: serverURL),
            notifier: notifier)
    }

    deinit {
        // Ends the event stream too, so the event task finishes on its own.
        client.shutdown()
    }

    /// Shows stored conversations at once, starts listening, then refreshes.
    /// Call from `.task`; later calls only refresh.
    public func start() async {
        if !started {
            started = true
            conversations = client.cachedConversations
            listenForEvents()
        }
        await refresh()
    }

    /// Re-lists rooms from the server (pull to refresh).
    public func refresh() async {
        isRefreshing = true
        defer { isRefreshing = false }
        do {
            conversations = try await client.conversations()
            listError = nil
            conversations.forEach { watchIfNotifying($0.id) }
        } catch {
            listError = Self.message(for: error)
        }
    }

    // MARK: Chat screen

    /// Opens a room: shows stored messages, connects, then catches up.
    public func open(_ conversationId: String) async {
        messages[conversationId] = client.cachedMessages(in: conversationId)
        // A watched room's stream is already running (and reported): keep its state.
        switch connection[conversationId] {
        case .connected, .reconnecting: break
        case .closed, .connecting, nil: connection[conversationId] = .connecting
        }
        do {
            try client.enter(conversationId)
            messages[conversationId] = try await client.syncMessages(in: conversationId)
        } catch {
            sendError[conversationId] = Self.message(for: error)
        }
    }

    /// Leaves a room. Messages still being sent finish anyway.
    public func close(_ conversationId: String) {
        client.leave(conversationId)
    }

    /// Sends `text`. The bubble appears through the `messageReceived` event.
    public func send(_ text: String, to conversationId: String) async {
        let body = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !body.isEmpty else { return }
        sendError[conversationId] = nil
        do {
            try await client.send(body, to: conversationId)
        } catch {
            sendError[conversationId] = Self.message(for: error)
        }
    }

    /// Tapping a failed bubble.
    public func retry(_ message: ChatMessage) async {
        guard message.status == .failed else { return }
        sendError[message.conversationId] = nil
        do {
            try await client.retry(messageId: message.id)
        } catch {
            sendError[message.conversationId] = Self.message(for: error)
        }
    }

    public func messages(in conversationId: String) -> [ChatMessage] {
        messages[conversationId] ?? []
    }

    public func title(of conversationId: String) -> String {
        conversations.first { $0.id == conversationId }?.title ?? conversationId
    }

    /// Keeps a room live in the background so its news can notify.
    /// Watching fails only after shutdown, so a failure is ignored.
    private func watchIfNotifying(_ conversationId: String) {
        guard notifier != nil else { return }
        try? client.watch(conversationId)
    }

    // MARK: Events

    private func listenForEvents() {
        let events: ChatEvents
        do {
            events = try client.events()
        } catch {
            listError = Self.message(for: error)
            return
        }
        eventTask = Task { [weak self] in
            for await event in events {
                guard let self else { break }
                self.apply(event)
            }
        }
    }

    /// Applies one Rust event. Internal so tests can drive it directly.
    func apply(_ event: ChatEvent) {
        switch event {
        case let .conversationUpdated(conversation):
            if let index = conversations.firstIndex(where: { $0.id == conversation.id }) {
                conversations[index] = conversation
            } else {
                conversations.append(conversation)
                watchIfNotifying(conversation.id)
            }
        case let .notify(message):
            // Rust raises this only for others' new messages in rooms that
            // are not open, so there is nothing to filter here.
            notifier?.post(
                ChatNotification(
                    messageId: message.id, conversationId: message.conversationId,
                    roomTitle: title(of: message.conversationId), author: message.author,
                    body: message.body))
        case let .messageReceived(message):
            var list = messages[message.conversationId] ?? []
            if let index = list.firstIndex(where: { $0.id == message.id }) {
                list[index] = message
            } else {
                list.append(message)
            }
            list.sort { ($0.sentAt, $0.id) < ($1.sentAt, $1.id) }
            messages[message.conversationId] = list
        case let .messageStatusChanged(id, status):
            for (room, list) in messages {
                guard let index = list.firstIndex(where: { $0.id == id }) else { continue }
                // Re-read from the store: a `.sent` message also carries the
                // server's time and may move.
                let stored = client.cachedMessages(in: room)
                if stored.contains(where: { $0.id == id }) {
                    messages[room] = stored
                } else {
                    var updated = list
                    updated[index].status = status
                    messages[room] = updated
                }
                break
            }
        case let .streamConnected(conversationId):
            connection[conversationId] = .connected
        case let .streamDisconnected(conversationId, reason, willRetry):
            connection[conversationId] = willRetry ? .reconnecting(reason: reason) : .closed
        case .eventsDropped:
            // We fell behind: the store is the truth.
            conversations = client.cachedConversations
            for room in messages.keys {
                messages[room] = client.cachedMessages(in: room)
            }
        }
    }

    // MARK: Helpers

    static func message(for error: any Error) -> String {
        if let error = error as? ChatClientError { return error.userMessage }
        if error is CancellationError { return "Cancelled." }
        return "Something went wrong: \(error.localizedDescription)"
    }

    /// One store per server, so switching servers never mixes rooms.
    static func storageDirectory(for serverURL: String) -> String {
        let base =
            FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        let safe = serverURL.unicodeScalars.map {
            CharacterSet.alphanumerics.contains($0) ? String($0) : "_"
        }.joined()
        return base.appendingPathComponent("ForgeChat/\(safe)", isDirectory: true).path
    }
}
