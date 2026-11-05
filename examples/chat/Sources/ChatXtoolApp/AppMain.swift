import ChatApp
import SwiftUI

// The xtool entry point. ChatLauncher is the same app for `swift run` and the Xcode project.
@main
struct ChatXtoolApp: App {
    var body: some Scene {
        WindowGroup {
            ChatRootView()
        }
    }
}
