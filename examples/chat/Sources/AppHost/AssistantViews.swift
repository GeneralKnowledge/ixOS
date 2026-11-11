@preconcurrency import ChatFFI
import SwiftUI

/// The Assistant tab (and the sign-up screen's Assistant sheet): private
/// conversations with a model that runs on this iPhone, no account needed.
struct AssistantTab: View {
    let model: AssistantModel
    @State private var path: [String] = []

    var body: some View {
        NavigationStack(path: $path) {
            AssistantListView(model: model) { path.append($0) }
                .navigationDestination(for: String.self) { id in
                    AssistantChatView(model: model, conversationId: id)
                }
        }
        // One alert for the list and the open conversation.
        .errorAlert("Assistant", message: Bindable(model).actionError)
    }
}

/// The assistant conversations, most recent first. New starts one with the
/// plain assistant or a character.
struct AssistantListView: View {
    let model: AssistantModel
    let open: (String) -> Void

    var body: some View {
        List {
            if let engine = model.engine, !engine.available {
                Section {
                    Label(
                        engine.unavailableReason ?? "\(engine.name) is not available.",
                        systemImage: "exclamationmark.triangle"
                    )
                    .accessibilityIdentifier("assistant.unavailable")
                }
            }
            if model.conversations.isEmpty {
                ContentUnavailableView {
                    Label("No Assistant Chats", systemImage: "sparkles")
                } description: {
                    Text(
                        "Chat with a model that runs on this iPhone. Nothing you write leaves the phone."
                    )
                } actions: {
                    newMenu(title: "Start a Chat")
                        .buttonStyle(.borderedProminent)
                }
                .listRowBackground(Color.clear)
            }
            ForEach(model.conversations, id: \.id) { conversation in
                Button {
                    open(conversation.id)
                } label: {
                    AssistantRow(conversation: conversation, writing: model.isWriting(conversation.id))
                }
                .foregroundStyle(.primary)
                .accessibilityIdentifier("assistant.conversation.\(conversation.id)")
                .swipeActions {
                    Button("Delete", systemImage: "trash", role: .destructive) {
                        model.delete(conversation.id)
                    }
                }
                .contextMenu {
                    Button("Delete", systemImage: "trash", role: .destructive) {
                        model.delete(conversation.id)
                    }
                }
            }
        }
        .navigationTitle("Assistant")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                newMenu(title: "New Assistant Chat")
                    .labelStyle(.iconOnly)
            }
        }
        .onAppear { model.refresh() }
    }

    /// Picks who to talk to.
    private func newMenu(title: String) -> some View {
        Menu {
            ForEach(model.characters, id: \.id) { character in
                Button {
                    if let id = model.start(characterId: character.id) { open(id) }
                } label: {
                    Text("\(character.emoji) \(character.name)")
                    Text(character.description)
                }
                .accessibilityIdentifier("assistant.character.\(character.id)")
            }
        } label: {
            Label(title, systemImage: "square.and.pencil")
        }
        .accessibilityIdentifier("assistant.new")
    }
}

private struct AssistantRow: View {
    let conversation: AssistantConversation
    let writing: Bool

    var body: some View {
        HStack(spacing: 12) {
            Text(conversation.character.emoji)
                .font(.title)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text(conversation.character.name).font(.headline)
                    Spacer()
                    Text(Date(unixMilliseconds: conversation.updatedAt).chatListLabel)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Text(preview)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var preview: String {
        if writing { return "Writing…" }
        guard let last = conversation.lastMessage, !last.text.isEmpty else {
            return conversation.character.description
        }
        return last.role == .user ? "You: \(last.text)" : last.text
    }
}

/// One assistant conversation: the bubbles, the answer as it is written, and
/// a composer whose Send button becomes Stop while the model writes.
struct AssistantChatView: View {
    let model: AssistantModel
    let conversationId: String
    @State private var draft = ""
    @FocusState private var composerFocused: Bool

    private var messages: [AssistantMessage] { model.messages[conversationId] ?? [] }
    private var writing: Bool { model.isWriting(conversationId) }
    private var conversation: AssistantConversation? { model.conversation(conversationId) }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 8) {
                    if let engine = model.engine {
                        Text("\(engine.name) · on this iPhone, offline")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .padding(.top, 8)
                    }
                    ForEach(messages, id: \.id) { message in
                        AssistantBubble(
                            message: message, liveText: model.liveText[message.id],
                            character: conversation?.character
                        )
                        .id(message.id)
                    }
                }
                .padding(.horizontal)
                .padding(.bottom, 8)
            }
            .defaultScrollAnchor(.bottom)
            .scrollDismissesKeyboard(.interactively)
            .onChange(of: scrollKey) {
                if let last = messages.last?.id { proxy.scrollTo(last, anchor: .bottom) }
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) { composer }
        .navigationTitle(title)
        #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
        #endif
        .onAppear { model.open(conversationId) }
    }

    private var title: String {
        guard let character = conversation?.character else { return "Assistant" }
        return "\(character.emoji) \(character.name)"
    }

    /// Changes whenever the transcript grows, to keep the newest text visible.
    private var scrollKey: Int {
        messages.count + (messages.last.flatMap { model.liveText[$0.id]?.count } ?? 0)
    }

    private var composer: some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextField("Message", text: $draft, axis: .vertical)
                .lineLimit(1...6)
                .focused($composerFocused)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .overlay {
                    RoundedRectangle(cornerRadius: 18, style: .continuous)
                        .strokeBorder(.quaternary)
                }
                .accessibilityIdentifier("assistantChat.composer")
            if writing {
                Button("Stop", systemImage: "stop.circle.fill") { model.stop() }
                    .labelStyle(.iconOnly)
                    .font(.title)
                    .accessibilityIdentifier("assistantChat.stop")
            } else {
                Button("Send", systemImage: "arrow.up.circle.fill", action: send)
                    .labelStyle(.iconOnly)
                    .font(.title)
                    .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .accessibilityIdentifier("assistantChat.send")
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(.bar)
    }

    private func send() {
        let text = draft
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        if model.send(text, in: conversationId) { draft = "" }
    }
}

private struct AssistantBubble: View {
    let message: AssistantMessage
    /// The text so far while this answer is being written.
    let liveText: String?
    let character: AssistantCharacter?

    private var mine: Bool { message.role == .user }
    private var text: String { liveText ?? message.text }

    var body: some View {
        HStack {
            if mine { Spacer(minLength: 48) }
            VStack(alignment: mine ? .trailing : .leading, spacing: 2) {
                content
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .foregroundStyle(mine ? Color.white : Color.primary)
                    .background(
                        mine ? AnyShapeStyle(.tint) : AnyShapeStyle(.fill.tertiary),
                        in: RoundedRectangle(cornerRadius: 18, style: .continuous)
                    )
                    .textSelection(.enabled)
                if let note {
                    Label(note.text, systemImage: note.symbol)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 4)
                }
            }
            if !mine { Spacer(minLength: 48) }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityText)
        .accessibilityIdentifier("assistantChat.message.\(message.id)")
    }

    @ViewBuilder private var content: some View {
        if text.isEmpty && message.state == .streaming {
            ProgressView()
                .accessibilityLabel("Thinking")
        } else if text.isEmpty {
            Text(emptyText)
                .italic()
        } else {
            Text(text)
        }
    }

    /// An answer that ended before any text was written.
    private var emptyText: String {
        switch message.state {
        case .interrupted: return "No answer was written"
        case .stopped: return "Stopped before any text"
        case .refused, .failed: return "No answer"
        case .done, .streaming: return "No text"
        }
    }

    private var note: (text: String, symbol: String)? {
        switch message.state {
        case .done, .streaming:
            guard let stats = message.stats, liveText == nil else { return nil }
            return (Self.timing(stats), "sparkles")
        case .stopped: return ("Stopped", "stop.circle")
        case .interrupted: return ("Interrupted when the app closed", "exclamationmark.circle")
        case .refused: return (message.error ?? "Declined to answer", "hand.raised")
        case .failed: return (message.error ?? "Failed", "exclamationmark.triangle")
        }
    }

    /// "First words 0.8 s · 3.2 s": what the model took, for the L1 phone
    /// run and the resource monitor to come (L4).
    static func timing(_ stats: AssistantStats) -> String {
        let total = Double(stats.durationMs) / 1000
        guard let first = stats.firstTextMs else { return String(format: "%.1f s", total) }
        return String(format: "First words %.1f s · %.1f s", Double(first) / 1000, total)
    }

    private var accessibilityText: String {
        let who = mine ? "You" : (character?.name ?? "Assistant")
        let state = note.map { ", \($0.text)" } ?? ""
        return "\(who): \(text.isEmpty ? "writing" : text)\(state)"
    }
}
