@preconcurrency import UrduNaturallyFFI
import ForgeUI
import SwiftUI

/// Practising a set of phrases, one at a time, each with its skill:
///
/// - Recognise: see the Roman Urdu, think what it means, reveal.
/// - Recall: see the English, say it out loud (or type it), reveal.
/// - Use: a line with a gap, say the whole line with the word in it.
/// - Hear: listen with no text, think what it means, reveal.
///
/// You judge how it went; the app never pretends to score pronunciation.
struct PracticeScreen: View {
    let model: UrduModel
    let title: String
    @State private var items: [ReviewItem]
    @State private var index = 0
    @State private var done = 0

    init(model: UrduModel, title: String, items: [ReviewItem]) {
        self.model = model
        self.title = title
        _items = State(initialValue: items)
    }

    var body: some View {
        Group {
            if items.isEmpty {
                ForgeStateView.empty("Nothing to practise", systemImage: "checkmark.circle", message: "Nothing is due right now.")
            } else if index < items.count {
                PracticeCard(model: model, item: items[index]) {
                    done += 1
                    index += 1
                }
                .id(index)
            } else {
                ForgeStateView.empty(
                    "Done",
                    systemImage: "checkmark.circle",
                    message: "You practised \(done) \(done == 1 ? "phrase" : "phrases"). They'll come back when they're due."
                )
            }
        }
        .navigationTitle(items.isEmpty || index >= items.count ? title : "\(title) · \(index + 1) of \(items.count)")
        .navigationBarTitleDisplayModeInlineIfAvailable()
    }
}

/// One item: the prompt, the reveal, and how it went.
struct PracticeCard: View {
    let model: UrduModel
    let item: ReviewItem
    let next: @MainActor () -> Void

    @State private var revealed = false
    @State private var hinted = false
    @State private var typed = ""
    @State private var verdict: Verdict?
    @State private var started = Date()
    @State private var latency: UInt32?
    @Environment(\.forgeTheme) private var theme

    private var phrase: PhraseView { item.phrase }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text(instruction)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(theme.colors.accent.text)
                prompt
                if !revealed {
                    if item.skill == .recall || item.skill == .speaking {
                        typing
                    }
                    HStack(spacing: 8) {
                        if (item.skill == .recall || item.skill == .speaking), !hinted {
                            Button("Hint") { hinted = true }
                                .buttonStyle(ForgeButtonStyle(.tertiary, expands: true))
                                .accessibilityIdentifier("practice.hint")
                        }
                        Button("Show") { reveal() }
                            .buttonStyle(ForgeButtonStyle(.primary, expands: true))
                            .accessibilityIdentifier("practice.reveal")
                    }
                } else {
                    answer
                    OutcomeButtons(hinted: hinted) { outcome in
                        Task {
                            await model.record(phrase.id, skill: item.skill, outcome: outcome, latencyMs: latency)
                            next()
                        }
                    }
                }
            }
            .padding()
        }
        .onAppear {
            started = Date()
            if item.skill == .listening { Narrator.shared.say(phrase) }
        }
    }

    private var instruction: String {
        switch item.skill {
        case .recognition: "What does it mean?"
        case .recall: "How do you say it? Say it out loud."
        case .speaking: phrase.cloze == nil ? "Say it out loud." : "Fill the gap and say the whole line."
        case .listening: "Listen. What did they say?"
        }
    }

    @ViewBuilder
    private var prompt: some View {
        switch item.skill {
        case .recognition:
            RomanText(text: phrase.roman, size: .largeTitle.weight(.semibold))
            PlayButtons(phrase: phrase)
        case .recall:
            Text(phrase.english)
                .font(.title.weight(.semibold))
                .foregroundStyle(theme.colors.textPrimary)
            if hinted { hint }
        case .speaking:
            if let cloze = phrase.cloze {
                Text("\(cloze.before)____\(cloze.after)")
                    .font(.title.weight(.semibold))
                    .foregroundStyle(theme.colors.textPrimary)
                    .accessibilityLabel("\(cloze.before) blank \(cloze.after)")
                Text(cloze.english)
                    .foregroundStyle(theme.colors.textSecondary)
            } else {
                Text(phrase.english)
                    .font(.title.weight(.semibold))
                    .foregroundStyle(theme.colors.textPrimary)
            }
            if hinted { hint }
        case .listening:
            if Narrator.hasUrduVoice {
                PlayButtons(phrase: phrase)
            } else {
                // No voice: practise reading instead of hearing, and say so.
                NoVoiceNote()
                RomanText(text: phrase.roman)
            }
        }
    }

    /// The gap line with the word in it.
    private func filled(_ cloze: ClozeView) -> String {
        let word = cloze.before.isEmpty ? phrase.roman : phrase.roman.lowercased()
        return cloze.before + word.trimmingCharacters(in: .punctuationCharacters) + cloze.after
    }

    private var hint: some View {
        // A word's first letter; a phrase's first word.
        let first = phrase.kind == .word
            ? String(phrase.roman.prefix(1))
            : phrase.roman.split(separator: " ").first.map(String.init) ?? ""
        return Text("Starts with \u{201C}\(first)…\u{201D}")
            .font(.callout)
            .foregroundStyle(theme.colors.textSecondary)
    }

    private var typing: some View {
        VStack(alignment: .leading, spacing: 6) {
            TextField(phrase.cloze != nil && item.skill == .speaking ? "Type the missing word (optional)" : "Type it (optional)", text: $typed)
                .textFieldStyle(.roundedBorder)
                .autocorrectionDisabled()
                .submitLabel(.done)
                .onSubmit { reveal() }
                .accessibilityIdentifier("practice.typed")
        }
    }

    @ViewBuilder
    private var answer: some View {
        VStack(alignment: .leading, spacing: 8) {
            if item.skill == .speaking, let cloze = phrase.cloze {
                RomanText(text: filled(cloze))
            } else if item.skill != .recognition {
                RomanText(text: phrase.roman)
            }
            Text(phrase.english)
                .font(.title3)
                .foregroundStyle(theme.colors.textSecondary)
            if let literal = phrase.literal {
                Text("Literally: \(literal)")
                    .font(.footnote)
                    .foregroundStyle(theme.colors.textTertiary)
            }
            if let verdict, !typed.isEmpty {
                verdictLine(verdict)
            }
            if item.skill != .recognition && item.skill != .listening {
                PlayButtons(phrase: phrase)
            }
            if let usage = phrase.usage {
                Text(usage)
                    .font(.footnote)
                    .foregroundStyle(theme.colors.textSecondary)
            }
        }
    }

    private func verdictLine(_ verdict: Verdict) -> some View {
        let (text, tone): (String, ForgeTone) = switch verdict {
        case .match: ("You typed it right.", .success)
        case .close: ("Nearly: check the spelling against the answer.", .warning)
        case .different: ("You typed \u{201C}\(typed)\u{201D}.", .neutral)
        }
        return ForgeStatus(text, tone: tone)
    }

    private func reveal() {
        latency = elapsedMs(since: started)
        if !typed.trimmingCharacters(in: .whitespaces).isEmpty {
            verdict = model.check(phrase.id, typed: typed)
        }
        revealed = true
        if item.skill == .recognition || item.skill == .listening {
            Narrator.shared.say(phrase)
        }
    }
}
