@preconcurrency import UrduNaturallyFFI
import ForgeUI
import SwiftUI

/// Listen and understand: hear a phrase with no text, pick what it meant.
/// Rounds alternate a woman's and a man's voice, so you get used to both.
/// Slow replays are always there. (More listening modes are in the plan:
/// no subtitles, real conversations, "what did they actually say?")
struct ListenScreen: View {
    let model: UrduModel
    @State private var round: [ListeningItem] = []
    @State private var index = 0
    @State private var picked: Int?
    @State private var firstTry = true
    @State private var right = 0
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        Group {
            if round.isEmpty {
                start
            } else if index < round.count {
                item(round[index])
                    .id(index)
            } else {
                finished
            }
        }
        .navigationTitle("Listen")
        .onDisappear { Narrator.shared.stop() }
    }

    private var start: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Hear it. Understand it.")
                .font(.title.weight(.bold))
            Text("Eight phrases, read by a woman and a man in turn, with no text. Pick what they meant. Phrases you've met come first.")
                .foregroundStyle(theme.colors.textSecondary)
            NoVoiceNote()
            Button("Start listening") { begin() }
                .buttonStyle(ForgeButtonStyle(.primary, expands: true))
                .accessibilityIdentifier("listen.start")
            Spacer()
        }
        .padding()
    }

    private func item(_ item: ListeningItem) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("\(index + 1) of \(round.count) · \(item.voice == .female ? "a woman" : "a man") speaking")
                    .font(.subheadline)
                    .foregroundStyle(theme.colors.textSecondary)
                if Narrator.hasUrduVoice {
                    PlayButtons(phrase: item.phrase, voice: item.voice)
                } else {
                    NoVoiceNote()
                    RomanText(text: item.phrase.roman)
                }
                ForEach(Array(item.choices.enumerated()), id: \.offset) { offset, choice in
                    Button {
                        choose(offset, in: item)
                    } label: {
                        HStack {
                            Text(choice)
                            Spacer()
                            if let picked, offset == picked || offset == Int(item.answer) {
                                Image(systemName: offset == Int(item.answer) ? "checkmark.circle.fill" : "xmark.circle")
                            }
                        }
                    }
                    .buttonStyle(ForgeButtonStyle(picked != nil && offset == Int(item.answer) ? .primary : .secondary, expands: true))
                    .disabled(picked != nil)
                    .accessibilityIdentifier("listen.choice.\(offset)")
                }
                if picked != nil {
                    RomanText(text: item.phrase.roman)
                    Text(item.phrase.english)
                        .foregroundStyle(theme.colors.textSecondary)
                    Button(index + 1 < round.count ? "Next" : "Finish") { next() }
                        .buttonStyle(ForgeButtonStyle(.primary, expands: true))
                        .accessibilityIdentifier("listen.next")
                }
            }
            .padding()
        }
        .onAppear {
            Narrator.shared.say(item.phrase, voice: item.voice)
        }
    }

    private var finished: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("\(right) of \(round.count) understood")
                .font(.title.weight(.bold))
            Text("The ones you missed come back sooner.")
                .foregroundStyle(theme.colors.textSecondary)
            Button("Another round") { begin() }
                .buttonStyle(ForgeButtonStyle(.primary, expands: true))
                .accessibilityIdentifier("listen.again")
            Spacer()
        }
        .padding()
    }

    private func begin() {
        round = model.listeningRound()
        index = 0
        right = 0
        picked = nil
    }

    private func choose(_ offset: Int, in item: ListeningItem) {
        picked = offset
        let correct = offset == Int(item.answer)
        if correct { right += 1 }
        // Without a voice it was reading, not hearing: record recognition.
        let skill: Skill = Narrator.hasUrduVoice ? .listening : .recognition
        Task {
            await model.record(item.phrase.id, skill: skill, outcome: correct ? .independent : .missed)
        }
    }

    private func next() {
        index += 1
        picked = nil
    }
}
