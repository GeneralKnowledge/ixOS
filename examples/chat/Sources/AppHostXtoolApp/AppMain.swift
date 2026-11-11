import ChatApp
import SwiftUI

// The xtool entry point. AppHostLauncher is the same app for `swift run` and the Xcode project.
@main
struct ChatXtool: App {
    init() {
        // Keychain, directories, notifications, permissions and deep links
        // for the Rust core (docs/native-services.md).
        AppServices.install()
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .receivesDeepLinks()
                .forgeLaunchAppearance()
        }
    }
}
