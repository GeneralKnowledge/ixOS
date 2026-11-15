import SwiftUI
import WeatherApp
import ForgeUI

// The xtool entry point. WeatherLauncher is the same app for `swift run` and the Xcode project.
@main
struct WeatherXtoolApp: App {
    var body: some Scene {
        WindowGroup {
            WeatherView()
                .forgeLaunchAppearance()
        }
    }
}
