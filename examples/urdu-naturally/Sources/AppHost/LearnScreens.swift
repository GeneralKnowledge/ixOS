@preconcurrency import UrduNaturallyFFI
import ForgeUI
import SwiftUI

/// The curriculum: Building Blocks first, then the phrases for
/// conversations, and a search across everything.
struct LearnScreen: View {
    let model: UrduModel
    @State private var query = ""

    var body: some View {
        List {
            if query.isEmpty {
                topics(.foundation, "Building Blocks", "The words you'll hear constantly. Learn these and you can build your own sentences.")
                topics(.conversation, "Phrases for conversations", "Whole things to say, from the conversations.")
            } else {
                let results = model.phrases(topic: nil, query: query)
                if results.isEmpty {
                    Text("Nothing matches \u{201C}\(query)\u{201D}. Try the English, or another spelling.")
                } else {
                    ForEach(results, id: \.id) { phrase in
                        NavigationLink(value: Route.phrase(phrase.id)) {
                            PhraseRow(phrase: phrase)
                        }
                        .accessibilityIdentifier("learn.result.\(phrase.id)")
                    }
                }
            }
        }
        .navigationTitle("Learn")
        .searchable(text: $query, prompt: "English or Roman Urdu")
    }

    @ViewBuilder
    private func topics(_ level: Level, _ title: String, _ footer: String) -> some View {
        Section {
            ForEach(model.home.topics.filter { $0.level == level }, id: \.id) { topic in
                NavigationLink(value: Route.topic(topic.id)) {
                    ForgeRow(
                        topic.title,
                        subtitle: topic.description,
                        value: topic.met == 0 ? nil : "\(topic.met)/\(topic.phrases)"
                    )
                }
                .accessibilityIdentifier("learn.topic.\(topic.id)")
            }
        } header: {
            Text(title)
        } footer: {
            Text(footer)
        }
    }
}

/// One topic's phrases, and a way to practise them.
struct TopicScreen: View {
    let model: UrduModel
    let topicID: String

    var body: some View {
        let phrases = model.phrases(topic: topicID)
        List {
            Section {
                NavigationLink(value: Route.practice(practiceSet(phrases))) {
                    ForgeRow("Practise these", subtitle: "What they mean, then saying them", systemImage: "play.circle")
                }
                .accessibilityIdentifier("topic.practise")
            }
            Section {
                ForEach(phrases, id: \.id) { phrase in
                    NavigationLink(value: Route.phrase(phrase.id)) {
                        PhraseRow(phrase: phrase)
                    }
                    .accessibilityIdentifier("topic.phrase.\(phrase.id)")
                }
            } footer: {
                if let topic = model.topic(topicID) {
                    Text(topic.description)
                }
            }
        }
        .navigationTitle(model.topic(topicID)?.title ?? "Topic")
    }

    /// New and weak ones first: recognise them until you do, then recall.
    private func practiceSet(_ phrases: [PhraseView]) -> PracticeSet {
        let items = phrases
            .sorted { ($0.skills.recognition + $0.skills.recall) < ($1.skills.recognition + $1.skills.recall) }
            .prefix(10)
            .map { phrase in
                ReviewItem(phrase: phrase, skill: phrase.skills.recognition < 2 ? .recognition : .recall)
            }
        return PracticeSet(title: model.topic(topicID)?.title ?? "Practice", items: Array(items))
    }
}

/// Everything about one phrase or word: hear it, see how it's built, when
/// to use it, its other forms and its twin.
struct PhraseScreen: View {
    let model: UrduModel
    let phraseID: String
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        Group {
            if let phrase = model.phrase(phraseID) {
                content(phrase)
            } else {
                ForgeStateView.empty("Not found", systemImage: "questionmark", message: "This phrase isn't in this version.")
            }
        }
        .task { await model.markSeen([phraseID]) }
    }

    private func content(_ phrase: PhraseView) -> some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: 10) {
                    RomanText(text: phrase.roman, size: .largeTitle.weight(.semibold))
                    Text(phrase.english)
                        .font(.title3)
                        .foregroundStyle(theme.colors.textSecondary)
                    if let literal = phrase.literal {
                        Text("Literally: \(literal)")
                            .font(.footnote)
                            .foregroundStyle(theme.colors.textTertiary)
                    }
                    PlayButtons(phrase: phrase)
                        .padding(.top, 4)
                    NoVoiceNote()
                }
                .padding(.vertical, 6)
            }

            if let usage = phrase.usage {
                Section("When to use it") {
                    Text(usage)
                }
            }

            if !phrase.parts.isEmpty {
                Section("How it's built") {
                    ForEach(Array(phrase.parts.enumerated()), id: \.offset) { _, part in
                        LabeledContent(part.roman, value: part.english)
                    }
                }
            }

            if !phrase.combos.isEmpty {
                Section("Say it with") {
                    ForEach(Array(phrase.combos.enumerated()), id: \.offset) { _, combo in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(combo.roman).font(.headline)
                            Text(combo.english).foregroundStyle(theme.colors.textSecondary)
                        }
                        .accessibilityElement(children: .combine)
                    }
                }
            }

            if let opposite = phrase.opposite {
                Section("Its opposite") {
                    NavigationLink(value: Route.phrase(opposite.id)) {
                        ForgeRow(opposite.roman, subtitle: opposite.english)
                    }
                    .accessibilityIdentifier("phrase.opposite")
                }
            }

            if !phrase.variants.isEmpty {
                Section("Other ways you'll hear it") {
                    ForEach(Array(phrase.variants.enumerated()), id: \.offset) { _, variant in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(variant.kind.label)
                                .font(.caption)
                                .foregroundStyle(theme.colors.textTertiary)
                            Text(variant.roman).font(.headline)
                            if let note = variant.note {
                                Text(note).font(.footnote).foregroundStyle(theme.colors.textSecondary)
                            }
                        }
                        .accessibilityElement(children: .combine)
                    }
                }
            }

            Section("Practise it") {
                practise(phrase, .recognition, "What does it mean?", "eye")
                practise(phrase, .recall, "Say it from the English", "mouth")
                if phrase.cloze != nil {
                    practise(phrase, .speaking, "Use it in a sentence", "text.insert")
                }
                practise(phrase, .listening, "Hear it and understand", "ear")
            }

            Section {
                HStack {
                    ForgeBadge(phrase.formality.label)
                    if phrase.maleForm {
                        ForgeBadge("Your (male) form")
                    }
                    Spacer()
                    ForgeBadge(phrase.stage.label, tone: phrase.stage == .canUse ? .success : .neutral)
                }
                SkillBars(skills: phrase.skills)
            } footer: {
                Text(phrase.status.label)
            }
        }
        .navigationTitle(phrase.kind == .word ? "Word" : "Phrase")
        .navigationBarTitleDisplayModeInlineIfAvailable()
    }

    private func practise(_ phrase: PhraseView, _ skill: Skill, _ title: String, _ image: String) -> some View {
        NavigationLink(value: Route.practice(PracticeSet(title: title, items: [ReviewItem(phrase: phrase, skill: skill)]))) {
            Label(title, systemImage: image)
        }
        .accessibilityIdentifier("phrase.practise.\(skill.label.lowercased())")
    }
}

/// The four abilities, each out of five.
struct SkillBars: View {
    let skills: Skills
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        VStack(spacing: 6) {
            bar("Recognise", skills.recognition)
            bar("Recall", skills.recall)
            bar("Use", skills.speaking)
            bar("Hear", skills.listening)
        }
    }

    private func bar(_ title: String, _ level: UInt32) -> some View {
        HStack {
            Text(title)
                .font(.footnote)
                .frame(width: 80, alignment: .leading)
            ProgressView(value: Double(level), total: 5)
                .tint(theme.colors.accent.solid)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(title): \(level) of 5")
    }
}
