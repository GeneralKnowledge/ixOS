import AppHost
import SwiftUI

@main
struct ForgeAppLauncher: App {
    /// Owns the Rust core for the life of the process (AppHost.swift).
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
