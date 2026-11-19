@preconcurrency import UrduNaturallyFFI
import ForgeUI
import SwiftUI

/// Where a screen can go. Every tab's stack handles all of them.
enum Route: Hashable {
    case scenario(String)
    case topic(String)
    case phrase(String)
    case practice(PracticeSet)
    case review
    case listen
    case settings
    case speechCheck
}

/// A set of items to practise, with a title for the screen.
struct PracticeSet: Hashable {
    let id = UUID()
    let title: String
    let items: [ReviewItem]

    /// Words, expressions and patterns, each with the skill it needs next:
    /// what it means until you know it, then saying it (a pattern: making
    /// a sentence with it).
    static func words(_ title: String, _ words: [PhraseView]) -> PracticeSet {
        PracticeSet(title: title, items: words.map { word in
            let skill: Skill = word.skills.recognition < 2 ? .recognition
                : word.kind == .pattern ? .speaking : .recall
            return ReviewItem(phrase: word, skill: skill)
        })
    }
}

extension View {
    /// The destinations of every `Route`.
    @MainActor
    func urduDestinations(_ model: UrduModel) -> some View {
        navigationDestination(for: Route.self) { route in
            switch route {
            case let .scenario(id): ConversationScreen(model: model, scenarioID: id)
            case let .topic(id): TopicScreen(model: model, topicID: id)
            case let .phrase(id): PhraseScreen(model: model, phraseID: id)
            case let .practice(set): PracticeScreen(model: model, title: set.title, items: set.items)
            case .review: PracticeScreen(model: model, title: "What came up", items: model.reviewQueue())
            case .listen: ListenScreen(model: model)
            case .settings: SettingsScreen(model: model)
            case .speechCheck: SpeechCheckScreen()
            }
        }
    }
}

/// Plays a phrase in Urdu, at natural pace or slowly.
struct PlayButtons: View {
    private let text: String
    private let id: String
    private let label: String
    private let identifier: String
    private let voice: Gender
    private let rate: Float
    @State private var narrator = Narrator.shared

    /// Plays a phrase or word. A pattern has a gap, so it has nothing to
    /// play: give it its examples' buttons instead.
    init(phrase: PhraseView, voice: Gender = .male, rate: Float = 1) {
        text = phrase.kind == .pattern ? "" : phrase.speech
        id = phrase.id
        label = phrase.roman
        identifier = "phrase.play"
        self.voice = voice
        self.rate = rate
    }

    /// Plays one example sentence of a pattern (or a word's combination).
    init(example: ExampleView, of phrase: PhraseView, index: Int) {
        text = example.speech ?? ""
        id = "\(phrase.id)#\(index)"
        label = example.roman
        identifier = "phrase.example.\(index).play"
        voice = .male
        rate = 1
    }

    var body: some View {
        HStack(spacing: 12) {
            Button {
                narrator.say(text: text, id: id, voice: voice, rate: rate)
            } label: {
                Label("Play", systemImage: narrator.speakingID == id ? "speaker.wave.3.fill" : "speaker.wave.2")
            }
            .buttonStyle(.forgeSecondary)
            .accessibilityLabel("Play \(label)")
            .accessibilityIdentifier(identifier)

            Button {
                narrator.say(text: text, id: id, voice: voice, rate: rate, pace: .slow)
            } label: {
                Label("Slowly", systemImage: "tortoise")
            }
            .buttonStyle(.forgeTertiary)
            .accessibilityLabel("Play \(label) slowly")
            .accessibilityIdentifier(identifier + "Slow")
        }
        .disabled(!Narrator.hasUrduVoice || text.isEmpty)
    }
}

/// The English for a phrase, shown as its progress says: in full while it's
/// new, smaller once familiar, and behind a tap once understood. A tap
/// always reveals it.
struct EnglishLine: View {
    let phrase: PhraseView
    @State private var revealed = false
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        switch (phrase.englishDisplay, revealed) {
        case (.visible, _):
            Text(phrase.english)
                .font(.body)
                .foregroundStyle(theme.colors.textSecondary)
        case (.secondary, _), (.hidden, true):
            Text(phrase.english)
                .font(.footnote)
                .foregroundStyle(theme.colors.textTertiary)
        case (.hidden, false):
            Button("Show English") { revealed = true }
                .font(.footnote)
                .buttonStyle(.borderless)
                .accessibilityIdentifier("phrase.showEnglish")
        }
    }
}

/// The big Roman Urdu line.
struct RomanText: View {
    let text: String
    var size: Font = .title.weight(.semibold)
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        Text(text)
            .font(size)
            .foregroundStyle(theme.colors.textPrimary)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// A phrase in a list: Roman Urdu, its English, and where you are with it.
struct PhraseRow: View {
    let phrase: PhraseView
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 2) {
                Text(phrase.roman)
                    .font(.headline)
                    .foregroundStyle(theme.colors.textPrimary)
                Text(phrase.english)
                    .font(.subheadline)
                    .foregroundStyle(theme.colors.textSecondary)
            }
            Spacer(minLength: 8)
            if phrase.isDue {
                ForgeBadge("Due", tone: .warning)
            } else if phrase.stage != .new {
                ForgeBadge(phrase.stage.label, tone: phrase.stage == .canUse ? .success : .neutral)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// How did it go? Four answers, one thumb. "Got it" after a hint counts as
/// with help.
struct OutcomeButtons: View {
    var hinted = false
    let pick: (Outcome) -> Void

    var body: some View {
        VStack(spacing: 8) {
            HStack(spacing: 8) {
                button(.missed, "Didn't get it", .tertiary)
                button(.struggled, "Struggled", .secondary)
            }
            HStack(spacing: 8) {
                button(hinted ? .withHelp : .independent, hinted ? "Got it, with a hint" : "Got it", .secondary)
                button(.confident, "Easy", .primary)
            }
        }
    }

    private func button(_ outcome: Outcome, _ title: String, _ kind: ForgeButtonStyle.Kind) -> some View {
        Button(title) {
            pick(outcome)
        }
        .buttonStyle(ForgeButtonStyle(kind, expands: true))
        .accessibilityIdentifier("outcome.\(outcome.identifier)")
    }
}

/// Says when the phone has no Urdu voice, so silence isn't a mystery.
struct NoVoiceNote: View {
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        if !Narrator.hasUrduVoice {
            Label(
                "This iPhone has no Urdu voice, so phrases can't be played. Settings, Speech Check says more.",
                systemImage: "speaker.slash"
            )
            .font(.footnote)
            .foregroundStyle(theme.colors.textSecondary)
        }
    }
}

/// Milliseconds since `start`, for how long an answer took.
func elapsedMs(since start: Date) -> UInt32 {
    UInt32(clamping: Int(Date().timeIntervalSince(start) * 1000))
}
