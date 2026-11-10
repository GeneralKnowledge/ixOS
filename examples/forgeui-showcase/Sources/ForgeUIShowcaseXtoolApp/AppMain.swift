#if canImport(SwiftUI)
import ForgeUIShowcase
import SwiftUI

// The xtool entry point (xtool wants @main in a library product). The whole
// app is ShowcaseRoot: there is no Rust core to open.
@main
struct ForgeUIShowcaseApp: App {
    var body: some Scene {
        WindowGroup {
            ShowcaseRoot()
        }
    }
}
#endif
