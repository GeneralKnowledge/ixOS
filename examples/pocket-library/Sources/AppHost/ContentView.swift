@preconcurrency import PocketLibraryFFI
import ForgeAppKit
import ForgeKit
import ForgeUI
import SwiftUI

/// The app's root: three tabs over one library. The App owns the model
/// (and so the Rust library) for the life of the process, and attaches its
/// host to the lifecycle.
public struct ContentView: View {
    @Bindable private var model: LibraryModel
    /// The app reopens on the tab it was left on. A launch argument
    /// (`-selectedTab activity`) overrides it, which is how the screenshot
    /// job opens each tab without UI automation.
    @AppStorage("selectedTab") private var selectedTab = "library"

    public init(model: LibraryModel) {
        self.model = model
    }

    public var body: some View {
        Group {
            switch model.phase {
            case .opening:
                ProgressView()
            case .failed(let message, let canStartFresh):
                OpenFailedView(message: message, canStartFresh: canStartFresh, model: model)
            case .open:
                TabView(selection: $selectedTab) {
                    LibraryView()
                        .tabItem {
                            Label("Library", systemImage: "books.vertical")
                                .accessibilityIdentifier("tab.library")
                        }
                        .tag("library")
                    ActivityView()
                        .tabItem {
                            Label("Activity", systemImage: "chart.bar")
                                .accessibilityIdentifier("tab.activity")
                        }
                        .tag("activity")
                    SettingsView()
                        .tabItem {
                            Label("Settings", systemImage: "gearshape")
                                .accessibilityIdentifier("tab.settings")
                        }
                        .tag("settings")
                }
            }
        }
        .environment(model)
        // ForgeUI's theme for the whole app: tokens in the environment, and
        // its accent on plain SwiftUI controls.
        .forgeTheme(.standard)
        .task { await model.open() }
        // Rust committed something, missed events were caught up, or the
        // app came back from the background: read everything again.
        .onChange(of: model.host.changes) { model.reload() }
    }
}

extension LibraryModel {
    /// Reports the application lifecycle to the Rust library: searches stop
    /// in the background, imports and exports finish in the time iOS gives
    /// (`beginBackgroundTask`), and the screens re-read on the way back.
    /// Call once, from the App's init.
    public func attachToLifecycle() {
        #if canImport(UIKit)
        ForgeLifecycle.attach(host)
        #endif
    }
}

/// Shown when the saved library cannot be read. Nothing is deleted without
/// the user saying so, and even then the old file is kept as a backup.
private struct OpenFailedView: View {
    enum Action { case startFresh }

    let message: String
    let canStartFresh: Bool
    let model: LibraryModel
    @State private var alert: ForgeAlert<Action>?

    var body: some View {
        VStack(spacing: 0) {
            ForgeStateView.failed(
                "Can't Open Library",
                message: message,
                retry: ForgeStateAction("Try Again", identifier: "open.retry") { Task { await model.open() } }
            )
            if canStartFresh {
                Button("Start a New Library…") {
                    alert = .confirm(
                        "Start a new, empty library?",
                        message: "The unreadable library is kept as a backup file in the app's storage.",
                        confirm: "Start New Library",
                        action: .startFresh,
                        identifier: "open.startFresh",
                        destructive: true
                    )
                }
                .buttonStyle(.forgeTertiary)
                .accessibilityIdentifier("open.startFresh")
                .padding(.bottom)
            }
        }
        .forgeAlert($alert) { _ in Task { await model.startFresh() } }
    }
}
