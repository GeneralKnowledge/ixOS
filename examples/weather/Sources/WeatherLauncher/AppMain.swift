import SwiftUI
import WeatherApp
import ForgeUI

@main
struct WeatherLauncher: App {
    var body: some Scene {
        WindowGroup {
            WeatherView()
                .forgeLaunchAppearance()
        }
    }
}
