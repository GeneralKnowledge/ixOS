@preconcurrency import UrduNaturallyFFI
import ForgeUI
import SwiftUI

/// What you can actually do, not points: how many phrases you recognise,
/// can recall, and can use, and which ones you find hardest to say.
struct ProgressScreen: View {
    let model: UrduModel

    var body: some View {
        let totals = model.home.totals
        List {
            Section {
                ForgeRow("Met", subtitle: "Seen or heard at least once", value: "\(totals.met) of \(totals.phrases)")
                ForgeRow("Recognise", subtitle: "Know what it means when you see or hear it", value: "\(totals.recognised)")
                ForgeRow("Can recall", subtitle: "Can come up with it from the English", value: "\(totals.canRecall)")
                ForgeRow("Can use", subtitle: "Can say it at your turn in a conversation", value: "\(totals.canUse)")
            } header: {
                Text("What you can do")
            } footer: {
                Text("\(totals.attemptsToday) practised today. \(totals.due) ready to come back.")
            }

            Section("Conversations") {
                ForEach(model.home.scenarios, id: \.id) { scenario in
                    NavigationLink(value: Route.scenario(scenario.id)) {
                        ScenarioRow(scenario: scenario)
                    }
                    .accessibilityIdentifier("progress.scenario.\(scenario.id)")
                }
            }
        }
        .navigationTitle("Progress")
    }
}
