@preconcurrency import ChatFFI
import ForgeAppKit
import ForgeKit
import SwiftUI

/// The app's root. It owns the model, opens the Rust core once, and shows
/// sign-up, the two tabs, a spinner, or a recovery screen.
public struct ContentView: View {
    @State private var model: ChatModel

    /// Shows `model`, which the app keeps for the whole process (the app's
    /// entry point, or Device Lab, which opens Chat in a sheet many times):
    /// one core (one set of room connections and notifications) must
    /// outlive each sheet. The entry point attaches `model.host` to
    /// `ForgeLifecycle`, which reports the scene phase to Rust.
    public init(model: ChatModel) {
        _model = State(initialValue: model)
    }

    /// The model the app uses: the Keychain account and Apple's on-device
    /// model for the assistant.
    @MainActor
    public static func makeModel() -> ChatModel {
        ChatModel(assistantEngine: ForgeAppleModel())
    }

    public var body: some View {
        Group {
            switch model.phase {
            case .loading:
                ProgressView()
            case .signedOut:
                SignUpView(model: model)
            case .ready:
                #if os(iOS)
                    // Face ID at launch and after a minute away, when on in Settings.
                    MainTabs(model: model)
                        .forgeAppLock(isEnabled: model.appLock, reason: "Unlock Chat")
                #else
                    MainTabs(model: model)
                #endif
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
        // Back on screen: Apple Intelligence may have been turned on.
        .onChange(of: model.host.phase) { _, phase in
            if phase == .active { model.becameActive() }
        }
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
        // A tapped message notification opens its room (rust/src/alerts.rs).
        .onForgeDeepLink { link in
            guard let route = ChatLinks.route(link) else { return }
            tab = .chats
            chatPath = [route]
        }
        // Failed swipes and menu actions, from either tab.
        .forgeAlert(failure: Bindable(model).actionError)
    }
}
