#if canImport(SwiftUI)
import ChecklistApp
import ForgeKit
import SwiftUI

// The xtool entry point (xtool wants @main in a library product). The App
// owns the host, and so the Rust core, for the life of the process.
@main
struct ChecklistXtoolApp: App {
    @State private var host = Checklist.makeHost()

    init() {
        #if canImport(UIKit)
        ForgeLifecycle.attach(host)
        #endif
    }

    var body: some Scene {
        WindowGroup {
            ChecklistRoot(host: host)
        }
    }
}
#endif
