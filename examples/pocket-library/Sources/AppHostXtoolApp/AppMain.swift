import PocketLibraryApp
import SwiftUI

// The xtool entry point. AppHostLauncher is the same app for `swift run` and the Xcode project.
@main
struct ForgeAppXtool: App {
    // The library lives as long as the process, not a view.
    @State private var model = LibraryModel()

    init() {
        model.attachToLifecycle()
    }

    var body: some Scene {
        WindowGroup {
            ContentView(model: model)
                .forgeLaunchAppearance()
        }
    }
}
