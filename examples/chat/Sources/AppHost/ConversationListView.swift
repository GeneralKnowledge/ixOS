@preconcurrency import ForgeFFI
import SwiftUI

/// The conversations, with unread badges. Pull to refresh re-lists them.
struct ConversationListView: View {
    let model: ChatModel
    @State private var showingSettings = false

    var body: some View {
        List {
            if let problem = model.accountProblem {
                Label(
                    "The server no longer accepts this account (\(problem)). Open Settings to sign out.",
                    systemImage: "person.crop.circle.badge.exclamationmark"
                )
                .foregroundStyle(.red)
                .font(.callout)
            } else if let error = model.listError {
                Label(error, systemImage: "wifi.exclamationmark")
                    .foregroundStyle(.red)
                    .font(.callout)
            }
            ForEach(model.conversations, id: \.id) { conversation in
                NavigationLink(value: conversation.id) {
                    ConversationRow(conversation: conversation)
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
        .navigationDestination(for: String.self) { conversationId in
            ChatView(model: model, conversationId: conversationId)
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    showingSettings = true
                } label: {
                    Label("Settings", systemImage: "gearshape")
                }
            }
        }
        .sheet(isPresented: $showingSettings) {
            SettingsView(model: model)
        }
        .errorAlert("Something Went Wrong", message: Bindable(model).actionError)
    }
}

private struct ConversationRow: View {
    let conversation: ChatConversation

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(conversation.title).font(.headline)
                if let at = conversation.lastMessageAt {
                    Text(Date(unixMilliseconds: at), format: .relative(presentation: .named))
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
                    .background(Capsule().fill(.tint))
                    .foregroundStyle(.white)
                    .accessibilityLabel("\(conversation.unreadCount) unread")
            }
        }
    }
}

extension Date {
    /// A date from Rust's Unix milliseconds.
    init(unixMilliseconds: Int64) {
        self.init(timeIntervalSince1970: TimeInterval(unixMilliseconds) / 1000)
    }
}
