@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI

/// The app's root: three tabs. Apps (the hosted apps, what the phone uses
/// Device Lab for) comes first; This iPhone is the sensor dashboard; Tools
/// holds the experiments and developer checks. It owns the dashboard's model,
/// keeps it following Rust, and runs the sensors only while This iPhone is
/// on screen.
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
    /// The tab on screen, kept across launches.
    @SceneStorage("tab") private var tab = LabTab.apps

    public init() {}

    public var body: some View {
        TabView(selection: $tab) {
            NavigationStack {
                AppsView(open: $openApp)
            }
            .tabItem {
                Label("Apps", systemImage: "square.grid.2x2")
                    .accessibilityIdentifier("tab.apps")
            }
            .tag(LabTab.apps)

            NavigationStack {
                DashboardView(model: model)
            }
            .tabItem {
                Label("This iPhone", systemImage: "iphone")
                    .accessibilityIdentifier("tab.iphone")
            }
            .tag(LabTab.iphone)

            NavigationStack {
                ToolsView(model: model)
            }
            .tabItem {
                Label("Tools", systemImage: "wrench.and.screwdriver")
                    .accessibilityIdentifier("tab.tools")
            }
            .tag(LabTab.tools)
        }
        // Full screen, so an app looks and behaves as it does on its own;
        // the workshop button on its edge comes back here or switches to
        // another app (changing `openApp` replaces the cover).
        .fullScreenCover(item: $openApp) { app in
            HostedAppView(app: app, settings: $workshop, switchTo: { openApp = $0 }) {
                openApp = nil
            }
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
        // Another tab, or a hosted app open on top, stops them.
        .onChange(of: scenePhase, initial: true) { _, phase in
            model.setInForeground(phase != .background)
            updateSensors()
        }
        .onChange(of: openApp) { _, _ in updateSensors() }
        .onChange(of: tab) { _, _ in updateSensors() }
    }

    private func updateSensors() {
        model.setActive(scenePhase != .background && openApp == nil && tab == .iphone)
    }
}

/// Device Lab's tabs.
enum LabTab: String {
    case apps, iphone, tools
}
