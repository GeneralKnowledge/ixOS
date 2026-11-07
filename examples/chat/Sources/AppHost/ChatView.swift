@preconcurrency import ForgeFFI
import PhotosUI
import QuickLook
import SwiftUI
import UniformTypeIdentifiers

#if canImport(UIKit)
    import UIKit
#elseif canImport(AppKit)
    import AppKit
#endif

/// One conversation. Goes live on appear (which also marks it read, here
/// and on the server), leaves on disappear, and sends from the composer
/// pinned above the keyboard. Touch and hold a bubble to react, reply,
/// copy, edit or delete. The composer's text is kept as a draft. Failed
/// messages are red: tap one to retry.
struct ChatView: View {
    let model: ChatModel
    let conversationId: String
    /// A message to scroll to and highlight once the room is shown (from a
    /// search result).
    var focusMessage: String? = nil

    @State private var draft = ""
    @State private var draftLoaded = false
    @State private var replyingTo: ChatMessage?
    @State private var editing: ChatMessage?
    /// The draft put aside while editing, restored afterwards.
    @State private var stashedDraft = ""
    @State private var deleting: ChatMessage?
    @State private var searching = false
    @State private var jumpTarget: String?
    @State private var highlighted: String?
    /// Photos and documents picked for the next message.
    @State private var staged: [StagedFile] = []
    @State private var pickingPhotos = false
    @State private var photoItems: [PhotosPickerItem] = []
    @State private var pickingFiles = false
    /// The attachment open in Quick Look.
    @State private var previewURL: URL?
    /// Records voice messages (Forge's audio module).
    @State private var recorder = ForgeAudioRecorder(maxDuration: 300)
    @State private var microphoneDenied = false
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
                                model: model,
                                message: message,
                                place: place,
                                delivery: place.showsDelivery ? model.delivery(of: message) : nil,
                                quoted: quote(for: message),
                                highlighted: highlighted == message.id,
                                actions: actions(for: message)
                            )
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
                guard jumpTarget == nil, highlighted == nil else { return }
                withAnimation { proxy.scrollTo(Self.bottom, anchor: .bottom) }
            }
            .onChange(of: composing) { _, focused in
                // The keyboard is coming up: keep the newest message in view.
                if focused && editing == nil {
                    withAnimation { proxy.scrollTo(Self.bottom, anchor: .bottom) }
                }
            }
            .onChange(of: jumpTarget) { _, target in
                guard let target else { return }
                withAnimation { proxy.scrollTo(target, anchor: .center) }
                highlighted = target
                jumpTarget = nil
                Task {
                    try? await Task.sleep(for: .seconds(1.5))
                    withAnimation { if highlighted == target { highlighted = nil } }
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
                context: composerContext,
                problem: model.sendError[conversationId],
                staged: staged,
                recorder: recorder,
                cancel: cancelContext,
                pickPhotos: { pickingPhotos = true },
                pickFiles: { pickingFiles = true },
                unstage: { file in staged.removeAll { $0.id == file.id } },
                startRecording: startRecording,
                cancelRecording: { recorder.cancel() },
                sendRecording: { if let recording = recorder.stop() { sendVoice(recording) } },
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
                    Button("Search in Conversation", systemImage: "magnifyingglass") {
                        searching = true
                    }
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
        .sheet(isPresented: $searching) {
            MessageSearchSheet(model: model, conversationId: conversationId) { found in
                searching = false
                jumpTarget = found.id
            }
        }
        .confirmationDialog(
            deleting?.status == .sent ? "Delete this message for everyone?" : "Delete this unsent message?",
            isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
            titleVisibility: .visible,
            presenting: deleting
        ) { message in
            Button(message.status == .sent ? "Delete for Everyone" : "Delete", role: .destructive) {
                Task { await model.delete(message) }
            }
        }
        .photosPicker(
            isPresented: $pickingPhotos, selection: $photoItems,
            maxSelectionCount: MAX_ATTACHMENTS_PER_MESSAGE_SWIFT, matching: .images
        )
        .onChange(of: photoItems) { _, items in
            guard !items.isEmpty else { return }
            photoItems = []
            Task { await stagePhotos(items) }
        }
        .fileImporter(
            isPresented: $pickingFiles, allowedContentTypes: [.item],
            allowsMultipleSelection: true
        ) { result in
            stageDocuments(result)
        }
        .quickLookPreview($previewURL)
        .alert("Microphone Access Is Off", isPresented: $microphoneDenied) {
            #if os(iOS)
                Button("Open Settings") { ForgePermissions.openSettings() }
            #endif
            Button("OK", role: .cancel) {}
        } message: {
            Text("To record voice messages, allow Chat to use the microphone in Settings.")
        }
        .sensoryFeedback(.impact(weight: .light), trigger: model.sentCount)
        .sensoryFeedback(.error, trigger: model.failedCount)
        .task {
            if !draftLoaded {
                draft = model.draft(for: conversationId)
                draftLoaded = true
            }
            // A recording that ends by itself (the 5-minute limit, or a
            // phone call) is sent rather than lost.
            recorder.onFinish = { recording in
                if let recording { sendVoice(recording) }
            }
            await model.enter(conversationId)
            if let focusMessage { jumpTarget = focusMessage }
        }
        .task(id: draft) {
            // Saved half a second after the last keystroke (and on leaving).
            guard draftLoaded, editing == nil else { return }
            try? await Task.sleep(for: .milliseconds(500))
            guard !Task.isCancelled else { return }
            model.saveDraft(draft, for: conversationId)
        }
        .onDisappear {
            recorder.cancel()
            ForgeAudioPlayer.shared.stop()
            model.saveDraft(editing == nil ? draft : stashedDraft, for: conversationId)
            model.leave(conversationId)
        }
    }

    private static let bottom = "bottom"

    private var composerContext: Composer.Context? {
        if let editing { return .editing(editing) }
        if let replyingTo { return .replying(replyingTo) }
        return nil
    }

    private func quote(for message: ChatMessage) -> Quote? {
        guard let id = message.replyTo else { return nil }
        return Quote(id: id, original: model.message(id, in: conversationId))
    }

    private func actions(for message: ChatMessage) -> BubbleActions {
        BubbleActions(
            retry: { Task { await model.retry(message) } },
            reply: {
                editing = nil
                replyingTo = message
                composing = true
            },
            edit: {
                if editing == nil { stashedDraft = draft }
                replyingTo = nil
                editing = message
                draft = message.body
                composing = true
            },
            delete: { deleting = message },
            react: { emoji in Task { await model.toggleReaction(emoji, on: message) } },
            jump: { jumpTarget = $0 },
            open: { attachment in
                Task {
                    if let url = await model.previewFile(for: attachment) { previewURL = url }
                }
            },
            play: { attachment in
                Task {
                    guard let url = await model.localFile(for: attachment) else {
                        model.actionError = String(localized: "This voice message could not be downloaded.")
                        return
                    }
                    do {
                        try ForgeAudioPlayer.shared.toggle(url)
                    } catch {
                        model.actionError = error.localizedDescription
                    }
                }
            }
        )
    }

    /// Starts a voice message (asking for the microphone the first time).
    private func startRecording() {
        ForgeAudioPlayer.shared.stop()
        composing = false
        Task {
            do {
                try await recorder.start()
            } catch ForgeAudioError.microphoneDenied {
                microphoneDenied = true
            } catch {
                model.actionError = error.localizedDescription
            }
        }
    }

    /// Sends a recording as a voice message (an `audio/mp4` attachment),
    /// answering the message being replied to, if any.
    private func sendVoice(_ recording: ForgeAudioRecorder.Recording) {
        let file = ChatOutgoingFile(
            path: recording.url.path, name: AttachmentFiles.voiceMessageName, mime: "audio/mp4")
        let replyTo = replyingTo?.id
        replyingTo = nil
        Task {
            await model.send(files: [file], text: "", to: conversationId, replyTo: replyTo)
            // Rust has its own copy now.
            try? FileManager.default.removeItem(at: recording.url)
        }
    }

    /// Shrinks picked photos to JPEG files and adds them to the next message.
    private func stagePhotos(_ items: [PhotosPickerItem]) async {
        for item in items {
            guard staged.count < MAX_ATTACHMENTS_PER_MESSAGE_SWIFT else { break }
            do {
                guard let data = try await item.loadTransferable(type: Data.self) else { continue }
                staged.append(try await AttachmentFiles.preparePhoto(data))
            } catch {
                model.actionError = String(localized: "That photo couldn't be added (\(error.localizedDescription)).")
            }
        }
    }

    /// Copies picked documents and adds them to the next message.
    private func stageDocuments(_ result: Result<[URL], Error>) {
        switch result {
        case .success(let urls):
            for url in urls.prefix(MAX_ATTACHMENTS_PER_MESSAGE_SWIFT - staged.count) {
                do {
                    staged.append(try AttachmentFiles.prepareDocument(url))
                } catch {
                    model.actionError = String(localized: "\(url.lastPathComponent) couldn't be added (\(error.localizedDescription)).")
                }
            }
        case .failure(let error):
            model.actionError = error.localizedDescription
        }
    }

    private func cancelContext() {
        if editing != nil {
            editing = nil
            draft = stashedDraft
        }
        replyingTo = nil
    }

    private func send() {
        let text = draft
        if editing == nil && !staged.isEmpty {
            let files = staged.map(\.outgoing)
            let replyTo = replyingTo?.id
            staged = []
            replyingTo = nil
            draft = ""
            Task { await model.send(files: files, text: text, to: conversationId, replyTo: replyTo) }
            return
        }
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        if let message = editing {
            editing = nil
            draft = stashedDraft
            Task { await model.edit(message, to: text) }
            return
        }
        let replyTo = replyingTo?.id
        replyingTo = nil
        draft = ""
        // Not tied to the view: leaving the room does not cancel the send.
        Task { await model.send(text, to: conversationId, replyTo: replyTo) }
    }
}

/// Most files one message may carry (Rust's `MAX_ATTACHMENTS_PER_MESSAGE`).
let MAX_ATTACHMENTS_PER_MESSAGE_SWIFT = 10

/// The reactions offered at the top of a bubble's menu.
let quickReactions = ["👍", "❤️", "😂", "😮", "😢", "🙏"]

/// What a bubble's menu and taps do.
struct BubbleActions {
    var retry: () -> Void
    var reply: () -> Void
    var edit: () -> Void
    var delete: () -> Void
    var react: (String) -> Void
    /// Scrolls to another message (the one a reply quotes).
    var jump: (String) -> Void
    /// Opens an attachment in Quick Look.
    var open: (ChatAttachment) -> Void
    /// Plays or pauses a voice message.
    var play: (ChatAttachment) -> Void
}

/// The message a reply answers: `original` is nil when it is not loaded.
struct Quote {
    let id: String
    let original: ChatMessage?
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
                "Reconnecting… Messages you send now wait and go out when the connection is back.",
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

/// The text field and send button, pinned above the keyboard, with a bar
/// for the message being answered or edited.
private struct Composer: View {
    enum Context {
        case replying(ChatMessage)
        case editing(ChatMessage)
    }

    @Binding var draft: String
    var focused: FocusState<Bool>.Binding
    let context: Context?
    let problem: String?
    let staged: [StagedFile]
    let recorder: ForgeAudioRecorder
    let cancel: () -> Void
    let pickPhotos: () -> Void
    let pickFiles: () -> Void
    let unstage: (StagedFile) -> Void
    let startRecording: () -> Void
    let cancelRecording: () -> Void
    let sendRecording: () -> Void
    let send: () -> Void

    private var empty: Bool {
        draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && (isEditing || staged.isEmpty)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if let problem {
                Label(problem, systemImage: "exclamationmark.circle")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            if let context {
                contextBar(context)
            }
            if !staged.isEmpty && !isEditing {
                StagedFilesBar(files: staged, remove: unstage)
            }
            if recorder.isRecording {
                RecordingBar(recorder: recorder, cancel: cancelRecording, send: sendRecording)
            } else {
                editor
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(.bar)
    }

    /// Nothing to send yet: the send button becomes the microphone.
    private var offersRecording: Bool { empty && !isEditing && staged.isEmpty }

    private var editor: some View {
        HStack(alignment: .bottom, spacing: 8) {
            if !isEditing {
                Menu {
                    Button("Photo", systemImage: "photo.on.rectangle", action: pickPhotos)
                    Button("Document", systemImage: "doc", action: pickFiles)
                } label: {
                    Label("Attach", systemImage: "plus.circle.fill")
                        .labelStyle(.iconOnly)
                        .font(.title)
                        .foregroundStyle(.secondary)
                }
                .disabled(staged.count >= MAX_ATTACHMENTS_PER_MESSAGE_SWIFT)
            }
            TextField("Message", text: $draft, axis: .vertical)
                .lineLimit(1...6)
                .focused(focused)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .overlay {
                    RoundedRectangle(cornerRadius: 18, style: .continuous)
                        .strokeBorder(.quaternary)
                }
            if offersRecording {
                Button("Record Voice Message", systemImage: "mic.circle.fill", action: startRecording)
                    .labelStyle(.iconOnly)
                    .font(.title)
            } else {
                Button(isEditing ? "Save" : "Send", systemImage: isEditing ? "checkmark.circle.fill" : "arrow.up.circle.fill", action: send)
                    .labelStyle(.iconOnly)
                    .font(.title)
                    .disabled(empty)
                    .keyboardShortcut(.return, modifiers: .command)
            }
        }
    }

    private var isEditing: Bool {
        if case .editing = context { return true }
        return false
    }

    private func contextBar(_ context: Context) -> some View {
        HStack(spacing: 8) {
            Rectangle().fill(.tint).frame(width: 3)
            VStack(alignment: .leading, spacing: 1) {
                switch context {
                case .replying(let message):
                    Text("Replying to \(message.isMine ? String(localized: "yourself") : message.author)")
                        .font(.caption.bold())
                    Text(message.summary).font(.caption).lineLimit(1)
                case .editing(let message):
                    Text("Editing message").font(.caption.bold())
                    Text(message.body).font(.caption).lineLimit(1)
                }
            }
            .foregroundStyle(.secondary)
            Spacer()
            Button("Cancel", systemImage: "xmark.circle.fill", action: cancel)
                .labelStyle(.iconOnly)
                .foregroundStyle(.secondary)
        }
        .frame(maxHeight: 36)
    }
}

/// Shown instead of the text field while recording: discard, the time so
/// far with a live level meter, and send.
private struct RecordingBar: View {
    let recorder: ForgeAudioRecorder
    let cancel: () -> Void
    let send: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Button("Discard Recording", systemImage: "trash", role: .destructive, action: cancel)
                .labelStyle(.iconOnly)
                .font(.title3)
            Circle().fill(.red).frame(width: 10, height: 10)
            Text(AttachmentFiles.timeLabel(recorder.elapsed))
                .font(.body.monospacedDigit())
            WaveformView(bars: Array(recorder.levels.suffix(40)), progress: 0, onMine: false)
                .frame(height: 28)
            Button("Send Voice Message", systemImage: "arrow.up.circle.fill", action: send)
                .labelStyle(.iconOnly)
                .font(.title)
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "Recording, \(AttachmentFiles.timeLabel(recorder.elapsed))"))
    }
}

private struct MessageBubble: View {
    let model: ChatModel
    let message: ChatMessage
    let place: Transcript.Place
    let delivery: ChatDelivery?
    let quoted: Quote?
    let highlighted: Bool
    let actions: BubbleActions

    private var live: Bool { message.status == .sent && !message.deleted }

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
                bubble
                    .overlay {
                        if highlighted {
                            RoundedRectangle(cornerRadius: 18, style: .continuous)
                                .strokeBorder(Color.yellow, lineWidth: 3)
                        }
                    }
                    .contextMenu { menu }
                if !message.reactions.isEmpty && !message.deleted {
                    ReactionsRow(reactions: message.reactions, react: actions.react)
                }
                if delivery != nil || (message.edited && !message.deleted) {
                    HStack(spacing: 4) {
                        if message.edited && !message.deleted {
                            Text("Edited").font(.caption2).foregroundStyle(.secondary)
                        }
                        if let delivery { DeliveryLabel(delivery: delivery) }
                    }
                    .padding(.horizontal, 4)
                }
            }
            if !message.isMine { Spacer(minLength: 48) }
        }
        .padding(.top, place.startsGroup ? 6 : 0)
        .contentShape(Rectangle())
        .onTapGesture {
            if message.status == .failed { actions.retry() }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityText)
        .accessibilityHint(message.status == .failed ? "Double-tap to send again" : "")
        .accessibilityActions {
            if live {
                Button("Reply", action: actions.reply)
                ForEach(quickReactions, id: \.self) { emoji in
                    Button("React \(emoji)") { actions.react(emoji) }
                }
            }
            if message.isMine && live { Button("Edit", action: actions.edit) }
            if message.isMine && !message.deleted { Button("Delete", action: actions.delete) }
            if !message.deleted {
                ForEach(Array(message.attachments.filter(\.isAudio).enumerated()), id: \.offset) {
                    _, attachment in
                    Button("Play Voice Message") { actions.play(attachment) }
                }
            }
        }
    }

    @ViewBuilder private var bubble: some View {
        if message.deleted {
            Text("This message was deleted")
                .italic()
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .overlay {
                    RoundedRectangle(cornerRadius: 18, style: .continuous)
                        .strokeBorder(.quaternary)
                }
        } else {
            VStack(alignment: .leading, spacing: 4) {
                if let quoted {
                    QuoteView(quote: quoted, onMine: message.isMine)
                        .onTapGesture { actions.jump(quoted.id) }
                }
                if !message.attachments.isEmpty {
                    AttachmentsView(
                        model: model, attachments: message.attachments, onMine: message.isMine,
                        open: actions.open, play: actions.play)
                }
                if !message.body.isEmpty {
                    Text(message.body)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .foregroundStyle(message.isMine ? Color.white : Color.primary)
            .background(
                RoundedRectangle(cornerRadius: 18, style: .continuous).fill(fill)
            )
        }
    }

    @ViewBuilder private var menu: some View {
        if live {
            ControlGroup {
                ForEach(quickReactions, id: \.self) { emoji in
                    Button(emoji) { actions.react(emoji) }
                }
            }
            .controlGroupStyle(.palette)
            Button("Reply", systemImage: "arrowshape.turn.up.left", action: actions.reply)
        }
        if !message.deleted && !message.body.isEmpty {
            Button("Copy", systemImage: "doc.on.doc") { copy(message.body) }
        }
        if !message.deleted {
            ForEach(Array(message.attachments.filter { !$0.isAudio }.enumerated()), id: \.offset) {
                _, attachment in
                Button(
                    attachment.isImage
                        ? String(localized: "Open Photo")
                        : String(localized: "Open \(attachment.name)"),
                    systemImage: attachment.isImage ? "photo" : "doc"
                ) { actions.open(attachment) }
            }
        }
        if message.isMine && live {
            Button("Edit", systemImage: "pencil", action: actions.edit)
        }
        if message.status == .failed {
            Button("Try Again", systemImage: "arrow.clockwise", action: actions.retry)
        }
        if message.isMine && !message.deleted {
            Button("Delete", systemImage: "trash", role: .destructive, action: actions.delete)
        }
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
        if message.deleted { return "\(who), message deleted, \(time)" }
        var text = "\(who)"
        if let quoted {
            text += ", replying to \(quoted.original?.author ?? String(localized: "a message"))"
        }
        if !message.body.isEmpty { text += ", \(message.body)" }
        for attachment in message.attachments {
            text +=
                attachment.isImage
                ? ", photo" : attachment.isAudio ? ", voice message" : ", document \(attachment.name)"
        }
        text += ", \(time)"
        if message.edited { text += ", edited" }
        for reaction in message.reactions {
            text += ", \(reaction.emoji) \(reaction.count)"
        }
        if let delivery { text += ", \(DeliveryLabel.text(delivery))" }
        return text
    }
}

/// The quoted message at the top of a reply's bubble. Tap to scroll to it.
private struct QuoteView: View {
    let quote: Quote
    /// Drawn on our own (tinted) bubble.
    let onMine: Bool

    var body: some View {
        HStack(spacing: 6) {
            Rectangle()
                .fill(onMine ? Color.white.opacity(0.8) : Color.accentColor)
                .frame(width: 3)
            VStack(alignment: .leading, spacing: 1) {
                Text(author).font(.caption.bold())
                Text(text).font(.caption).lineLimit(2)
            }
            .opacity(0.85)
        }
        .padding(6)
        .background(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(onMine ? Color.white.opacity(0.2) : Color.primary.opacity(0.06))
        )
        .accessibilityAddTraits(.isButton)
    }

    private var author: String {
        guard let original = quote.original else { return String(localized: "Reply") }
        return original.isMine ? String(localized: "You") : original.author
    }

    private var text: String {
        guard let original = quote.original else { return String(localized: "Original message not loaded") }
        return original.deleted ? String(localized: "This message was deleted") : original.summary
    }
}

/// The reactions under a bubble. Tap one to add or remove yours.
private struct ReactionsRow: View {
    let reactions: [ChatReaction]
    let react: (String) -> Void

    var body: some View {
        HStack(spacing: 4) {
            ForEach(reactions, id: \.emoji) { reaction in
                Button {
                    react(reaction.emoji)
                } label: {
                    Text("\(reaction.emoji) \(reaction.count)")
                        .font(.caption)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(
                            Capsule().fill(
                                reaction.mine ? Color.accentColor.opacity(0.25) : Color.incomingBubble)
                        )
                        .overlay {
                            if reaction.mine { Capsule().strokeBorder(Color.accentColor) }
                        }
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(reaction.emoji), \(reaction.count)\(reaction.mine ? ", including you" : "")")
            }
        }
        .padding(.horizontal, 4)
    }
}

/// Searches this iPhone's messages (Rust's SQLite full-text index) in one
/// conversation; picking a result scrolls to it.
private struct MessageSearchSheet: View {
    let model: ChatModel
    let conversationId: String
    let picked: (ChatMessage) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var results: [ChatMessage] = []

    var body: some View {
        NavigationStack {
            List(results, id: \.id) { message in
                Button {
                    picked(message)
                } label: {
                    SearchResultRow(message: message, roomTitle: nil, query: query)
                }
                .buttonStyle(.plain)
            }
            .overlay {
                if !query.isEmpty && results.isEmpty {
                    ContentUnavailableView.search(text: query)
                }
            }
            .searchable(text: $query, prompt: "Search messages")
            .task(id: query) {
                try? await Task.sleep(for: .milliseconds(200))
                guard !Task.isCancelled else { return }
                results = await model.search(query, in: conversationId)
            }
            .navigationTitle("Search")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}

/// One search hit: who, when, the text, and (across rooms) where.
struct SearchResultRow: View {
    let message: ChatMessage
    let roomTitle: String?
    let query: String

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(alignment: .firstTextBaseline) {
                Text(roomTitle ?? (message.isMine ? String(localized: "You") : message.author))
                    .font(.headline)
                Spacer()
                Text(Date(unixMilliseconds: message.sentAt).chatListLabel)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            Text(highlighted)
            .font(.subheadline)
            .foregroundStyle(.secondary)
            .lineLimit(3)
        }
        .contentShape(Rectangle())
    }

    /// The body (after the author, across rooms) with the words that
    /// matched in bold.
    private var highlighted: AttributedString {
        let author = message.isMine ? String(localized: "You") : message.author
        var text = AttributedString(roomTitle == nil ? message.body : "\(author): \(message.body)")
        let words = query.split(whereSeparator: \.isWhitespace).map(String.init)
        for word in words where !word.isEmpty {
            var searchStart = text.startIndex
            while let range = text[searchStart...].range(
                of: word, options: [.caseInsensitive, .diacriticInsensitive])
            {
                text[range].inlinePresentationIntent = .stronglyEmphasized
                searchStart = range.upperBound
            }
        }
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
