import AppHost
import SwiftUI

@main
struct ForgeApplication: App {
    /// Owns the Rust core for the life of the process (AppHost.swift).
    @State private var host = makeAppHost()

    init() {
        // Keychain, directories, notifications, permissions and deep links
        // for the Rust core (docs/native-services.md).
        AppServices.install()
        // Reports the application lifecycle to Rust: on-screen work stops in
        // the background, and the screens re-read on the way back.
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
