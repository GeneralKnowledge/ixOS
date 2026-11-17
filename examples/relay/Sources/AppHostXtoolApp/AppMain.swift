import RelayApp
import SwiftUI
import ForgeUI

// The xtool entry point. AppHostLauncher is the same app for `swift run` and the Xcode project.
@main
struct ForgeAppXtool: App {
    /// Own the Rust cores for the life of the process (RelayHost.swift,
    /// Messaging/ChatModel.swift).
    @State private var host = makeAppHost()
    @State private var chat = RelayRoot.makeChatModel()

    init() {
        // Keychain, directories, notifications, permissions and deep links
        // for the Rust core (docs/native-services.md).
        AppServices.install()
        // Reports the application lifecycle to Rust, and tells the screen to
        // re-read when the app comes back.
        #if canImport(UIKit)
        ForgeLifecycle.attach(host)
        ForgeLifecycle.attach(chat.host)
        #endif
    }

    var body: some Scene {
        WindowGroup {
            RelayRoot(host: host, chat: chat)
                .receivesDeepLinks()
                .forgeLaunchAppearance()
        }
    }
}
