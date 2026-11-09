import AppHost
import ForgeKit

#if canImport(SwiftUI)
import SwiftUI

/// The app owns the host (and so the Rust core) for the process.
@main
struct SpikeApp: App {
    @State private var host = ItemsModel.makeHost(environment: try! ForgeEnvironment.standard(app: "SpikeApp"))

    init() {
        #if canImport(UIKit)
        ForgeLifecycle.attach(host)
        #endif
    }

    var body: some Scene {
        WindowGroup {
            ForgeRoot(host) { _ in ContentView(host: host) }
        }
    }
}
#else
/// Linux: no UI; prove the executable links against Rust and ForgeKit.
@main
struct SpikeApp {
    @MainActor
    static func main() async throws {
        let host = ItemsModel.makeHost(environment: try ForgeEnvironment.temporary())
        await host.start()
        print("SpikeApp opened: \(host.status)")
    }
}
#endif
