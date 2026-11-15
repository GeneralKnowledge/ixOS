import PocketLibraryApp
import SwiftUI
import ForgeUI

// The xtool entry point. AppHostLauncher is the same app for `swift run` and the Xcode project.
@main
struct ForgeAppXtool: App {
    // The library lives as long as the process, not a view.
    @State private var model = LibraryModel()

    init() {
        // URLSession for catalogue requests (docs/native-services.md).
        AppServices.install()
        model.attachToLifecycle()
    }

    var body: some Scene {
        WindowGroup {
            ContentView(model: model)
                .forgeLaunchAppearance()
        }
    }
}
