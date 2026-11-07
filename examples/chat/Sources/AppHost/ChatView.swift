@preconcurrency import ForgeFFI
import SwiftUI

#if canImport(UIKit)
    import UIKit
#elseif canImport(AppKit)
    import AppKit
#endif

/// One conversation. Goes live on appear (which also marks it read, here
/// and on the server), leaves on disappear, and sends from the composer
/// pinned above the keyboard. Failed messages are red: tap one to retry.
struct ChatView: View {
    let model: ChatModel
    let conversationId: String
    @State private var draft = ""
    @FocusState private var composing: Bool

    private var messages: [ChatMessage] { model.messages(in: conversationId) }
    private var muted: Bool { model.conversation(conversationId)?.muted ?? false }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 2) {
                    ForEach(Transcript.rows(messages)) { row in
                        switch row.kind {
                        case .separator(let date):
                            Text(date.transcriptLabel)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .padding(.top, 12)
                                .padding(.bottom, 4)
                        case .message(let message, let place):
                            MessageBubble(
                                message: message,
                                place: place,
                                delivery: place.showsDelivery ? model.delivery(of: message) : nil
                            ) {
                                Task { await model.retry(message) }
                            }
                        }
                    }
                    Color.clear.frame(height: 1).id(Self.bottom)
                }
                .padding(.horizontal)
                .padding(.vertical, 8)
            }
            .defaultScrollAnchor(.bottom)
            .scrollDismissesKeyboard(.interactively)
            .onChange(of: messages.last?.id) { _, _ in
                withAnimation { proxy.scrollTo(Self.bottom, anchor: .bottom) }
            }
            .onChange(of: composing) { _, focused in
                // The keyboard is coming up: keep the newest message in view.
                if focused {
                    withAnimation { proxy.scrollTo(Self.bottom, anchor: .bottom) }
                }
            }
        }
        .safeAreaInset(edge: .top, spacing: 0) {
            ConnectionBanner(state: model.connection[conversationId] ?? .closed)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            Composer(
                draft: $draft,
                focused: $composing,
                problem: model.sendError[conversationId],
                send: send
            )
        }
        .navigationTitle(model.title(of: conversationId))
        #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Menu("More", systemImage: "ellipsis.circle") {
                    if muted {
                        Button("Unmute", systemImage: "bell") {
                            model.setMuted(conversationId, false)
                        }
                    } else {
                        Button("Mute", systemImage: "bell.slash") {
                            model.setMuted(conversationId, true)
                        }
                    }
                }
            }
        }
        .sensoryFeedback(.impact(weight: .light), trigger: model.sentCount)
        .sensoryFeedback(.error, trigger: model.failedCount)
        .task { await model.enter(conversationId) }
        .onDisappear { model.leave(conversationId) }
    }

    private static let bottom = "bottom"

    private func send() {
        let text = draft
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        draft = ""
        // Not tied to the view: leaving the room does not cancel the send.
        Task { await model.send(text, to: conversationId) }
    }
}

/// The transcript's rows: messages grouped by author and time, with a time
/// separator at each new day or after an hour's quiet, as Messages does.
enum Transcript {
    /// Messages from the same author closer together than this form a group.
    static let groupGap: Int64 = 5 * 60 * 1000
    /// A pause longer than this gets a time separator.
    static let separatorGap: Int64 = 60 * 60 * 1000

    /// Where a bubble sits in its group.
    struct Place: Equatable {
        var startsGroup: Bool
        var endsGroup: Bool
        /// Show the author's name above it (others' messages only).
        var showsAuthor: Bool
        /// Show sending, delivered, read or failed under it: every one of
        /// ours still sending or failed, and the newest one we sent.
        var showsDelivery: Bool
    }

    struct Row: Identifiable {
        enum Kind {
            case separator(Date)
            case message(ChatMessage, Place)
        }
        let id: String
        let kind: Kind
    }

    static func rows(_ messages: [ChatMessage], calendar: Calendar = .current) -> [Row] {
        let newestSentMine = messages.lastIndex { $0.isMine && $0.status == .sent }
        var rows: [Row] = []
        for (index, message) in messages.enumerated() {
            let previous = index > 0 ? messages[index - 1] : nil
            let next = index + 1 < messages.count ? messages[index + 1] : nil
            let separated = needsSeparator(before: message, after: previous, calendar: calendar)
            if separated {
                rows.append(
                    Row(
                        id: "separator-\(message.id)",
                        kind: .separator(Date(unixMilliseconds: message.sentAt))))
            }
            let starts = separated || !sameGroup(previous, message)
            let ends =
                next.map {
                    needsSeparator(before: $0, after: message, calendar: calendar)
                        || !sameGroup(message, $0)
                } ?? true
            let place = Place(
                startsGroup: starts,
                endsGroup: ends,
                showsAuthor: starts && !message.isMine,
                showsDelivery: message.isMine
                    && (message.status != .sent || index == newestSentMine)
            )
            rows.append(Row(id: message.id, kind: .message(message, place)))
        }
        return rows
    }

    static func needsSeparator(
        before message: ChatMessage, after previous: ChatMessage?, calendar: Calendar
    ) -> Bool {
        guard let previous else { return true }
        let (a, b) = (Date(unixMilliseconds: previous.sentAt), Date(unixMilliseconds: message.sentAt))
        return !calendar.isDate(a, inSameDayAs: b) || message.sentAt - previous.sentAt > separatorGap
    }

    static func sameGroup(_ a: ChatMessage?, _ b: ChatMessage) -> Bool {
        guard let a else { return false }
        let sameAuthor =
            a.isMine == b.isMine
            && (a.authorId.isEmpty || b.authorId.isEmpty
                ? a.author == b.author : a.authorId == b.authorId)
        return sameAuthor && b.sentAt - a.sentAt <= groupGap
    }
}

private struct ConnectionBanner: View {
    let state: ConnectionState

    var body: some View {
        switch state {
        case .connected:
            EmptyView()
        case .connecting:
            banner("Connecting…", systemImage: "antenna.radiowaves.left.and.right")
        case .reconnecting:
            banner(
                "Reconnecting… Messages you send now will fail; tap them to retry.",
                systemImage: "wifi.exclamationmark")
        case .closed:
            banner("Not connected", systemImage: "wifi.slash")
        }
    }

    private func banner(_ text: LocalizedStringKey, systemImage: String) -> some View {
        Label(text, systemImage: systemImage)
            .font(.footnote)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 6)
            .padding(.horizontal)
            .background(.bar)
    }
}

/// The text field and send button, pinned above the keyboard.
private struct Composer: View {
    @Binding var draft: String
    var focused: FocusState<Bool>.Binding
    let problem: String?
    let send: () -> Void

    private var empty: Bool { draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if let problem {
                Label(problem, systemImage: "exclamationmark.circle")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            HStack(alignment: .bottom, spacing: 8) {
                TextField("Message", text: $draft, axis: .vertical)
                    .lineLimit(1...6)
                    .focused(focused)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .overlay {
                        RoundedRectangle(cornerRadius: 18, style: .continuous)
                            .strokeBorder(.quaternary)
                    }
                Button("Send", systemImage: "arrow.up.circle.fill", action: send)
                    .labelStyle(.iconOnly)
                    .font(.title)
                    .disabled(empty)
                    .keyboardShortcut(.return, modifiers: .command)
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(.bar)
    }
}

private struct MessageBubble: View {
    let message: ChatMessage
    let place: Transcript.Place
    let delivery: ChatDelivery?
    let retry: () -> Void

    var body: some View {
        HStack {
            if message.isMine { Spacer(minLength: 48) }
            VStack(alignment: message.isMine ? .trailing : .leading, spacing: 2) {
                if place.showsAuthor {
                    Text(message.author)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 12)
                }
                Text(message.body)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .foregroundStyle(message.isMine ? Color.white : Color.primary)
                    .background(
                        RoundedRectangle(cornerRadius: 18, style: .continuous).fill(fill)
                    )
                    .contextMenu {
                        Button("Copy", systemImage: "doc.on.doc") { copy(message.body) }
                        if message.status == .failed {
                            Button("Try Again", systemImage: "arrow.clockwise", action: retry)
                        }
                    }
                if let delivery {
                    DeliveryLabel(delivery: delivery)
                }
            }
            if !message.isMine { Spacer(minLength: 48) }
        }
        .padding(.top, place.startsGroup ? 6 : 0)
        .contentShape(Rectangle())
        .onTapGesture {
            if message.status == .failed { retry() }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityText)
        .accessibilityHint(message.status == .failed ? "Double-tap to send again" : "")
    }

    private var fill: Color {
        guard message.isMine else { return .incomingBubble }
        switch message.status {
        case .sending: return .accentColor.opacity(0.6)
        case .sent: return .accentColor
        case .failed: return .red
        }
    }

    private var accessibilityText: String {
        let who = message.isMine ? String(localized: "You") : message.author
        let time = Date(unixMilliseconds: message.sentAt).formatted(date: .omitted, time: .shortened)
        var text = "\(who), \(message.body), \(time)"
        if let delivery { text += ", \(DeliveryLabel.text(delivery))" }
        return text
    }
}

private struct DeliveryLabel: View {
    let delivery: ChatDelivery

    var body: some View {
        Text(Self.text(delivery))
            .font(.caption2)
            .foregroundStyle(delivery == .failed ? Color.red : Color.secondary)
            .padding(.horizontal, 4)
    }

    static func text(_ delivery: ChatDelivery) -> String {
        switch delivery {
        case .sending: return String(localized: "Sending…")
        case .sent: return String(localized: "Delivered")
        case .read: return String(localized: "Read")
        case .failed: return String(localized: "Not Delivered. Tap to Try Again.")
        }
    }
}

private func copy(_ text: String) {
    #if canImport(UIKit)
        UIPasteboard.general.string = text
    #elseif canImport(AppKit)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    #endif
}

extension Color {
    /// Others' bubbles: the grey Messages uses, in light and dark mode.
    static var incomingBubble: Color {
        #if canImport(UIKit)
            Color(uiColor: .systemGray5)
        #else
            Color.secondary.opacity(0.2)
        #endif
    }
}

extension Date {
    /// A transcript separator: "Today 14:05", "Yesterday 09:12", or the
    /// date and time.
    var transcriptLabel: String {
        let time = formatted(date: .omitted, time: .shortened)
        let calendar = Calendar.current
        if calendar.isDateInToday(self) { return String(localized: "Today \(time)") }
        if calendar.isDateInYesterday(self) { return String(localized: "Yesterday \(time)") }
        return formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated).hour().minute())
    }
}
