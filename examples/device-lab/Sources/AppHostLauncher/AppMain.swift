import AppHost
import SwiftUI

@main
struct ForgeAppLauncher: App {
    #if os(iOS)
        // Passes background download events to Forge's transfers service.
        @UIApplicationDelegateAdaptor(TransfersAppDelegate.self) private var appDelegate
    #endif

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
