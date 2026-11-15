import CounterApp
import SwiftUI
import ForgeUI

@main
struct CounterApplication: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .forgeLaunchAppearance()
        }
    }
}
