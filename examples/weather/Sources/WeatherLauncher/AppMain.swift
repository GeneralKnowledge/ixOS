import SwiftUI
import WeatherApp

@main
struct WeatherLauncher: App {
    var body: some Scene {
        WindowGroup {
            WeatherView()
                .forgeLaunchAppearance()
        }
    }
}
