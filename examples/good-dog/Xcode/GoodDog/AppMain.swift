import GoodDogApp
import ForgeKit
import ForgeUI
import SwiftUI

@main
struct ForgeApplication: App {
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
