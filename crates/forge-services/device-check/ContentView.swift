@preconcurrency import ForgeFFI
import SwiftUI
import UniformTypeIdentifiers

// Written by scripts/make-services-check-app.sh in place of the starter
// screen: one button per native service, with the results on screen.
public struct ContentView: View {
    public init() {}

    @State private var report: [String] = []
    @State private var running = false
    @State private var shareItem: ShareItem?
    @State private var importing = false
    @State private var links: [String] = []
    @State private var permission = "unknown"

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
                }
                Section("3. Share and import") {
                    if let shareItem {
                        ForgeShareLink(item: shareItem, title: "Share the file Rust wrote")
                    } else {
                        Text("Run the checks first").foregroundStyle(.secondary)
                    }
                    Button("Import a file") { importing = true }
                }
            }
            .navigationTitle("Services check")
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
