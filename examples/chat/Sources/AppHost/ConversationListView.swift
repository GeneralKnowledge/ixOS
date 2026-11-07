@preconcurrency import ForgeFFI
import SwiftUI

/// The Chats tab: its own navigation stack, so a new conversation can open
/// as soon as it is created.
struct ChatsTab: View {
    let model: ChatModel
    @State private var path: [String] = []

    var body: some View {
        NavigationStack(path: $path) {
            ConversationListView(model: model) { path.append($0) }
                .navigationDestination(for: String.self) { conversationId in
                    ChatView(model: model, conversationId: conversationId)
                }
        }
    }
}

/// The conversations, most recent first, with a preview of the last message
/// and an unread dot. Swipe to mark read or unread, or to mute. Pull to
/// refresh re-lists them.
struct ConversationListView: View {
    let model: ChatModel
    /// Opens a conversation (after creating one).
    let open: (String) -> Void
    @State private var creating = false

    var body: some View {
        List {
            if let problem = model.accountProblem {
                Label(
                    "The server no longer accepts this account (\(problem)). Sign out in Settings.",
                    systemImage: "person.crop.circle.badge.exclamationmark"
                )
                .font(.callout)
                .foregroundStyle(.secondary)
            } else if let error = model.listError {
                Label(error, systemImage: "wifi.exclamationmark")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            ForEach(model.sortedConversations, id: \.id) { conversation in
                NavigationLink(value: conversation.id) {
                    ConversationRow(conversation: conversation)
                }
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
                ProgressView()
            }
        }
        .refreshable { await model.refresh() }
        .navigationTitle("Chats")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("New Conversation", systemImage: "square.and.pencil") { creating = true }
            }
        }
        .sheet(isPresented: $creating) {
            NewConversationView(model: model) { id in
                creating = false
                open(id)
            }
        }
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
        switch last.status {
        case .failed: return "Not sent: \(last.body)"
        case .sending, .sent: return "\(who): \(last.body)"
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
    @State private var error: String?
    @FocusState private var focused: Bool

    private var trimmed: String { title.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Title", text: $title)
                        .focused($focused)
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
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if working {
                        ProgressView()
                    } else {
                        Button("Create", action: create).disabled(trimmed.isEmpty)
                    }
                }
            }
            .interactiveDismissDisabled(!trimmed.isEmpty)
            .errorAlert("Can't Create the Conversation", message: $error)
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
                self.error = ChatModel.describe(error)
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
