import AppHost
import SwiftUI

@main
struct ForgeApplication: App {
    init() {
        // Keychain, directories, notifications, permissions and deep links
        // for the Rust core (docs/native-services.md).
        AppServices.install()
        // Native services and lifecycle for the apps the Apps screen opens.
        EmbeddedApps.install()
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}
