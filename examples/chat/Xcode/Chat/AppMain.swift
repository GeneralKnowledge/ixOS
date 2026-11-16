import ChatApp
import ForgeKit
import SwiftUI
import ForgeUI

@main
struct ChatApplication: App {
    /// The model and its Rust core's host, for the life of the process.
    @State private var model = ContentView.makeModel()

    init() {
        // The Keychain (the account), notifications and deep links for the
        // Rust core (docs/native-services.md).
        AppServices.install()
        #if canImport(UIKit)
            ForgeLifecycle.attach(model.host)
        #endif
    }

    var body: some Scene {
        WindowGroup {
            ContentView(model: model)
                .receivesDeepLinks()
                .forgeLaunchAppearance()
        }
    }
}
