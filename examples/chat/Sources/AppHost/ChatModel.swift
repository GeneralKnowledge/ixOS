@preconcurrency import ChatFFI
import Foundation
import Observation

/// The state of a room's live connection, as the chat screen shows it.
public enum ConnectionState: Equatable, Sendable {
    case closed
    case connecting
    case connected
    /// The connection dropped (or the app was in the background); Rust is
    /// reconnecting.
    case reconnecting(reason: String)
}

/// Shows local notifications for Rust's `.notify` events. The app uses
/// `LocalChatNotifier`; tests record them.
@MainActor
public protocol ChatNotifier: AnyObject {
    func post(_ message: ChatMessage, roomTitle: String)
}

/// The app's presentation state. It holds no chat rules: the account, the
/// store, sync cursors, reconnects, retries, unread counts and who wrote
/// what are all decided in Rust (`rust/src/lib.rs`). This model mirrors what
/// Rust reports, through command results and one event stream.
@Observable
@MainActor
public final class ChatModel {
    /// How far opening the Rust core has got.
    public enum Phase: Equatable {
        case loading
        /// No account on this device yet: show sign-up.
        case signedOut
        case ready
        case failed(String)
    }

    public private(set) var phase: Phase = .loading
    public private(set) var account: ChatAccount?
    /// Conversations in the server's order.
    public private(set) var conversations: [ChatConversation] = []
    /// Messages per room, oldest first, for rooms that have been opened.
    public private(set) var messages: [String: [ChatMessage]] = [:]
    public private(set) var connection: [String: ConnectionState] = [:]
    public private(set) var isRefreshing = false
    /// Why the last refresh failed (stored conversations stay on screen).
    public private(set) var listError: String?
    /// Why the last send or retry failed, per room.
    public private(set) var sendError: [String: String] = [:]
    /// Why opening a room (going live or catching up) failed, per room. Kept
    /// apart from `sendError`: nothing the person wrote failed.
    public private(set) var roomError: [String: String] = [:]
    /// Set when the server stopped accepting this device's account.
    public private(set) var accountProblem: String?
    /// A failed action, for an alert. The view clears it when dismissed.
    public var actionError: String?
    /// The lifecycle phase last reported to Rust.
    public private(set) var lifecyclePhase: AppPhase = .launching
    /// Increases after each message this device sends; a haptic trigger.
    public private(set) var sentCount = 0
    /// Increases each time one of this device's messages fails to send; a
    /// haptic trigger.
    public private(set) var failedCount = 0
    /// Our messages waiting in Rust's outbox (written offline, or the
    /// server could not be reached). Rust sends them by itself.
    public private(set) var waitingCount = 0

    public private(set) var core: ChatCore?
    /// The on-device assistant, once the core is open. It needs no account.
    public private(set) var assistant: AssistantModel?
    private let storageDirectory: String?
    private let useKeychain: Bool?
    @ObservationIgnored private let notifier: ChatNotifier?
    @ObservationIgnored private let assistantEngine: LocalModelEngine?
    @ObservationIgnored private var eventTask: Task<Void, Never>?
    @ObservationIgnored private var events: ChatEvents?

    /// - Parameters:
    ///   - storageDirectory: where Rust keeps each account's chats; defaults
    ///     to Application Support. Tests pass a temporary folder.
    ///   - notifier: shows notifications for new messages in rooms that are
    ///     not open. With one, every room is watched while the app runs.
    ///   - useKeychain: keep the account in the Keychain (the default when
    ///     `ForgeServices.install()` has run) or only in memory (tests).
    ///   - assistantEngine: the on-device model for the assistant (the app
    ///     passes `FoundationModelsEngine()`; tests a fake or nothing).
    public init(
        storageDirectory: String? = nil, notifier: ChatNotifier? = nil, useKeychain: Bool? = nil,
        assistantEngine: LocalModelEngine? = nil
    ) {
        self.storageDirectory = storageDirectory
        self.notifier = notifier
        self.useKeychain = useKeychain
        self.assistantEngine = assistantEngine
    }

    // MARK: Opening and the account

    /// Opens the Rust core and starts listening. Call from `.task`; does
    /// nothing when already open.
    public func open() async {
        guard core == nil else { return }
        phase = .loading
        do {
            let directory = try storageDirectory ?? Self.applicationSupportDirectory()
            let keychain = useKeychain ?? nativeServicesInstalled()
            // Opening reads the Keychain and a file: do it off the main actor.
            let core = try await Task.detached {
                if keychain {
                    return try ChatCore(storageDirectory: directory)
                }
                return try ChatCore.withMemoryCredentials(storageDirectory: directory)
            }.value
            // Subscribe before reading, so nothing is missed in between.
            let events = try core.events()
            self.core = core
            self.events = events
            assistant = AssistantModel(core: core.assistant(), engine: assistantEngine)
            listen(to: events)
            if lifecyclePhase != .launching { core.setPhase(phase: lifecyclePhase) }
            // Before the tabs appear, so a locked app never shows them.
            appLock = core.appLock()
            showAccount(core.account())
        } catch {
            phase = .failed(Self.describe(error))
        }
    }

    /// Creates an account on the server and signs this device in with it.
    /// Throws so the sign-up form can stay open with the input intact.
    public func signUp(serverURL: String, displayName: String) async throws {
        guard let core else { throw ChatError.ShutDown }
        let account = try await core.signUp(serverUrl: serverURL, displayName: displayName)
        showAccount(account)
    }

    /// Forgets the account and its chats on this device.
    public func signOut() {
        guard let core else { return }
        do {
            try core.signOut()
            showAccount(nil)
        } catch {
            actionError = Self.describe(error)
        }
    }

    /// Ends the event stream and shuts Rust down. Sent messages are on disk.
    public func close() {
        events?.cancel()
        eventTask?.cancel()
        core?.shutdown()
        core = nil
        events = nil
    }

    /// Reports the scene phase to Rust (`ContentView` uses
    /// `.forgeLifecycle`). Rust closes every connection in the background and
    /// reconnects on the way back, then this model refreshes the list.
    public func setPhase(_ phase: AppPhase) {
        lifecyclePhase = phase
        if phase == .terminated {
            close()
        } else {
            core?.setPhase(phase: phase)
            // Apple Intelligence may have been turned on meanwhile.
            if phase == .active { assistant?.refresh() }
        }
    }

    // MARK: Conversation list

    /// Re-lists rooms from the server (pull to refresh, and on resume).
    public func refresh() async {
        guard let core, account != nil else { return }
        isRefreshing = true
        defer { isRefreshing = false }
        do {
            conversations = try await core.listConversations()
            listError = nil
            conversations.forEach { watchIfNotifying($0.id) }
        } catch {
            listError = Self.describe(error)
        }
    }

    public func title(of conversationId: String) -> String {
        conversation(conversationId)?.title ?? conversationId
    }

    public func conversation(_ id: String) -> ChatConversation? {
        conversations.first { $0.id == id }
    }

    /// The list as the Chats tab shows it: most recent activity first.
    /// Rooms without messages keep the server's order, after the rest.
    public var sortedConversations: [ChatConversation] {
        conversations.enumerated()
            .sorted { a, b in
                let (left, right) = (activity(a.element), activity(b.element))
                return left != right ? left > right : a.offset < b.offset
            }
            .map(\.element)
    }

    /// Rooms with the unread dot, for the tab's badge.
    public var unreadConversationCount: Int {
        conversations.filter(\.showsUnread).count
    }

    private func activity(_ conversation: ChatConversation) -> Int64 {
        max(conversation.lastMessage?.sentAt ?? .min, conversation.lastMessageAt ?? .min)
    }

    /// Creates a conversation and returns its id, ready to open. Throws so
    /// the sheet can stay open with the title intact.
    public func createConversation(title: String) async throws -> String {
        guard let core else { throw ChatError.ShutDown }
        let conversation = try await core.createConversation(title: title)
        if self.conversation(conversation.id) == nil {
            conversations.append(conversation)
        }
        watchIfNotifying(conversation.id)
        return conversation.id
    }

    /// Marks the room read here and on the server (a swipe).
    public func markRead(_ conversationId: String) async {
        guard let core else { return }
        do {
            try await core.markRead(conversationId: conversationId)
        } catch {
            actionError = Self.describe(error)
        }
    }

    /// Shows the room as unread until it is opened (a swipe).
    public func markUnread(_ conversationId: String) {
        perform { try $0.setMarkedUnread(conversationId: conversationId, marked: true) }
    }

    /// Mutes or unmutes the room's notifications on this iPhone.
    public func setMuted(_ conversationId: String, _ muted: Bool) {
        perform { try $0.setMuted(conversationId: conversationId, muted: muted) }
    }

    /// The app lock setting (Rust keeps it); `appLockMethod` says how it
    /// would unlock here, nil when this iPhone can't lock at all.
    public private(set) var appLock = false
    public var appLockMethod: String? { ForgeFFI.appLockMethod() }

    public func setAppLock(_ enabled: Bool) {
        perform { try $0.setAppLock(enabled: enabled) }
        appLock = core?.appLock() ?? false
    }

    private func perform(_ action: (ChatCore) throws -> Void) {
        guard let core else { return }
        do {
            try action(core)
        } catch {
            actionError = Self.describe(error)
        }
    }

    // MARK: A conversation

    public func messages(in conversationId: String) -> [ChatMessage] {
        messages[conversationId] ?? []
    }

    /// Where one of our messages is: sending, sent, read or failed. Rust
    /// decides "read" from the other accounts' read cursors.
    public func delivery(of message: ChatMessage) -> ChatDelivery {
        chatDelivery(message: message, conversation: conversation(message.conversationId))
    }

    /// Opens a room: shows stored messages, goes live, then catches up.
    public func enter(_ conversationId: String) async {
        guard let core else { return }
        messages[conversationId] = core.cachedMessages(conversationId: conversationId)
        switch connection[conversationId] {
        case .connected, .reconnecting: break
        case .closed, .connecting, nil: connection[conversationId] = .connecting
        }
        roomError[conversationId] = nil
        do {
            try core.subscribe(conversationId: conversationId)
        } catch {
            // No stream started, so no event will ever move the room on from
            // "Connecting…": say it isn't connected, and why.
            connection[conversationId] = .closed
            roomError[conversationId] = Self.describe(error)
            return
        }
        do {
            messages[conversationId] = try await core.syncMessages(conversationId: conversationId)
        } catch {
            // Live, but the catch-up failed: stored messages stay on screen.
            roomError[conversationId] = Self.describe(error)
        }
    }

    /// Leaves a room. Messages still being sent finish anyway.
    public func leave(_ conversationId: String) {
        try? core?.unsubscribe(conversationId: conversationId)
    }

    /// Sends `text`, as an answer to `replyTo` if given. The bubble
    /// appears through `.messageReceived`. Offline, it waits in Rust's
    /// outbox (still "Sending…") and goes out when the server is back, even
    /// after a relaunch. Sending clears the room's draft.
    public func send(_ text: String, to conversationId: String, replyTo: String? = nil) async {
        guard let core else { return }
        let body = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !body.isEmpty else { return }
        sendError[conversationId] = nil
        sentCount += 1
        do {
            _ = try await core.sendReply(
                conversationId: conversationId, body: body, replyTo: replyTo)
        } catch {
            sendError[conversationId] = Self.describe(error)
        }
        updateWaitingCount()
    }

    /// Sends a failed (or still waiting) message again now.
    public func retry(_ message: ChatMessage) async {
        guard let core, message.status != .sent else { return }
        sendError[message.conversationId] = nil
        do {
            _ = try await core.retryMessage(messageId: message.id)
        } catch {
            sendError[message.conversationId] = Self.describe(error)
        }
        updateWaitingCount()
    }

    /// Sends photos and documents (copied into Rust's attachment store at
    /// once), with `text` as an optional caption. Like `send`, they wait in
    /// the outbox while the server cannot be reached.
    public func send(
        files: [ChatOutgoingFile], text: String, to conversationId: String, replyTo: String? = nil
    ) async {
        guard let core, !files.isEmpty else { return }
        sendError[conversationId] = nil
        sentCount += 1
        do {
            _ = try await core.sendFiles(
                conversationId: conversationId,
                body: text.trimmingCharacters(in: .whitespacesAndNewlines),
                replyTo: replyTo, files: files)
        } catch {
            sendError[conversationId] = Self.describe(error)
        }
        updateWaitingCount()
    }

    /// The attachment's file on this iPhone, fetching it from the server
    /// the first time (Rust checks it against its id). Nil if that fails.
    public func localFile(for attachment: ChatAttachment) async -> URL? {
        guard let core else { return nil }
        if let path = core.attachmentFile(attachmentId: attachment.id) {
            return URL(fileURLWithPath: path)
        }
        do {
            let path = try await core.fetchAttachment(attachmentId: attachment.id)
            return URL(fileURLWithPath: path)
        } catch {
            attachmentLog.warn("fetch of \(attachment.id) failed: \(error)")
            return nil
        }
    }

    /// A file for Quick Look and the share sheet: the attachment under its
    /// own name. Shows an alert if it cannot be fetched.
    public func previewFile(for attachment: ChatAttachment) async -> URL? {
        guard let core else { return nil }
        do {
            let path: String
            if let local = core.attachmentFile(attachmentId: attachment.id) {
                path = local
            } else {
                path = try await core.fetchAttachment(attachmentId: attachment.id)
            }
            return try AttachmentFiles.namedCopy(
                of: URL(fileURLWithPath: path), for: attachment)
        } catch {
            actionError = Self.describe(error)
            return nil
        }
    }

    /// Replaces the text of one of our sent messages, for everyone.
    public func edit(_ message: ChatMessage, to text: String) async {
        guard let core else { return }
        let body = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !body.isEmpty, body != message.body else { return }
        do {
            upsert(try await core.editMessage(messageId: message.id, body: body))
        } catch {
            actionError = Self.describe(error)
        }
    }

    /// Deletes one of our messages for everyone (a "deleted" placeholder
    /// stays). One that was never sent is simply removed.
    public func delete(_ message: ChatMessage) async {
        guard let core else { return }
        do {
            if let tombstone = try await core.deleteMessage(messageId: message.id) {
                upsert(tombstone)
            } else {
                remove(message.id, from: message.conversationId)
            }
        } catch {
            actionError = Self.describe(error)
        }
        updateWaitingCount()
    }

    /// Adds our `emoji` to a message, or takes it away if it is already ours.
    public func toggleReaction(_ emoji: String, on message: ChatMessage) async {
        guard let core else { return }
        let mine = message.reactions.contains { $0.emoji == emoji && $0.mine }
        do {
            upsert(try await core.setReaction(messageId: message.id, emoji: emoji, on: !mine))
        } catch {
            actionError = Self.describe(error)
        }
    }

    /// The room's unsent composer text, kept by Rust across launches.
    public func draft(for conversationId: String) -> String {
        core?.draft(conversationId: conversationId) ?? ""
    }

    /// Saves the composer's text (empty clears the draft).
    public func saveDraft(_ text: String, for conversationId: String) {
        try? core?.setDraft(conversationId: conversationId, text: text)
    }

    /// Messages on this iPhone containing every word of `query` (as word
    /// starts, ignoring case and accents), newest first: in one room, or in
    /// all with nil. Rust searches its SQLite full-text index.
    public func search(_ query: String, in conversationId: String? = nil) async -> [ChatMessage] {
        guard let core, !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return []
        }
        do {
            return try await core.search(query: query, conversationId: conversationId)
        } catch {
            return []
        }
    }

    /// A loaded message of a room, for quoting the message a reply answers.
    public func message(_ id: String, in conversationId: String) -> ChatMessage? {
        messages[conversationId]?.first { $0.id == id }
    }

    private func updateWaitingCount() {
        waitingCount = Int(core?.outboxCount() ?? 0)
    }

    // MARK: Events

    private func listen(to events: ChatEvents) {
        eventTask = Task { [weak self] in
            while let event = await events.next() {
                guard let self else { return }
                self.apply(event)
            }
        }
    }

    /// Applies one Rust event. Internal so tests can drive it directly.
    func apply(_ event: ChatEvent) {
        switch event {
        case .accountChanged(let account):
            showAccount(account)
        case .accountRejected(let reason):
            accountProblem = reason
        case .conversationUpdated(let conversation):
            if let index = conversations.firstIndex(where: { $0.id == conversation.id }) {
                conversations[index] = conversation
            } else {
                conversations.append(conversation)
                watchIfNotifying(conversation.id)
            }
        case .notify(let message):
            // Rust raises this only for others' new messages in rooms that
            // are not open.
            notifier?.post(message, roomTitle: title(of: message.conversationId))
        case .messageReceived(let message):
            upsert(message)
            if message.isMine { updateWaitingCount() }
        case .messageUpdated(let message):
            // Edits, deletions and reactions, by anyone. Only rooms already
            // on screen keep a list; the rest read the store when opened.
            if messages[message.conversationId]?.contains(where: { $0.id == message.id }) == true {
                upsert(message)
            }
        case .messageRemoved(let room, let id):
            remove(id, from: room)
            updateWaitingCount()
        case .messageStatusChanged(let room, let id, let status):
            updateWaitingCount()
            guard var list = messages[room], let index = list.firstIndex(where: { $0.id == id })
            else { break }
            // A `.sent` message also carries the server's time and may move:
            // re-read the store.
            if status == .failed && list[index].isMine { failedCount += 1 }
            let stored = core?.cachedMessages(conversationId: room) ?? []
            if stored.contains(where: { $0.id == id }) {
                messages[room] = stored
            } else {
                list[index].status = status
                messages[room] = list
            }
        case .streamConnected(let conversationId):
            connection[conversationId] = .connected
        case .streamDisconnected(let conversationId, let reason, let willRetry):
            connection[conversationId] = willRetry ? .reconnecting(reason: reason) : .closed
        case .phaseChanged(_, let resumed):
            // Back from the background: Rust has reconnected the rooms; fetch
            // what changed meanwhile.
            if resumed { Task { await refresh() } }
        case .eventsDropped:
            // Fell behind: the store is the truth.
            reloadFromStore()
        }
    }

    private func upsert(_ message: ChatMessage) {
        var list = messages[message.conversationId] ?? []
        if let index = list.firstIndex(where: { $0.id == message.id }) {
            list[index] = message
        } else {
            list.append(message)
        }
        list.sort { ($0.sentAt, $0.id) < ($1.sentAt, $1.id) }
        messages[message.conversationId] = list
    }

    private func remove(_ id: String, from conversationId: String) {
        messages[conversationId]?.removeAll { $0.id == id }
    }

    private func reloadFromStore() {
        guard let core else { return }
        conversations = core.cachedConversations()
        for room in messages.keys {
            messages[room] = core.cachedMessages(conversationId: room)
        }
    }

    /// Switches the screens to `account` (nil: sign-up), starting fresh.
    private func showAccount(_ newAccount: ChatAccount?) {
        let changed = newAccount != account || phase == .loading
        account = newAccount
        guard changed else { return }
        accountProblem = nil
        messages = [:]
        connection = [:]
        sendError = [:]
        listError = nil
        conversations = core?.cachedConversations() ?? []
        updateWaitingCount()
        phase = newAccount == nil ? .signedOut : .ready
        if newAccount != nil {
            conversations.forEach { watchIfNotifying($0.id) }
            Task { await refresh() }
        }
    }

    /// Keeps a room live while the app runs, so its news can notify.
    private func watchIfNotifying(_ conversationId: String) {
        guard notifier != nil else { return }
        try? core?.watch(conversationId: conversationId)
    }

    // MARK: Helpers

    /// Text for the screen; `reason` fields are diagnostics from Rust.
    public static func describe(_ error: Error) -> String {
        guard let error = error as? ChatError else { return error.localizedDescription }
        switch error {
        case .Network(let reason):
            return "Can't reach the chat server. Is it running, and is the phone on the same network? (\(reason))"
        case .Certificate(let reason): return "The server's certificate was rejected: \(reason)"
        case .Http(let status, let reason): return "The server answered \(status): \(reason)"
        case .BadResponse(let reason): return "The server sent something unexpected: \(reason)"
        case .Cancelled: return "Cancelled."
        case .NotFound: return "That conversation no longer exists."
        case .UnsupportedOnHost(let detail): return "Not available here: \(detail)"
        case .InvalidInput(let reason): return "Check your input: \(reason)"
        case .NotSignedIn: return "Sign up first."
        case .Unauthorized:
            return "The server no longer knows this account. Sign out and sign up again."
        case .Storage(let reason): return "Your chats couldn't be read or saved (\(reason))."
        case .ShutDown: return "The app is closing."
        case .Internal(let reason): return "Something went wrong: \(reason)"
        }
    }

    private static func applicationSupportDirectory() throws -> String {
        let base = try FileManager.default
            .url(
                for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil,
                create: true
            )
            .appendingPathComponent("Chat", isDirectory: true)
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base.path
    }
}
