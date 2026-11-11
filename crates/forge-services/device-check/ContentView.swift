@preconcurrency import ForgeFFI
import SwiftUI
import UniformTypeIdentifiers

// Written by scripts/make-services-check-app.sh in place of the starter
// screen: one button per native service, with the results on screen.
public struct ContentView: View {
    public init() {}
    /// The starter's entry points pass the app's runtime host; this screen
    /// talks to the services directly and doesn't need it.
    public init<Host>(host: Host) {}

    @State private var report: [String] = []
    @State private var running = false
    @State private var shareItem: ShareItem?
    @State private var importing = false
    @State private var links: [String] = []
    @State private var responses: [String] = []
    @State private var permission = "unknown"
    @State private var books = 1
    #if os(iOS)
        @State private var webPage: ForgeWebPage?
        @State private var safariURL: URL?
    #endif

    /// Proves the compiled String Catalog is in the bundle: the languages
    /// iOS sees, and the Spanish text read straight from es.lproj.
    private var languageReport: String {
        let spanish = Bundle.main.path(forResource: "es", ofType: "lproj").flatMap(Bundle.init(path:))
        let text = spanish?.localizedString(forKey: "This text follows the iPhone's language", value: "MISSING", table: nil)
        return "Languages: \(Bundle.main.localizations.sorted().joined(separator: ", ")); "
            + "using \(Bundle.main.preferredLocalizations.first ?? "?"); "
            + "es.lproj: \(text ?? "not in the bundle")"
    }

    public var body: some View {
        NavigationStack {
            List {
                Section("1. Rust → Swift services") {
                    Button(running ? "Running…" : "Run checks") {
                        Task {
                            running = true
                            report = await servicesSelfCheck()
                            shareItem = servicesCheckShareItem()
                            let status = await ForgePermissions.shared.status(permission: .notifications)
                            permission = "\(status)"
                            running = false
                        }
                    }
                    .disabled(running)
                    ForEach(report, id: \.self) { line in
                        Text(line)
                            .font(.footnote.monospaced())
                            .foregroundStyle(line.hasPrefix("FAIL") ? Color.red : Color.primary)
                    }
                    Text("Notification permission: \(permission)").font(.footnote)
                    #if os(iOS)
                        Button("Open Settings") { ForgePermissions.openSettings() }
                    #endif
                }
                Section("2. Deep links (tap the notification, or open forgecheck://check/url)") {
                    if links.isEmpty { Text("None yet").foregroundStyle(.secondary) }
                    ForEach(links, id: \.self) { Text($0).font(.footnote.monospaced()) }
                    ForEach(responses, id: \.self) { Text($0).font(.footnote.monospaced()) }
                    Button("Show notification responses") { responses = servicesCheckResponses() }
                }
                Section("3. Share and import") {
                    if let shareItem {
                        ForgeShareLink(item: shareItem, title: "Share the file Rust wrote")
                    } else {
                        Text("Run the checks first").foregroundStyle(.secondary)
                    }
                    Button("Import a file") { importing = true }
                }
                #if os(iOS)
                    Section("4. Web pages (links off the site should open Safari)") {
                        Button("Web page in the app") {
                            webPage = .link(URL(string: "https://www.swift.org/about/")!, title: "Swift")
                        }
                        Button("HTML from the app (no network)") {
                            webPage = .html(
                                "<h1>Forge</h1><p>Shown offline. <img src='https://www.swift.org/favicon.ico' alt='blocked image'> <a href='https://www.swift.org'>This link opens Safari</a>.</p>",
                                title: "HTML")
                        }
                        Button("Safari in the app") { safariURL = URL(string: "https://www.swift.org") }
                    }
                #endif
                Section("5. Languages (Settings > ServicesCheck > Language)") {
                    Text("This text follows the iPhone's language")
                    Stepper("\(books) books", value: $books, in: 0...5)
                        .accessibilityIdentifier("check.books")
                    Text(languageReport).font(.footnote).foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Services check")
            #if os(iOS)
                .forgeWebSheet($webPage)
                .forgeSafari($safariURL)
            #endif
            .onForgeDeepLink { link in
                links.append("PASS \(link.url) → route \(link.route)")
            }
            .forgeFileImporter(isPresented: $importing, allowedContentTypes: [.item]) { result in
                switch result {
                case .success(let url): report.append(servicesCheckReadImport(path: url.path))
                case .failure(let error): report.append("FAIL import: \(error)")
                }
            }
        }
    }
}
