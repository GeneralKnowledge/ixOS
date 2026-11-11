import ChatApp
import SwiftUI

@main
struct ChatApplication: App {
    init() {
        // The Keychain (the account), notifications and deep links for the
        // Rust core (docs/native-services.md).
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
