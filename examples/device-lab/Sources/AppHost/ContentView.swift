@preconcurrency import ForgeFFI
import SwiftUI

/// The app's root: one navigation stack over the dashboard. It owns the
/// model, keeps it following Rust while on screen, and tells Rust when the
/// app goes to the background (everything stops) and comes back.
public struct ContentView: View {
    @State private var model = LabModel()
    @Environment(\.scenePhase) private var scenePhase
    /// The app the Apps screen has open, if any (AppsView).
    @State private var openApp: EmbeddedApp?
    /// The app to open at launch, chosen on the Apps screen. A launch
    /// argument (`-launchApp weather`) sets it too.
    @AppStorage(EmbeddedApp.launchKey) private var launchApp = ""
    @State private var launched = false

    public init() {}

    public var body: some View {
        NavigationStack {
            DashboardView(model: model, openApp: $openApp)
        }
        .sheet(item: $openApp) { app in
            app.content
                .presentationDragIndicator(.visible)
        }
        .onAppear {
            guard !launched else { return }
            launched = true
            openApp = EmbeddedApp(rawValue: launchApp)
        }
        // `-ForgeAppearance dark|light` (LaunchAppearance.swift), applied
        // here rather than with forgeLaunchAppearance() in the App: Weather
        // and Pocket Library each have a public one too, so the call there
        // is ambiguous.
        .preferredColorScheme(ForgeLaunchAppearance.scheme)
        .onOpenURL { url in AppServices.open(url) }
        .task { await model.follow() }
        // Inactive (Control Center pulled down, the app switcher) keeps the
        // sensors running, so toggling Wi-Fi or Bluetooth shows up live.
        // Another app open on top (the Apps screen) stops them too.
        .onChange(of: scenePhase, initial: true) { _, phase in
            model.setActive(phase != .background && openApp == nil)
        }
        .onChange(of: openApp) { _, app in
            model.setActive(scenePhase != .background && app == nil)
        }
    }
}
