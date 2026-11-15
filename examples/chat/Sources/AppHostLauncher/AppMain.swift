import ChatApp
import SwiftUI
import ForgeUI

@main
struct ChatLauncher: App {
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
