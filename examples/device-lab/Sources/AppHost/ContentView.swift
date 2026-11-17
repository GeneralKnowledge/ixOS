@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI

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
    /// How the workshop shows hosted apps (WorkshopView.swift), starting
    /// from launch arguments and kept until Device Lab quits.
    @State private var workshop = WorkshopSettings.fromLaunch()

    public init() {}

    public var body: some View {
        NavigationStack {
            DashboardView(model: model, openApp: $openApp)
        }
        // Full screen, so an app looks and behaves as it does on its own;
        // the workshop button on its edge comes back here.
        .fullScreenCover(item: $openApp) { app in
            HostedAppView(app: app, settings: $workshop) { openApp = nil }
        }
        .onAppear {
            guard !launched else { return }
            launched = true
            // Chat merged into Relay: an "Open at Launch" of Chat opens Relay.
            openApp = EmbeddedApp(rawValue: launchApp == "chat" ? "relay" : launchApp)
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
            model.setInForeground(phase != .background)
            model.setActive(phase != .background && openApp == nil)
        }
        .onChange(of: openApp) { _, app in
            model.setActive(scenePhase != .background && app == nil)
        }
    }
}
