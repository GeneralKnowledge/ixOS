import ChatBridge
import SwiftUI

/// Screen 2: one room. Connects on appear, disconnects on disappear, sends on
/// submit, and shows `.failed` messages in red: tap one to retry.
struct ChatView: View {
    @ObservedObject var session: ChatSession
    let conversationId: String
    @State private var draft = ""

    private var messages: [ChatMessage] { session.messages(in: conversationId) }

    var body: some View {
        VStack(spacing: 0) {
            ConnectionBanner(state: session.connection[conversationId] ?? .closed)
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 6) {
                        ForEach(messages, id: \.id) { message in
                            MessageBubble(
                                message: message,
                                isMine: message.author == "me"
                            ) {
                                Task { await session.retry(message) }
                            }
                            .id(message.id)
                        }
                    }
                    .padding()
                }
                .onChange(of: messages.last?.id) { _, last in
                    if let last {
                        withAnimation { proxy.scrollTo(last, anchor: .bottom) }
                    }
                }
            }
            if let error = session.sendError[conversationId] {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .padding(.horizontal)
            }
            HStack {
                TextField("Message", text: $draft)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit(send)
                Button("Send", action: send)
                    .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .padding()
        }
        .navigationTitle(session.title(of: conversationId))
        #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
        #endif
        .task { await session.open(conversationId) }
        .onDisappear { session.close(conversationId) }
    }

    private func send() {
        let text = draft
        draft = ""
        // Not tied to the view: leaving the room does not cancel the send.
        Task { await session.send(text, to: conversationId) }
    }
}

private struct ConnectionBanner: View {
    let state: ConnectionState

    var body: some View {
        switch state {
        case .connected:
            EmptyView()
        case .connecting:
            banner("Connecting…", color: .gray)
        case .reconnecting:
            banner("Reconnecting… messages you send now will fail; tap them to retry.", color: .orange)
        case .closed:
            banner("Not connected", color: .gray)
        }
    }

    private func banner(_ text: String, color: Color) -> some View {
        Text(text)
            .font(.caption)
            .frame(maxWidth: .infinity)
            .padding(6)
            .background(color.opacity(0.2))
    }
}

private struct MessageBubble: View {
    let message: ChatMessage
    let isMine: Bool
    let retry: () -> Void

    var body: some View {
        HStack {
            if isMine { Spacer(minLength: 40) }
            VStack(alignment: isMine ? .trailing : .leading, spacing: 2) {
                if !isMine {
                    Text(message.author).font(.caption2).foregroundStyle(.secondary)
                }
                Text(message.body)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(RoundedRectangle(cornerRadius: 14).fill(fill))
                    .foregroundStyle(isMine ? Color.white : Color.primary)
                status
            }
            if !isMine { Spacer(minLength: 40) }
        }
        .contentShape(Rectangle())
        .onTapGesture {
            if message.status == .failed { retry() }
        }
        .accessibilityHint(message.status == .failed ? "Double-tap to send again" : "")
    }

    private var fill: Color {
        guard isMine else { return Color.gray.opacity(0.2) }
        switch message.status {
        case .sending: return .blue.opacity(0.5)
        case .sent: return .blue
        case .failed: return .red
        }
    }

    @ViewBuilder private var status: some View {
        switch message.status {
        case .sending:
            Label("Sending", systemImage: "clock").font(.caption2).foregroundStyle(.secondary)
        case .sent:
            EmptyView()
        case .failed:
            Label("Not sent. Tap to retry.", systemImage: "exclamationmark.circle")
                .font(.caption2)
                .foregroundStyle(.red)
        }
    }
}
