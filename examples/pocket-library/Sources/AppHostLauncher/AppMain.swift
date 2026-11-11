import PocketLibraryApp
import SwiftUI

@main
struct ForgeAppLauncher: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .forgeLaunchAppearance()
        }
    }
}
