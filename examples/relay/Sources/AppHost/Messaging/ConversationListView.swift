@preconcurrency import RelayFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// The Chats tab: its own navigation stack, so a new conversation can open
/// as soon as it is created.
struct ChatsTab: View {
    let model: ChatModel
    @Binding var path: [ChatRoute]

    var body: some View {
        NavigationStack(path: $path) {
            ConversationListView(model: model) { path.append($0) }
                .navigationDestination(for: ChatRoute.self) { route in
                    ChatView(
                        model: model, conversationId: route.conversationId,
                        focusMessage: route.messageId)
                }
        }
    }
}

/// The conversations, most recent first, with a preview of the last message
/// and an unread dot. Swipe to mark read or unread, or to mute. Pull to
/// refresh re-lists them. The search field finds messages in every
/// conversation stored on this iPhone.
struct ConversationListView: View {
    let model: ChatModel
    /// Opens a conversation (after creating one, or at a search result).
    let open: (ChatRoute) -> Void
    @State private var creating = false
    @State private var query = ""
    @State private var results: [ChatMessage] = []
    /// The query `results` answer: "No Results" waits for the search of
    /// what is typed now instead of flashing on every keystroke.
    @State private var resultsFor = ""

    var body: some View {
        Group {
            if query.trimmingCharacters(in: .whitespaces).isEmpty {
                conversationList
            } else {
                searchResults
            }
        }
        .searchable(text: $query, prompt: "Search messages")
        .task(id: query) {
            try? await Task.sleep(for: .milliseconds(200))
            guard !Task.isCancelled else { return }
            let found = await model.search(query)
            guard !Task.isCancelled else { return }
            results = found
            resultsFor = query
        }
        .navigationTitle("Chats")
        .toolbar {
            // Conversations are made on a chat server; a device's comes
            // with trusting it.
            if model.account != nil {
                ToolbarItem(placement: .primaryAction) {
                    Button("New Conversation", systemImage: "square.and.pencil") { creating = true }
                        .accessibilityIdentifier("chats.new")
                }
            }
        }
        .sheet(isPresented: $creating) {
            NewConversationView(model: model) { id in
                creating = false
                open(ChatRoute(conversationId: id))
            }
        }
    }

    private var searchResults: some View {
        List(results, id: \.id) { message in
            NavigationLink(
                value: ChatRoute(conversationId: message.conversationId, messageId: message.id)
            ) {
                SearchResultRow(
                    message: message, roomTitle: model.title(of: message.conversationId),
                    query: query)
            }
        }
        .overlay {
            if results.isEmpty {
                if resultsFor == query {
                    ForgeStateView.noResults(for: query)
                } else {
                    ForgeStateView.loading()
                }
            }
        }
    }

    /// A notice as a row of its own, drawn on the list's background.
    private func notice(_ notice: ForgeNotice) -> some View {
        notice
            .listRowInsets(EdgeInsets(top: 4, leading: 16, bottom: 4, trailing: 16))
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
    }

    private var conversationList: some View {
        List {
            if model.waitingCount > 0 {
                notice(ForgeNotice(
                    model.waitingCount == 1
                        ? "1 message is waiting to be sent. It goes out when the server can be reached."
                        : "\(model.waitingCount) messages are waiting to be sent. They go out when the server can be reached.",
                    systemImage: "tray.and.arrow.up", tone: .info))
            }
            if let problem = model.accountProblem {
                notice(ForgeNotice(
                    "The server no longer accepts this account (\(problem)). Sign out in Settings.",
                    systemImage: "person.crop.circle.badge.exclamationmark"))
            } else if let error = model.listError {
                notice(ForgeNotice(error, systemImage: "wifi.exclamationmark"))
            }
            ForEach(model.sortedConversations, id: \.id) { conversation in
                NavigationLink(value: ChatRoute(conversationId: conversation.id)) {
                    ConversationRow(conversation: conversation)
                }
                .accessibilityIdentifier("chats.conversation.\(conversation.id)")
                .swipeActions(edge: .leading) { readAction(conversation) }
                .swipeActions(edge: .trailing) { muteAction(conversation) }
                .contextMenu {
                    readAction(conversation)
                    muteAction(conversation)
                }
            }
        }
        .overlay {
            if model.conversations.isEmpty && model.listError == nil {
                if model.hasListed {
                    ForgeStateView.empty(
                        "No Conversations", systemImage: "bubble.left.and.bubble.right",
                        description: "Start one with the New Conversation button.")
                } else {
                    ForgeStateView.loading()
                }
            }
        }
        .refreshable { await model.refresh() }
    }

    @ViewBuilder private func readAction(_ conversation: ChatConversation) -> some View {
        if conversation.showsUnread {
            Button("Mark as Read", systemImage: "envelope.open") {
                Task { await model.markRead(conversation.id) }
            }
            .tint(.blue)
        } else {
            Button("Mark as Unread", systemImage: "envelope.badge") {
                model.markUnread(conversation.id)
            }
            .tint(.blue)
        }
    }

    @ViewBuilder private func muteAction(_ conversation: ChatConversation) -> some View {
        if conversation.muted {
            Button("Unmute", systemImage: "bell") { model.setMuted(conversation.id, false) }
                .tint(.indigo)
        } else {
            Button("Mute", systemImage: "bell.slash") { model.setMuted(conversation.id, true) }
                .tint(.indigo)
        }
    }
}

/// One row: unread dot, title, time, and the last message.
private struct ConversationRow: View {
    let conversation: ChatConversation

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Circle()
                .fill(.tint)
                .frame(width: 10, height: 10)
                .padding(.top, 6)
                .opacity(conversation.showsUnread ? 1 : 0)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                HStack(alignment: .firstTextBaseline) {
                    Text(conversation.title)
                        .font(.headline)
                    if conversation.muted {
                        Image(systemName: "bell.slash.fill")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .accessibilityHidden(true)
                    }
                    Spacer()
                    if let at = activity {
                        Text(Date(unixMilliseconds: at).chatListLabel)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                }
                Text(preview)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityText)
    }

    private var activity: Int64? {
        conversation.lastMessage?.sentAt ?? conversation.lastMessageAt
    }

    private var preview: String {
        guard let last = conversation.lastMessage else { return "No messages yet" }
        let who = last.isMine ? "You" : last.author
        if last.deleted { return "\(who): Message deleted" }
        switch last.status {
        case .failed: return "Not sent: \(last.summary)"
        case .sending, .sent: return "\(who): \(last.summary)"
        }
    }

    private var accessibilityText: String {
        var parts = [conversation.title]
        if conversation.unreadCount > 0 {
            parts.append("\(conversation.unreadCount) unread")
        } else if conversation.markedUnread {
            parts.append("marked unread")
        }
        if conversation.muted { parts.append("muted") }
        parts.append(preview)
        return parts.joined(separator: ", ")
    }
}

/// Starts a conversation on the server. Stays open, with the title, if that
/// fails.
private struct NewConversationView: View {
    let model: ChatModel
    let created: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var title = ""
    @State private var working = false
    @State private var error: ForgeFailure?
    @FocusState private var focused: Bool

    private var trimmed: String { title.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Title", text: $title)
                        .focused($focused)
                        .accessibilityIdentifier("newConversation.title")
                        .submitLabel(.done)
                        .onSubmit(create)
                } footer: {
                    Text("Everyone with an account on this server can see and join it.")
                }
            }
            .navigationTitle("New Conversation")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .forgeFormToolbar(
                id: "newConversation", confirmTitle: "Create", canConfirm: !trimmed.isEmpty,
                isWorking: working, hasChanges: !trimmed.isEmpty, cancel: { dismiss() }, confirm: create
            )
            .forgeAlert(failure: $error)
            .onAppear { focused = true }
        }
    }

    private func create() {
        guard !trimmed.isEmpty, !working else { return }
        working = true
        Task {
            defer { working = false }
            do {
                created(try await model.createConversation(title: trimmed))
            } catch {
                self.error = ChatModel.failure(error, title: "Can't Create the Conversation")
            }
        }
    }
}

extension Date {
    /// A date from Rust's Unix milliseconds.
    init(unixMilliseconds: Int64) {
        self.init(timeIntervalSince1970: TimeInterval(unixMilliseconds) / 1000)
    }

    /// The list's time column, as Messages shows it: the time today,
    /// "Yesterday", the weekday within a week, else the date.
    var chatListLabel: String {
        let calendar = Calendar.current
        if calendar.isDateInToday(self) {
            return formatted(date: .omitted, time: .shortened)
        }
        if calendar.isDateInYesterday(self) {
            return String(localized: "Yesterday")
        }
        if let days = calendar.dateComponents([.day], from: self, to: .now).day, days < 7 {
            return formatted(.dateTime.weekday(.wide))
        }
        return formatted(date: .numeric, time: .omitted)
    }
}
