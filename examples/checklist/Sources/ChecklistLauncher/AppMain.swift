import ChecklistApp
import ForgeKit

#if canImport(SwiftUI)
import SwiftUI

/// The same app for `swift run` on macOS.
@main
struct ChecklistLauncher: App {
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
#else
import Foundation

/// Linux has no SwiftUI: open the real core through ForgeKit on throwaway
/// data, run one command, and shut down, to show the executable links.
@main
struct ChecklistLauncher {
    @MainActor
    static func main() async {
        let host = Checklist.makeTemporaryHost()
        await host.start()
        guard let core = host.core else {
            print("Checklist failed to open: \(host.status)")
            exit(1)
        }
        let model = ChecklistModel(core: core)
        await model.add("Try Checklist on Linux")
        print("Checklist opened: \(model.items.count) item, revision \(core.revision())")
        host.report(.terminated)
    }
}
#endif
