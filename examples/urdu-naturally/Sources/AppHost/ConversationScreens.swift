@preconcurrency import UrduNaturallyFFI
import ForgeUI
import SwiftUI

/// A conversation, turn by turn. The other person's lines appear and are
/// read aloud in their voice; at your turn you see what you want to say in
/// English, say it out loud (or type it), then see the Urdu and say how it
/// went.
struct ConversationScreen: View {
    let model: UrduModel
    let scenarioID: String

    /// Read once, so the conversation doesn't change under you as you
    /// practise it.
    @State private var scenario: ScenarioView?
    /// How many turns are on screen.
    @State private var shown = 0
    @State private var revealed = false
    @State private var hinted = false
    @State private var typed = ""
    @State private var verdict: Verdict?
    @State private var started = Date()
    @State private var results: [Outcome] = []
    @State private var autoPlay = true
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        Group {
            if let scenario {
                conversation(scenario)
            } else {
                ForgeStateView.empty("Not found", systemImage: "questionmark", message: "This conversation isn't in this version.")
            }
        }
        .navigationTitle(scenario?.summary.title ?? "Conversation")
        .navigationBarTitleDisplayModeInlineIfAvailable()
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Toggle(isOn: $autoPlay) {
                    Image(systemName: autoPlay ? "speaker.wave.2" : "speaker.slash")
                }
                .toggleStyle(.button)
                .accessibilityLabel("Read their lines aloud")
                .accessibilityIdentifier("conversation.autoPlay")
            }
        }
        .onAppear {
            if scenario == nil {
                scenario = model.scenario(scenarioID)
                advance()
            }
        }
        .onDisappear { Narrator.shared.stop() }
    }

    private func conversation(_ scenario: ScenarioView) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    header(scenario)
                    ForEach(Array(scenario.turns.prefix(shown).enumerated()), id: \.offset) { offset, turn in
                        bubble(turn, character: scenario.summary.character)
                            .id(offset)
                    }
                    if shown < scenario.turns.count, scenario.turns[shown].speaker == .you {
                        yourTurn(scenario.turns[shown].phrase)
                            .id("turn")
                    } else if shown >= scenario.turns.count {
                        finished(scenario)
                            .id("turn")
                    }
                }
                .padding()
            }
            .onChange(of: shown) {
                withAnimation { proxy.scrollTo("turn", anchor: .bottom) }
            }
            .onChange(of: revealed) {
                withAnimation { proxy.scrollTo("turn", anchor: .bottom) }
            }
        }
    }

    private func header(_ scenario: ScenarioView) -> some View {
        let summary = scenario.summary
        return VStack(alignment: .leading, spacing: 4) {
            Text(summary.description)
                .foregroundStyle(theme.colors.textSecondary)
            Text("Goal: \(summary.objective)")
                .font(.footnote)
                .foregroundStyle(theme.colors.textTertiary)
            if !scenario.words.isEmpty {
                Text("You'll pick up: \(scenario.words.prefix(6).map(\.roman).joined(separator: ", "))\(scenario.words.count > 6 ? "…" : "")")
                    .font(.footnote)
                    .foregroundStyle(theme.colors.textTertiary)
            }
            NoVoiceNote()
        }
        .padding(.bottom, 8)
    }

    private func bubble(_ turn: TurnView, character: CharacterView) -> some View {
        let mine = turn.speaker == .you
        return HStack {
            if mine { Spacer(minLength: 40) }
            VStack(alignment: .leading, spacing: 4) {
                Text(mine ? "You" : character.name)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(mine ? theme.colors.accent.text : theme.colors.textSecondary)
                Text(turn.phrase.roman)
                    .font(.title3.weight(.medium))
                    .foregroundStyle(theme.colors.textPrimary)
                EnglishLine(phrase: turn.phrase)
                Button {
                    Narrator.shared.say(turn.phrase, voice: mine ? .male : character.gender, rate: mine ? 1 : character.speechRate)
                } label: {
                    Image(systemName: "speaker.wave.2")
                }
                .buttonStyle(.borderless)
                .disabled(!Narrator.hasUrduVoice)
                .accessibilityLabel("Play \(turn.phrase.roman)")
                .accessibilityIdentifier("conversation.play")
            }
            .padding(12)
            .background(
                mine ? theme.colors.accent.soft : theme.colors.surfaceRaised,
                in: RoundedRectangle(cornerRadius: 16, style: .continuous)
            )
            if !mine { Spacer(minLength: 40) }
        }
    }

    private func yourTurn(_ phrase: PhraseView) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Your turn. Say:")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(theme.colors.accent.text)
            Text(phrase.english)
                .font(.title2.weight(.semibold))
                .foregroundStyle(theme.colors.textPrimary)
            if !revealed {
                if hinted {
                    Text("Starts with \u{201C}\(phrase.roman.split(separator: " ").first.map(String.init) ?? "")…\u{201D}")
                        .foregroundStyle(theme.colors.textSecondary)
                }
                TextField("Say it out loud, or type it", text: $typed)
                    .textFieldStyle(.roundedBorder)
                    .autocorrectionDisabled()
                    .onSubmit { reveal(phrase) }
                    .accessibilityIdentifier("conversation.typed")
                HStack(spacing: 8) {
                    if !hinted {
                        Button("Hint") { hinted = true }
                            .buttonStyle(ForgeButtonStyle(.tertiary, expands: true))
                            .accessibilityIdentifier("conversation.hint")
                    }
                    Button("Show") { reveal(phrase) }
                        .buttonStyle(ForgeButtonStyle(.primary, expands: true))
                        .accessibilityIdentifier("conversation.reveal")
                }
            } else {
                RomanText(text: phrase.roman)
                if let verdict, !typed.isEmpty {
                    Text(verdict == .match ? "You typed it right." : verdict == .close ? "Nearly: check the spelling." : "You typed \u{201C}\(typed)\u{201D}.")
                        .font(.footnote)
                        .foregroundStyle(theme.colors.textSecondary)
                }
                PlayButtons(phrase: phrase)
                OutcomeButtons(hinted: hinted) { outcome in
                    Task {
                        await model.record(phrase.id, skill: .speaking, outcome: outcome, latencyMs: elapsedMs(since: started))
                        results.append(outcome)
                        shown += 1
                        advance()
                    }
                }
            }
        }
        .padding()
        .background(theme.colors.surfaceRaised, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }

    private func finished(_ scenario: ScenarioView) -> some View {
        let gotIt = results.filter { $0 != .missed && $0 != .struggled }.count
        return VStack(alignment: .leading, spacing: 12) {
            Text("Conversation done")
                .font(.title2.weight(.bold))
            Text("You said \(gotIt) of \(results.count) of your lines without trouble. The ones you found hard come back in Practise.")
                .foregroundStyle(theme.colors.textSecondary)
            if !scenario.words.isEmpty {
                Text("Words from this conversation")
                    .font(.headline)
                    .padding(.top, 4)
                ForEach(scenario.words, id: \.id) { word in
                    NavigationLink(value: Route.phrase(word.id)) {
                        HStack {
                            Text(word.roman).foregroundStyle(theme.colors.textPrimary)
                            Spacer()
                            Text(word.english)
                                .font(.footnote)
                                .foregroundStyle(theme.colors.textSecondary)
                                .multilineTextAlignment(.trailing)
                        }
                    }
                    .accessibilityIdentifier("conversation.word.\(word.id)")
                }
                NavigationLink(value: Route.practice(PracticeSet.words("Words from \(scenario.summary.title)", scenario.words))) {
                    Label("Practise these words", systemImage: "text.badge.checkmark")
                }
                .buttonStyle(ForgeButtonStyle(.secondary, expands: true))
                .accessibilityIdentifier("conversation.practiseWords")
            }
            Button("Have it again") { restart() }
                .buttonStyle(ForgeButtonStyle(.primary, expands: true))
                .accessibilityIdentifier("conversation.again")
        }
        .padding()
        .background(theme.colors.surfaceRaised, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }

    private func reveal(_ phrase: PhraseView) {
        if !typed.trimmingCharacters(in: .whitespaces).isEmpty {
            verdict = model.check(phrase.id, typed: typed)
        }
        revealed = true
    }

    /// Shows the other person's lines up to your next turn, reading the
    /// first of them aloud, and resets your turn.
    private func advance() {
        guard let scenario else { return }
        revealed = false
        hinted = false
        typed = ""
        verdict = nil
        var theirs: [PhraseView] = []
        while shown < scenario.turns.count, scenario.turns[shown].speaker == .character {
            theirs.append(scenario.turns[shown].phrase)
            shown += 1
        }
        started = Date()
        if autoPlay, let first = theirs.first {
            let character = scenario.summary.character
            // Their lines in one go, in their voice.
            let text = theirs.map(\.speech).joined(separator: " ")
            Narrator.shared.say(text: text, id: first.id, voice: character.gender, rate: character.speechRate)
        }
        var ids = theirs.map(\.id)
        // At the end, the conversation's words count as met: that is how a
        // word enters your practice.
        if shown >= scenario.turns.count { ids += scenario.words.map(\.id) }
        Task { await model.markSeen(ids) }
    }

    private func restart() {
        scenario = model.scenario(scenarioID)
        shown = 0
        results = []
        advance()
    }
}
