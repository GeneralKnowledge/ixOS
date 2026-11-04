import ChatBridge
import SwiftUI

/// Screen 1: rooms, with unread badges. Pull to refresh re-lists.
struct ConversationListView: View {
    @ObservedObject var session: ChatSession

    var body: some View {
        List {
            if let error = session.listError {
                Label(error, systemImage: "wifi.exclamationmark")
                    .foregroundStyle(.red)
                    .font(.callout)
            }
            ForEach(session.conversations, id: \.id) { conversation in
                NavigationLink(value: conversation.id) {
                    ConversationRow(conversation: conversation)
                }
            }
        }
        .overlay {
            if session.conversations.isEmpty && session.listError == nil {
                ProgressView()
            }
        }
        .refreshable { await session.refresh() }
        .navigationTitle("Chats")
        .navigationDestination(for: String.self) { conversationId in
            ChatView(session: session, conversationId: conversationId)
        }
        .task { await session.start() }
    }
}

private struct ConversationRow: View {
    let conversation: ChatConversation

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(conversation.title).font(.headline)
                if let date = conversation.lastMessageDate {
                    Text(date, format: .relative(presentation: .named))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
            if conversation.unreadCount > 0 {
                Text("\(conversation.unreadCount)")
                    .font(.caption.bold())
                    .padding(.horizontal, 8)
                    .padding(.vertical, 2)
                    .background(Capsule().fill(.blue))
                    .foregroundStyle(.white)
                    .accessibilityLabel("\(conversation.unreadCount) unread")
            }
        }
    }
}
