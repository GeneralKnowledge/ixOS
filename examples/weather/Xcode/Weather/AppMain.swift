import SwiftUI
import WeatherApp
import ForgeUI

@main
struct WeatherApplication: App {
    var body: some Scene {
        WindowGroup {
            WeatherView()
                .forgeLaunchAppearance()
        }
    }
}
