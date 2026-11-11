import AppHost
import SwiftUI

@main
struct ForgeApplication: App {
    init() {
        // Keychain, directories, notifications, permissions and deep links
        // for the Rust core (docs/native-services.md).
        AppServices.install()
        // The lifecycle for the apps the Apps screen opens (Checklist's host).
        EmbeddedApps.attach()
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .receivesDeepLinks()
                .forgeLaunchAppearance()
        }
    }
}
