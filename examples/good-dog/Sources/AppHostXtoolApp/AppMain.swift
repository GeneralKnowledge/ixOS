import GoodDogApp
import ForgeKit
import ForgeUI
import SwiftUI

// The xtool entry point. AppHostLauncher is the same app for `swift run` and the Xcode project.
@main
struct ForgeAppXtool: App {
    /// The Rust core's host, for the life of the process.
    @State private var host = makeAppHost()

    init() {
        // Keychain, directories, notifications, permissions and deep links
        // for the Rust core (docs/native-services.md).
        AppServices.install()
        #if canImport(UIKit)
            ForgeLifecycle.attach(host)
        #endif
    }

    var body: some Scene {
        WindowGroup {
            ContentView(host: host)
                .receivesDeepLinks()
                .forgeLaunchAppearance()
        }
    }
}
