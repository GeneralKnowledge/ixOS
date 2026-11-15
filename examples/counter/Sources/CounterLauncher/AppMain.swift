import CounterApp
import SwiftUI
import ForgeUI

@main
struct CounterLauncher: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .forgeLaunchAppearance()
        }
    }
}
