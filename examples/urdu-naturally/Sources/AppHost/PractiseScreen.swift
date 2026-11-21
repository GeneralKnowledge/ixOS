@preconcurrency import UrduNaturallyFFI
import ForgeUI
import SwiftUI

/// Practice is what came up in your conversations coming back, not a
/// course of its own: due lines and words, listening, and the lines you
/// find hardest to say.
struct PractiseScreen: View {
    let model: UrduModel

    var body: some View {
        let totals = model.home.totals
        List {
            Section {
                if totals.due > 0 {
                    NavigationLink(value: Route.review) {
                        ForgeRow(
                            "What came up",
                            subtitle: "\(totals.due) lines and words from your conversations are ready to come back",
                            systemImage: "arrow.counterclockwise"
                        )
                    }
                    .accessibilityIdentifier("practise.review")
                } else {
                    ForgeRow(
                        "Nothing to come back yet",
                        subtitle: "Have a conversation. What you meet in it comes back here when it's due.",
                        systemImage: "bubble.left.and.bubble.right"
                    )
                }
            } footer: {
                Text("Lines you found hard come back sooner; ones you know well, later.")
            }

            Section {
                NavigationLink(value: Route.listen) {
                    ForgeRow("Listen", subtitle: "Hear lines with no text, and pick what they meant.", systemImage: "ear")
                }
                .accessibilityIdentifier("practise.listen")
            }

            if !totals.hardestToSay.isEmpty {
                Section("Hardest to say") {
                    ForEach(totals.hardestToSay, id: \.id) { phrase in
                        NavigationLink(value: Route.phrase(phrase.id)) {
                            PhraseRow(phrase: phrase)
                        }
                        .accessibilityIdentifier("practise.hard.\(phrase.id)")
                    }
                    NavigationLink(value: Route.practice(PracticeSet(
                        title: "Hardest to say",
                        items: totals.hardestToSay.map { ReviewItem(phrase: $0, skill: .speaking) }
                    ))) {
                        Label("Practise saying these", systemImage: "mouth")
                    }
                    .accessibilityIdentifier("practise.hardest")
                }
            }
        }
        .navigationTitle("Practise")
    }
}
