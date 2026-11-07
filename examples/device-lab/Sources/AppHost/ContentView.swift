@preconcurrency import ForgeFFI
import SwiftUI

/// The app's root: one navigation stack over the dashboard. It owns the
/// model, keeps it following Rust while on screen, and tells Rust when the
/// app goes to the background (everything stops) and comes back.
public struct ContentView: View {
    @State private var model = LabModel()
    @Environment(\.scenePhase) private var scenePhase

    public init() {}

    public var body: some View {
        NavigationStack {
            DashboardView(model: model)
        }
        .task { await model.follow() }
        // Inactive (Control Center pulled down, the app switcher) keeps the
        // sensors running, so toggling Wi-Fi or Bluetooth shows up live.
        .onChange(of: scenePhase, initial: true) { _, phase in
            model.setActive(phase != .background)
        }
    }
}
