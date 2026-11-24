@preconcurrency import UrduNaturallyFFI
@preconcurrency import ForgeKit
import ForgeAppKit
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

/// Four tabs, conversation first: Talk (the app), Practise (what came up in
/// your conversations, and listening), Words (to look things up), Progress.
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
                .tabItem { Label("Talk", systemImage: "bubble.left.and.bubble.right") }
            tab { PractiseScreen(model: model) }
                .tabItem { Label("Practise", systemImage: "arrow.counterclockwise") }
            tab { LearnScreen(model: model) }
                .tabItem { Label("Words", systemImage: "text.magnifyingglass") }
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
        TabStack(model: model, content: content())
    }
}

/// One tab's stack, with its own path: Layered Navigation goes back to a
/// screen already open instead of stacking a copy.
private struct TabStack<Content: View>: View {
    let model: UrduModel
    let content: Content
    @State private var path: [Route] = []

    var body: some View {
        NavigationStack(path: $path.forgeLayered()) {
            content.urduDestinations(model)
        }
    }
}

// MARK: Home

/// The app's front door: the next conversation, and all of them. Words are
/// learnt inside conversations, so this is where learning starts.
struct HomeScreen: View {
    let model: UrduModel
    @Environment(\.forgeTheme) private var theme

    private var characters: [CharacterView] { model.characters() }

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

            Section("Next") {
                suggestion(home.suggestion)
            }

            Section {
                ForEach(characters, id: \.id) { character in
                    NavigationLink(value: Route.chat(character.id)) {
                        ForgeRow(
                            "Chat with \(character.name)",
                            subtitle: character.description,
                            systemImage: "text.bubble"
                        )
                    }
                    .accessibilityIdentifier("home.chat.\(character.id)")
                }
            } header: {
                Text("Just chat")
            } footer: {
                Text("Say anything, in Urdu or English. They answer with lines from the conversations, so it goes somewhere new every time.")
            }

            ForEach(Self.levels, id: \.difficulty) { level in
                let scenarios = home.scenarios.filter { $0.difficulty == level.difficulty }
                if !scenarios.isEmpty {
                    Section {
                        ForEach(scenarios, id: \.id) { scenario in
                            NavigationLink(value: Route.scenario(scenario.id)) {
                                ScenarioRow(scenario: scenario)
                            }
                            .accessibilityIdentifier("home.scenario.\(scenario.id)")
                        }
                    } header: {
                        Text(level.title)
                    }
                }
            }
        }
        .navigationTitle("Talk")
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

    private struct Band {
        let difficulty: UInt32
        let title: String
    }

    private static let levels = [
        Band(difficulty: 1, title: "Starting out"),
        Band(difficulty: 2, title: "Everyday"),
        Band(difficulty: 3, title: "Having fun"),
    ]

    @ViewBuilder
    private func suggestion(_ suggestion: Suggestion) -> some View {
        switch suggestion {
        case let .review(count):
            NavigationLink(value: Route.review) {
                ForgeRow("Practise what came up", subtitle: "\(count) from your conversations. A few minutes, then the next one.", systemImage: "arrow.counterclockwise")
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
        case .allDone:
            ForgeRow(
                "You've had every conversation",
                subtitle: "Have one again, or go and talk to someone for real.",
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
