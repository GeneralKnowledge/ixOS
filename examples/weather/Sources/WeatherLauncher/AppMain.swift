import SwiftUI
import WeatherApp
import ForgeUI

// The same app for `swift run` on a Mac and the Xcode project.
@main
struct WeatherLauncher: App {
    /// Owns the Rust core for the life of the process (WeatherHost.swift).
    @State private var host = makeAppHost()

    init() {
        // URLSession for forecasts, Core Location for "My Location", and the
        // rest of Forge's native services for the Rust core.
        AppServices.install()
        // Reports the application lifecycle to Rust: refreshes stop in the
        // background, and the core shuts down when iOS ends the app.
        #if canImport(UIKit)
        ForgeLifecycle.attach(host)
        #endif
    }

    var body: some Scene {
        WindowGroup {
            WeatherRoot(host: host)
                .forgeLaunchAppearance()
        }
    }
}
