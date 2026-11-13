import PocketLibraryApp
import SwiftUI

@main
struct ForgeApplication: App {
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
