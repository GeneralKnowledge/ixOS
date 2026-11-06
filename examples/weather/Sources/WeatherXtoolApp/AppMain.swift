import SwiftUI
import WeatherApp

// The xtool entry point. WeatherLauncher is the same app for `swift run` and the Xcode project.
@main
struct WeatherXtoolApp: App {
    var body: some Scene {
        WindowGroup {
            WeatherView()
        }
    }
}
