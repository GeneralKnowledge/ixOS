import ChecklistApp
import CommonplaceApp
import CommonplaceFFI
import FieldKitApp
import FieldKitFFI
import ForgeFFI
import ForgeKit
import ForgeNative
import ForgeUIShowcase
import GoodDogApp
import GoodDogFFI
import PocketLibraryApp
import PocketLibraryFFI
import RelayApp
import RelayFFI
import SketchbookApp
import SoundscapeApp
import SoundscapeFFI
import UrduNaturallyApp
import SwiftUI
import WeatherApp
import WeatherFFI

/// The other example apps, run inside Device Lab: one app, one Rust library
/// (Device Lab's, which links their Rust cores) and one ForgeKit, so the
/// showcase checks that they all run on the same system. Each app is its own
/// Swift package (examples/<app>), unchanged. What was hard
/// about putting them together is in https://github.com/GeneralKnowledge/ixOS/blob/6de2125c63803063e706f511abe15ecf05025d3c/docs/research/showcase-friction-log.md.
struct AppsView: View {
    @Binding var open: EmbeddedApp?
    @AppStorage(EmbeddedApp.launchKey) private var launchApp = ""

    var body: some View {
        List {
            BuildSection(info: .current)
            Section {
                ForEach(EmbeddedApp.allCases) { app in
                    Button {
                        open = app
                    } label: {
                        AppRow(app: app)
                    }
                    .accessibilityIdentifier("apps.\(app.rawValue)")
                    .accessibilityHint("Opens \(app.name) full screen. The Workshop button on its edge comes back.")
                }
            } footer: {
                Text("Each app is the full example app, running full screen in Device Lab. The round workshop button on its edge comes back here, and shows the app light or dark, at another text size, or at iPhone SE size.")
            }
            Section {
                NavigationLink {
                    PatternsView()
                } label: {
                    Label {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Interaction Patterns")
                            Text("ForgeAppKit's banner and haptics, with every variant.")
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                        }
                    } icon: {
                        Image(systemName: "hand.tap")
                    }
                    .accessibilityElement(children: .combine)
                }
                .accessibilityIdentifier("apps.patterns")
            } footer: {
                Text("The ForgeUI Showcase above has the visual components; these are the shared behaviours.")
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
                Text("Device Lab opens straight into this app. Its workshop button gets back to Device Lab.")
            }
        }
        .groupedList()
        .navigationTitle("Apps")
    }
}

/// The apps the showcase runs, in the order the list shows them.
enum EmbeddedApp: String, CaseIterable, Identifiable {
    case weather, checklist, library, goodDog, commonplace, fieldKit, sketchbook, soundscape, relay, urdu, forgeUI

    /// The UserDefaults key of the app to open at launch.
    static let launchKey = "launchApp"

    var id: String { rawValue }

    var name: String {
        switch self {
        case .weather: "Weather"
        case .checklist: "Checklist"
        case .library: "Pocket Library"
        case .goodDog: "Good Dog"
        case .commonplace: "Commonplace"
        case .fieldKit: "FieldKit"
        case .sketchbook: "Sketchbook"
        case .soundscape: "Soundscape"
        case .relay: "Relay"
        case .urdu: "Urdu, Naturally"
        case .forgeUI: "ForgeUI Showcase"
        }
    }

    var summary: String {
        switch self {
        case .weather: "Forecasts over the network, cached for offline use."
        case .checklist: "A list kept on the phone, on the application runtime."
        case .library: "A reading tracker with search, EPUB import and read-aloud."
        case .goodDog: "A daily plan of training and care for your dog."
        case .commonplace: "A feed reader that keeps articles to read offline."
        case .fieldKit: "A field notebook: record a GPS track and take notes along the way."
        case .sketchbook: "Draw with your finger in layers, with undo, and export to PNG or PDF."
        case .soundscape: "A music player for your songs from your computer: albums, a queue, lock-screen playback, and takes to compare and pick."
        case .relay: "Messages through your chat server with an on-device assistant, and files, photos and folders straight to a nearby device."
        case .urdu: "Conversational Pakistani Urdu, learnt by talking: Roman Urdu, conversations with Ali, Ayesha and Omar, listening and review."
        case .forgeUI: "Every ForgeUI component, with example screens."
        }
    }

    var systemImage: String {
        switch self {
        case .weather: "cloud.sun"
        case .checklist: "checklist"
        case .library: "books.vertical"
        case .goodDog: "pawprint"
        case .commonplace: "newspaper"
        case .fieldKit: "map"
        case .sketchbook: "scribble.variable"
        case .soundscape: "waveform"
        case .relay: "arrow.left.arrow.right"
        case .urdu: "bubble.left.and.text.bubble.right"
        case .forgeUI: "paintpalette"
        }
    }

    @MainActor @ViewBuilder
    var content: some View {
        switch self {
        case .weather: WeatherRoot(host: EmbeddedApps.weather)
        case .checklist: ChecklistRoot(host: EmbeddedApps.checklist)
        case .library: PocketLibraryApp.ContentView(model: EmbeddedApps.library)
        case .goodDog: GoodDogApp.ContentView(host: EmbeddedApps.goodDog)
        case .commonplace: CommonplaceApp.ContentView(host: EmbeddedApps.commonplace)
        case .fieldKit: FieldKitApp.ContentView(host: EmbeddedApps.fieldKit)
        case .sketchbook: SketchbookApp.ContentView(host: EmbeddedApps.sketchbook)
        case .soundscape: SoundscapeApp.ContentView(host: EmbeddedApps.soundscape)
        case .relay: RelayApp.RelayRoot(host: EmbeddedApps.relay, chat: EmbeddedApps.chat)
        case .urdu: UrduNaturallyApp.ContentView(host: EmbeddedApps.urdu)
        case .forgeUI: ShowcaseRoot()
        }
    }
}

/// Which build is installed, at the top of the home screen, so a phone
/// sitting can check at a glance that the new build arrived (the build number
/// changes with every `forge run ios`).
private struct BuildSection: View {
    let info: ForgeBuildInfo

    var body: some View {
        Section {
            LabeledContent("This Build", value: info.versionLine)
                .monospacedDigit()
                .accessibilityIdentifier("apps.build.version")
            if let source = info.sourceLine {
                Text(source)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                    .accessibilityIdentifier("apps.build.source")
            }
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
    /// Weather, Good Dog, Checklist, Pocket Library, Commonplace, FieldKit, Sketchbook, Soundscape, Relay and Urdu, Naturally run on the application
    /// runtime: each host owns its Rust core for the whole process, as in
    /// the standalone app, so closing the app doesn't close the core.
    static let weather = WeatherApp.makeAppHost()
    static let goodDog = GoodDogApp.makeAppHost()
    static let checklist = Checklist.makeHost()
    static let library = LibraryModel()
    static let commonplace = CommonplaceApp.makeAppHost()
    static let fieldKit = FieldKitApp.makeAppHost()
    static let sketchbook = SketchbookApp.makeAppHost()
    static let soundscape = SoundscapeApp.makeAppHost()
    static let relay = RelayApp.makeAppHost()
    static let urdu = UrduNaturallyApp.makeAppHost()
    /// One messaging core for the process (Relay's Chats), so its room connections and message
    /// notifications don't multiply each time the app opens.
    static let chat = RelayApp.RelayRoot.makeChatModel()

    /// The servers the hosted apps talk to, at the addresses the apps use
    /// now (Tools, Servers).
    static func servers() -> [ServersModel.Server] {
        [
            ServersModel.Server(
                app: "Relay Chats", role: "Chat server",
                address: chat.account?.serverUrl,
                setUp: "Sign up in Relay's Chats tab first."),
            ServersModel.Server(
                app: "Soundscape", role: "Take server",
                address: soundscape.core?.listenServer(),
                setUp: "Add the address in Soundscape's Listen screen first."),
        ]
    }

    /// Hands `url` to the hosted app whose URL scheme it uses. Each app's
    /// bindings have their own `ForgeDeepLinks`, so a link reaches an app
    /// only through its own. Returns false for links that are Device Lab's.
    static func openDeepLink(_ url: URL) -> Bool {
        switch url.scheme {
        case "forgechat": RelayFFI.ForgeDeepLinks.shared.open(url)
        default: false
        }
    }

    /// Call once, from the App's init, after `AppServices.install()`.
    public static func install() {
        // Each app with native services has its own Rust side for them, so
        // each installs its own adapters. Secrets are kept apart by Keychain
        // service name; everything else (notifications, deep links) is the
        // process's, and the last app to install wins.
        let bundle = Bundle.main.bundleIdentifier ?? "org.forgemobile.devicelab"
        PocketLibraryFFI.ForgeServices.install(keychainService: "\(bundle).pocketlibrary")
        // Good Dog's services are the shared forge-native crate's, whose
        // adapters are ForgeKit's ForgeNative module.
        ForgeNative.ForgeServices.install(keychainService: "\(bundle).gooddog")
        CommonplaceFFI.ForgeServices.install(keychainService: "\(bundle).commonplace")
        // Relay's messaging keeps the Keychain service Chat had, so the
        // account made before Chat merged into Relay is still there.
        RelayFFI.ForgeServices.install(keychainService: "\(bundle).chat")
        FieldKitFFI.ForgeServices.install(keychainService: "\(bundle).fieldkit")
        WeatherFFI.ForgeServices.install(keychainService: "\(bundle).weather")
        // Soundscape's Listen tab asks its take server through URLSession.
        SoundscapeFFI.ForgeServices.install(keychainService: "\(bundle).soundscape")
        #if canImport(UIKit)
            ForgeLifecycle.attach(weather)
            ForgeLifecycle.attach(goodDog)
            ForgeLifecycle.attach(checklist)
            ForgeLifecycle.attach(commonplace)
            ForgeLifecycle.attach(fieldKit)
            ForgeLifecycle.attach(sketchbook)
            ForgeLifecycle.attach(soundscape)
            ForgeLifecycle.attach(relay)
            ForgeLifecycle.attach(urdu)
            ForgeLifecycle.attach(chat.host)
        #endif
        library.attachToLifecycle()
    }
}
