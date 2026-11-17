@preconcurrency import RelayFFI
import ForgeAppKit
import ForgeKit
import ForgeUI
import SwiftUI

/// The app's root: messaging (Chats, the Assistant, Settings; Chat's
/// screens before Chat merged into Relay) and Relay's Devices, one tab
/// each (docs/relay-chat-merge.md, step 1). Each side keeps its own Rust
/// core and host: `ChatModel.host` (data in Application Support/Chat, as
/// before the merge) and `host` (Relay's).
public struct RelayRoot: View {
    @Bindable private var host: ForgeHost<RelayCore>
    @State private var chat: ChatModel
    @State private var tab = Tab.chats
    @State private var chatPath: [ChatRoute] = []

    enum Tab: Hashable { case chats, assistant, devices, settings }

    /// Shows `host` and `chat`, which the app keeps for the whole process
    /// (the entry point, or Device Lab, which opens Relay in a sheet many
    /// times): one core each must outlive each sheet. The entry point
    /// attaches both hosts to `ForgeLifecycle`.
    public init(host: ForgeHost<RelayCore>, chat: ChatModel) {
        self.host = host
        _chat = State(initialValue: chat)
    }

    /// The messaging model the app uses: the Keychain account and Apple's
    /// on-device model for the assistant.
    @MainActor
    public static func makeChatModel() -> ChatModel {
        ChatModel(assistantEngine: ForgeAppleModel())
    }

    public var body: some View {
        tabs
            #if os(iOS)
                // Face ID at launch and after a minute away, when on in Settings.
                .forgeAppLock(isEnabled: chat.phase == .ready && chat.appLock, reason: "Unlock Relay")
            #endif
            // Opens messaging's Rust core off the main thread; does nothing
            // when already open.
            .task { await chat.open() }
            // Back on screen: Apple Intelligence may have been turned on.
            .onChange(of: chat.host.phase) { _, phase in
                if phase == .active { chat.becameActive() }
            }
            // A tapped message notification opens its room (rust/src/chat/alerts.rs).
            .onForgeDeepLink { link in
                guard let route = ChatLinks.route(link) else { return }
                tab = .chats
                chatPath = [route]
            }
            // Failed swipes and menu actions in messaging.
            .forgeAlert(failure: Bindable(chat).actionError)
            // Messaging's haptics, whichever tab is showing: a message in
            // another chat, a message that failed to send.
            .forgeFeedback(chat.feedback)
    }

    private var tabs: some View {
        TabView(selection: $tab) {
            chats
                .tabItem { Label("Chats", systemImage: "bubble.left.and.bubble.right") }
                .badge(chat.phase == .ready ? chat.unreadConversationCount : 0)
                .tag(Tab.chats)

            if chat.phase == .ready, let assistant = chat.assistant {
                AssistantTab(model: assistant)
                    .tabItem { Label("Assistant", systemImage: "sparkles") }
                    .tag(Tab.assistant)
            }

            ContentView(host: host)
                .tabItem { Label("Devices", systemImage: "arrow.left.arrow.right") }
                .tag(Tab.devices)

            if chat.phase == .ready {
                NavigationStack {
                    SettingsView(model: chat)
                }
                .tabItem { Label("Settings", systemImage: "gearshape") }
                .tag(Tab.settings)
            }
        }
    }

    /// Sign-up, the conversations, a spinner, or a recovery screen.
    @ViewBuilder private var chats: some View {
        switch chat.phase {
        case .loading:
            ProgressView()
        case .signedOut:
            SignUpView(model: chat)
        case .ready:
            ChatsTab(model: chat, path: $chatPath)
        case .failed(let message):
            ContentUnavailableView {
                Label("Can't Open Chats", systemImage: "exclamationmark.bubble")
            } description: {
                Text(message)
            } actions: {
                Button("Try Again") { Task { await chat.open() } }
                    .buttonStyle(.borderedProminent)
                    .accessibilityIdentifier("content.retry")
            }
        }
    }
}
