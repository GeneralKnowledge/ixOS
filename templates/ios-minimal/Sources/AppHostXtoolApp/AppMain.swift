import FORGE_MODULE_NAMEApp
import SwiftUI

// The xtool entry point. AppHostLauncher is the same app for `swift run` and the Xcode project.
@main
struct ForgeAppXtool: App {
    /// Owns the Rust core for the life of the process (FORGE_MODULE_NAMEApp.swift).
    @State private var host = makeAppHost()

    init() {
        // Reports the application lifecycle to Rust, and tells the screen to
        // re-read when the app comes back.
        #if canImport(UIKit)
        ForgeLifecycle.attach(host)
        #endif
    }

    var body: some Scene {
        WindowGroup {
            ContentView(host: host)
                .forgeLaunchAppearance()
        }
    }
}
