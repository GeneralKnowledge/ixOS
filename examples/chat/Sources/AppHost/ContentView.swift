@preconcurrency import ForgeFFI
import SwiftUI

/// The app's root. It owns the model, opens the Rust core once, and shows
/// sign-up, the two tabs, a spinner, or a recovery screen.
public struct ContentView: View {
    @State private var model: ChatModel

    public init() {
        _model = State(
            initialValue: ChatModel(
                notifier: LocalChatNotifier.shared, assistantEngine: FoundationModelsEngine()))
    }

    public var body: some View {
        Group {
            switch model.phase {
            case .loading:
                ProgressView()
            case .signedOut:
                SignUpView(model: model)
            case .ready:
                MainTabs(model: model)
            case .failed(let message):
                ContentUnavailableView {
                    Label("Can't Open Chat", systemImage: "exclamationmark.bubble")
                } description: {
                    Text(message)
                } actions: {
                    Button("Try Again") { Task { await model.open() } }
                        .buttonStyle(.borderedProminent)
                        .accessibilityIdentifier("content.retry")
                }
            }
        }
        // Opens Rust off the main thread; does nothing when already open.
        .task { await model.open() }
        // Reports the scene phase to Rust (see AppLifecycle.swift).
        .forgeLifecycle { model.setPhase($0) }
    }
}

/// Chats, the on-device Assistant and Settings, each with its own
/// navigation stack.
struct MainTabs: View {
    let model: ChatModel
    @State private var tab = Tab.chats
    @State private var chatPath: [ChatRoute] = []

    enum Tab: Hashable { case chats, assistant, settings }

    var body: some View {
        TabView(selection: $tab) {
            ChatsTab(model: model, path: $chatPath)
                .tabItem { Label("Chats", systemImage: "bubble.left.and.bubble.right") }
                .badge(model.unreadConversationCount)
                .tag(Tab.chats)

            if let assistant = model.assistant {
                AssistantTab(model: assistant)
                    .tabItem { Label("Assistant", systemImage: "sparkles") }
                    .tag(Tab.assistant)
            }

            NavigationStack {
                SettingsView(model: model)
            }
            .tabItem { Label("Settings", systemImage: "gearshape") }
            .tag(Tab.settings)
        }
        // A tapped message notification opens its room (LocalChatNotifier).
        .onForgeDeepLink { link in
            guard let route = ChatLinks.route(link) else { return }
            tab = .chats
            chatPath = [route]
        }
        // Failed swipes and menu actions, from either tab.
        .errorAlert("Something Went Wrong", message: Bindable(model).actionError)
    }
}
