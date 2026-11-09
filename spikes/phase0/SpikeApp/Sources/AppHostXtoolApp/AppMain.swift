#if canImport(SwiftUI)
import AppHost
import ForgeKit
import SwiftUI

// The xtool entry point (xtool wants @main in a library product).
@main
struct SpikeAppXtool: App {
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
#endif
