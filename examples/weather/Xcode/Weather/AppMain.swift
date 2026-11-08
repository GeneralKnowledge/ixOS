import SwiftUI
import WeatherApp

@main
struct WeatherApplication: App {
    var body: some Scene {
        WindowGroup {
            WeatherView()
                .forgeLaunchAppearance()
        }
    }
}
