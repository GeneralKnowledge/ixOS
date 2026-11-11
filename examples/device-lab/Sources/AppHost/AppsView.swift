import ChecklistApp
import ForgeFFI
import ForgeKit
import ForgeUIShowcase
import PocketLibraryApp
import SwiftUI
import WeatherApp

/// The other example apps, run inside Device Lab: one app, one Rust library
/// (Device Lab's, which links their Rust cores) and one ForgeKit, so the
/// showcase checks that they all run on the same system. Each app's code is
/// its own folder's (symlinked into Sources/), unchanged. What was hard
/// about putting them together is in docs/research/showcase-friction-log.md.
struct AppsView: View {
    @Binding var open: EmbeddedApp?
    @AppStorage(EmbeddedApp.launchKey) private var launchApp = ""

    var body: some View {
        List {
            Section {
                ForEach(EmbeddedApp.allCases) { app in
                    Button {
                        open = app
                    } label: {
                        AppRow(app: app)
                    }
                    .accessibilityIdentifier("apps.\(app.rawValue)")
                    .accessibilityHint("Opens \(app.name). Swipe down to come back.")
                }
            } footer: {
                Text("Each app is the full example app, running in Device Lab. Swipe down to come back.")
            }
            Section {
                Picker("Open at Launch", selection: $launchApp) {
                    Text("Device Lab").tag("")
                    ForEach(EmbeddedApp.allCases) { app in
                        Text(app.name).tag(app.rawValue)
                    }
                }
                .accessibilityIdentifier("apps.launch")
            } footer: {
                Text("Device Lab opens straight into this app. Swipe down to get back to Device Lab.")
            }
        }
        .groupedList()
        .navigationTitle("Apps")
    }
}

/// The apps the showcase runs, in the order the list shows them.
enum EmbeddedApp: String, CaseIterable, Identifiable {
    case weather, checklist, library, forgeUI

    /// The UserDefaults key of the app to open at launch.
    static let launchKey = "launchApp"

    var id: String { rawValue }

    var name: String {
        switch self {
        case .weather: "Weather"
        case .checklist: "Checklist"
        case .library: "Pocket Library"
        case .forgeUI: "ForgeUI Showcase"
        }
    }

    var summary: String {
        switch self {
        case .weather: "Forecasts over the network, cached for offline use."
        case .checklist: "A list kept on the phone, on the application runtime."
        case .library: "A reading tracker with search, EPUB import and read-aloud."
        case .forgeUI: "Every ForgeUI component, with example screens."
        }
    }

    var systemImage: String {
        switch self {
        case .weather: "cloud.sun"
        case .checklist: "checklist"
        case .library: "books.vertical"
        case .forgeUI: "paintpalette"
        }
    }

    @MainActor @ViewBuilder
    var content: some View {
        switch self {
        case .weather: WeatherView()
        case .checklist: ChecklistRoot(host: EmbeddedApps.checklist)
        case .library: PocketLibraryApp.ContentView()
        case .forgeUI: ShowcaseRoot()
        }
    }
}

private struct AppRow: View {
    let app: EmbeddedApp

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: 2) {
                Text(app.name)
                    .foregroundStyle(.primary)
                Text(app.summary)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        } icon: {
            Image(systemName: app.systemImage)
        }
        .accessibilityElement(children: .combine)
    }
}

/// What the embedded apps need for the life of the process.
@MainActor
public enum EmbeddedApps {
    /// Checklist runs on the application runtime: its host owns its Rust
    /// core for the whole process, as in the standalone app, so closing the
    /// sheet doesn't close the checklist.
    static let checklist = Checklist.makeHost()

    /// Call once, from the App's init. ForgeLifecycle reports the app's
    /// lifecycle to one host only (attaching another replaces it); Checklist
    /// is the only embedded app on ForgeKit's host so far.
    public static func attach() {
        #if canImport(UIKit)
            ForgeLifecycle.attach(checklist)
        #endif
    }
}
