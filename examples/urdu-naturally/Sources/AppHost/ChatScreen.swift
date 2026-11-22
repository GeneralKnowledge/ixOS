@preconcurrency import UrduNaturallyFFI
import ForgeUI
import SwiftUI

/// A free chat with Ali, Ayesha, Omar or Khala. You say anything, in Roman
/// Urdu or English; they answer with lines from the conversations, chosen by
/// what you said (rust/src/chat.rs). When you say it in English or other
/// words, you see how to say it. Help shows the lines that would fit.
struct ChatScreen: View {
    let model: UrduModel
    let characterID: String

    @State private var chat: ChatView?
    @State private var typed = ""
    @State private var busy = false
    @State private var showHelp = false
    @State private var autoPlay = true
    /// Apple's on-device model, which may work out what you meant. Made once
    /// per chat; without Apple Intelligence only your lines are understood.
    @State private var phoneModel = ForgeAppleModel()
    @FocusState private var typing: Bool
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        Group {
            if let chat {
                conversation(chat)
            } else {
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .navigationTitle(chat.map { "Chat with \($0.character.name)" } ?? "Chat")
        .navigationBarTitleDisplayModeInlineIfAvailable()
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Toggle(isOn: $autoPlay) {
                    Image(systemName: autoPlay ? "speaker.wave.2" : "speaker.slash")
                }
                .toggleStyle(.button)
                .accessibilityLabel("Read their lines aloud")
                .accessibilityIdentifier("chat.autoPlay")
            }
        }
        .task {
            if chat == nil { await start() }
        }
        // The chat stays in Rust's memory until the next one starts, so
        // switching tabs and back carries on where you were.
        .onDisappear { Narrator.shared.stop() }
    }

    private func conversation(_ chat: ChatView) -> some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 12) {
                        header(chat)
                        ForEach(Array(chat.lines.enumerated()), id: \.offset) { offset, line in
                            bubble(line, character: chat.character)
                                .id(offset)
                        }
                        if chat.ended {
                            ended(chat.character)
                        }
                        Color.clear.frame(height: 1).id("end")
                    }
                    .padding()
                }
                .onChange(of: chat.lines.count) {
                    withAnimation { proxy.scrollTo("end", anchor: .bottom) }
                }
                .onChange(of: showHelp) {
                    withAnimation { proxy.scrollTo("end", anchor: .bottom) }
                }
            }
            if !chat.ended {
                Divider()
                input(chat)
                    .padding()
            }
        }
    }

    private func header(_ chat: ChatView) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Say anything, in Urdu or English. \(chat.character.name) answers in lines from the course, and if you say it in English you'll see how to say it in Urdu.")
                .font(.footnote)
                .foregroundStyle(theme.colors.textSecondary)
            NoVoiceNote()
        }
        .padding(.bottom, 8)
    }

    @ViewBuilder
    private func bubble(_ line: ChatLine, character: CharacterView) -> some View {
        let mine = line.speaker == .you
        HStack {
            if mine { Spacer(minLength: 40) }
            VStack(alignment: .leading, spacing: 4) {
                Text(mine ? "You" : character.name)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(mine ? theme.colors.accent.text : theme.colors.textSecondary)
                if mine, let typed = line.typed, line.heard != .said {
                    Text("\u{201C}\(typed)\u{201D}")
                        .font(.callout)
                        .foregroundStyle(theme.colors.textSecondary)
                }
                if mine, let note = heardNote(line.heard, name: character.name) {
                    Text(note)
                        .font(.footnote.weight(.semibold))
                        .foregroundStyle(theme.colors.accent.text)
                }
                if let phrase = line.phrase {
                    Text(phrase.roman)
                        .font(.title3.weight(.medium))
                        .foregroundStyle(theme.colors.textPrimary)
                    EnglishLine(phrase: phrase)
                    Button {
                        Narrator.shared.say(phrase, voice: mine ? .male : character.gender, rate: mine ? 1 : character.speechRate)
                    } label: {
                        Image(systemName: "speaker.wave.2")
                    }
                    .buttonStyle(.borderless)
                    .disabled(!Narrator.hasUrduVoice)
                    .accessibilityLabel("Play \(phrase.roman)")
                    .accessibilityIdentifier("chat.play")
                }
            }
            .padding(12)
            .background(
                mine ? theme.colors.accent.soft : theme.colors.surfaceRaised,
                in: RoundedRectangle(cornerRadius: 16, style: .continuous)
            )
            .accessibilityElement(children: .combine)
            if !mine { Spacer(minLength: 40) }
        }
    }

    /// What became of your line, above the Urdu.
    private func heardNote(_ heard: ChatHeard?, name: String) -> String? {
        switch heard {
        case .said, .picked, nil: nil
        case .nearly: "Nearly. It's:"
        case .meant: "Say it like this:"
        case .missed: "\(name) didn't catch that. Try other words, or Help."
        }
    }

    private func input(_ chat: ChatView) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            if let note = chat.note {
                Text(note)
                    .font(.footnote)
                    .foregroundStyle(theme.colors.textTertiary)
            }
            if showHelp, !chat.suggestions.isEmpty {
                Text("You could say")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(theme.colors.textSecondary)
                ForEach(chat.suggestions, id: \.id) { phrase in
                    Button {
                        Task { await pick(phrase.id) }
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(phrase.roman)
                                .foregroundStyle(theme.colors.textPrimary)
                            Text(phrase.english)
                                .font(.footnote)
                                .foregroundStyle(theme.colors.textSecondary)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .buttonStyle(ForgeButtonStyle(.secondary, expands: true))
                    .disabled(busy)
                    .accessibilityIdentifier("chat.suggestion.\(phrase.id)")
                }
            }
            HStack(spacing: 8) {
                TextField("Say something", text: $typed)
                    .textFieldStyle(.roundedBorder)
                    .autocorrectionDisabled()
                    .focused($typing)
                    .submitLabel(.send)
                    .onSubmit { Task { await send() } }
                    .accessibilityIdentifier("chat.typed")
                if busy {
                    ProgressView()
                        .accessibilityLabel("Working out what you said")
                } else {
                    Button {
                        Task { await send() }
                    } label: {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.title2)
                    }
                    .buttonStyle(.borderless)
                    .disabled(typed.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .accessibilityLabel("Send")
                    .accessibilityIdentifier("chat.send")
                }
            }
            if !chat.suggestions.isEmpty {
                Button(showHelp ? "Hide help" : "Help: what could I say?") {
                    showHelp.toggle()
                }
                .buttonStyle(ForgeButtonStyle(.tertiary, expands: true))
                .accessibilityIdentifier("chat.help")
            }
        }
    }

    private func ended(_ character: CharacterView) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("\(character.name) has to go")
                .font(.title2.weight(.bold))
            Text("That's everything \(character.name) has to talk about in this version. What you said comes back in Practise.")
                .foregroundStyle(theme.colors.textSecondary)
            Button("Chat again") {
                Task { await start() }
            }
            .buttonStyle(ForgeButtonStyle(.primary, expands: true))
            .accessibilityIdentifier("chat.again")
        }
        .padding()
        .background(theme.colors.surfaceRaised, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }

    // MARK: Actions

    private func start() async {
        showHelp = false
        typed = ""
        update(await model.chatStart(characterID), from: 0)
    }

    private func send() async {
        let text = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !busy, let before = chat?.lines.count else { return }
        busy = true
        typed = ""
        // Apple's model only when it can answer: Rust says why otherwise.
        let view = await model.chatSay(text, model: phoneModel)
        busy = false
        update(view, from: before)
    }

    private func pick(_ phraseID: String) async {
        guard !busy, let before = chat?.lines.count else { return }
        busy = true
        let view = await model.chatPick(phraseID)
        busy = false
        showHelp = false
        update(view, from: before)
    }

    /// Shows the chat as Rust returns it, and reads their new lines aloud
    /// in their voice.
    private func update(_ view: ChatView?, from before: Int) {
        guard let view else { return }
        chat = view
        let theirs = view.lines.dropFirst(before)
            .filter { $0.speaker == .character }
            .compactMap(\.phrase)
        if autoPlay, let first = theirs.first {
            let text = theirs.map(\.speech).joined(separator: " ")
            Narrator.shared.say(text: text, id: first.id, voice: view.character.gender, rate: view.character.speechRate)
        }
    }
}
