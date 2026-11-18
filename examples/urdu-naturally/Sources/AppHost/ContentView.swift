@preconcurrency import UrduNaturallyFFI
@preconcurrency import ForgeKit
import ForgeUI
import SwiftUI

/// The app's root. ForgeKit's `ForgeRoot` shows a spinner while the Rust
/// core opens, the app once it has, or a recovery screen if it failed.
public struct ContentView: View {
    @Bindable private var host: ForgeHost<UrduCore>

    public init(host: ForgeHost<UrduCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            RootTabs(core: core, changes: host.changes)
        }
        // A restrained green on ForgeUI's neutral greys.
        .forgeTheme(ForgeTheme(accent: .green))
    }
}

/// Five tabs: Home, Learn, Talk, Listen, Progress.
private struct RootTabs: View {
    @State private var model: UrduModel
    let changes: UInt64

    init(core: UrduCore, changes: UInt64) {
        _model = State(initialValue: UrduModel(core: core))
        self.changes = changes
    }

    var body: some View {
        TabView {
            tab { HomeScreen(model: model) }
                .tabItem { Label("Home", systemImage: "house") }
            tab { LearnScreen(model: model) }
                .tabItem { Label("Learn", systemImage: "text.book.closed") }
            tab { TalkScreen(model: model) }
                .tabItem { Label("Talk", systemImage: "bubble.left.and.bubble.right") }
            tab { ListenScreen(model: model) }
                .tabItem { Label("Listen", systemImage: "ear") }
            tab { ProgressScreen(model: model) }
                .tabItem { Label("Progress", systemImage: "chart.bar") }
        }
        // Rust committed a change, or the app came back: show what Rust holds.
        .onChange(of: changes) { model.reload() }
        .alert(
            "Something Went Wrong",
            isPresented: Binding(
                get: { model.errorMessage != nil },
                set: { if !$0 { model.errorMessage = nil } }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(model.errorMessage ?? "")
        }
    }

    private func tab<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        NavigationStack {
            content().urduDestinations(model)
        }
    }
}

// MARK: Home

/// Where to pick up, what's due, and the conversations.
struct HomeScreen: View {
    let model: UrduModel
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        let home = model.home
        List {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Let's get talking.")
                        .font(.largeTitle.weight(.bold))
                        .foregroundStyle(theme.colors.textPrimary)
                    Text("Less studying. More talking.")
                        .foregroundStyle(theme.colors.textSecondary)
                }
                .listRowBackground(Color.clear)
            }

            Section("Pick up where you left off") {
                suggestion(home.suggestion)
            }

            if home.totals.due > 0 {
                Section {
                    NavigationLink(value: Route.review) {
                        ForgeRow(
                            "Review",
                            subtitle: "\(home.totals.due) phrases are ready to come back",
                            systemImage: "arrow.counterclockwise"
                        )
                    }
                    .accessibilityIdentifier("home.review")
                }
            }

            Section("Conversations") {
                ForEach(home.scenarios, id: \.id) { scenario in
                    NavigationLink(value: Route.scenario(scenario.id)) {
                        ScenarioRow(scenario: scenario)
                    }
                    .accessibilityIdentifier("home.scenario.\(scenario.id)")
                }
            }
        }
        .navigationTitle("Urdu, Naturally")
        .navigationBarTitleDisplayModeInlineIfAvailable()
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                NavigationLink(value: Route.settings) {
                    Image(systemName: "gearshape")
                }
                .accessibilityLabel("Settings")
                .accessibilityIdentifier("home.settings")
            }
        }
    }

    @ViewBuilder
    private func suggestion(_ suggestion: Suggestion) -> some View {
        switch suggestion {
        case let .review(count):
            NavigationLink(value: Route.review) {
                ForgeRow("Review \(count) phrases", subtitle: "A few minutes, then something new.", systemImage: "arrow.counterclockwise")
            }
            .accessibilityIdentifier("home.next")
        case let .conversation(id):
            if let scenario = model.scenarioSummary(id) {
                NavigationLink(value: Route.scenario(id)) {
                    ForgeRow(
                        scenario.title,
                        subtitle: "With \(scenario.character.name). \(scenario.description)",
                        systemImage: "bubble.left.and.bubble.right"
                    )
                }
                .accessibilityIdentifier("home.next")
            }
        case let .learn(topicID):
            if let topic = model.topic(topicID) {
                NavigationLink(value: Route.topic(topicID)) {
                    ForgeRow("Learn: \(topic.title)", subtitle: topic.description, systemImage: "text.book.closed")
                }
                .accessibilityIdentifier("home.next")
            }
        case .allDone:
            ForgeRow(
                "Nothing due",
                subtitle: "Go and talk to someone. Come back to note what you couldn't say.",
                systemImage: "checkmark.circle"
            )
        }
    }
}

/// A conversation in a list: who with, and how much of it you can say.
struct ScenarioRow: View {
    let scenario: ScenarioSummary

    var body: some View {
        ForgeRow(
            scenario.title,
            subtitle: "With \(scenario.character.name) · \(status)",
            systemImage: scenario.character.gender == .female ? "person.crop.circle.fill" : "person.crop.circle"
        )
    }

    private var status: String {
        if scenario.newLines == scenario.yourLines { return "new" }
        if scenario.dueLines > 0 { return "\(scenario.dueLines) lines to practise" }
        return "you can say \(scenario.canUse) of \(scenario.yourLines) lines"
    }
}

extension View {
    /// Inline titles on iOS; nothing elsewhere (the macOS test host).
    func navigationBarTitleDisplayModeInlineIfAvailable() -> some View {
        #if os(iOS)
            navigationBarTitleDisplayMode(.inline)
        #else
            self
        #endif
    }
}
