@preconcurrency import RelayFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import ForgeUI
import SwiftUI
#if os(iOS)
import QuickLook
import UIKit
#endif

/// The app's root. ForgeKit's `ForgeRoot` shows a spinner while the Rust
/// core opens, the devices once it has, or a recovery screen if it failed.
public struct ContentView: View {
    @Bindable private var host: ForgeHost<RelayCore>

    public init(host: ForgeHost<RelayCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            RelayScreen(core: core, host: host, changes: host.changes)
        }
        .forgeTheme(.standard)
    }
}

/// Owns the screens' model for as long as the core is open.
private struct RelayScreen: View {
    @State private var model: RelayModel
    let changes: UInt64

    init(core: RelayCore, host: ForgeHost<RelayCore>, changes: UInt64) {
        let model = RelayModel(core: core)
        model.follow(host)
        _model = State(initialValue: model)
        self.changes = changes
    }

    var body: some View {
        #if os(iOS)
        DevicesView(model: model, changes: changes)
            .onChange(of: changes) { model.reload() }
        #else
        // Bonjour, the scanner and the share sheet are iOS only; the macOS
        // build exists for the model tests.
        Text("Relay needs an iPhone.")
        #endif
    }
}

#if os(iOS)
/// Screen 1: trusted devices and the history. While it is on screen, Relay
/// advertises its iroh endpoint for trusted devices and looks for them.
struct DevicesView: View {
    @Bindable var model: RelayModel
    let changes: UInt64

    @State private var network = RelayNetwork()
    @State private var receivingCode = false
    @State private var sendingCode = false
    @State private var renaming = false
    @State private var preview: URL?

    var body: some View {
        NavigationStack {
            notices(deviceList)
            .onChange(of: anySheet) { if !anySheet { model.refreshTrustOffer() } }
            .quickLookPreview($preview)
            .forgeFeedback(model.feedback)
            // Progress of what is on its way, twice a second; what waits in
            // the outbox, every two seconds, to see it go.
            .task(id: model.isTransferring || model.isWaiting) {
                while model.isTransferring || model.isWaiting,
                      (try? await Task.sleep(for: .milliseconds(model.isTransferring ? 500 : 2000))) != nil {
                    model.reload()
                }
            }
            // Presence ages: "reachable" turns "offline" a few minutes after
            // the last connection, with nothing else changing.
            .task {
                while (try? await Task.sleep(for: .seconds(30))) != nil {
                    model.refreshDevices()
                }
            }
            // The relay can go down or come back with nothing else changing:
            // its notice follows within seconds.
            .task {
                while (try? await Task.sleep(for: .seconds(5))) != nil {
                    model.refreshServerHealth()
                }
            }
        }
        // On the stack, not its first screen: the device screen it pushes asks
        // to forget a device, and an alert on the root showed behind it.
        // While a sheet is up, the sheet shows the alerts.
        .forgeAlert(Binding(
            get: { anySheet ? nil : model.alert },
            set: { model.alert = $0 }
        )) { action in Task { await model.handle(action) } }
        .onAppear(perform: open)
        .onDisappear(perform: close)
        // The model follows the app's phases: iOS suspends the adverts and
        // browsing in the background anyway.
        .onChange(of: model.isAway) { _, away in
            if away { close() } else { open() }
        }
    }

    /// The list, its toolbar, the code sheets and renaming, apart from
    /// `body` so the compiler can type-check each part in time.
    private var deviceList: some View {
        List {
            // At the top, not only under Connections: a relay that can't be
            // reached is why a device away from home doesn't answer.
            if model.connectionMode == .hybrid, model.serverHealth == .unreachable {
                Section {
                    ServersDownNotice()
                }
            }
            if !model.trustedDevices.isEmpty {
                Section {
                    ForEach(model.trustedDevices) { device in
                        NavigationLink(value: TrustedRoute(endpointID: device.endpointId)) {
                            TrustedDeviceRow(device: device)
                        }
                        .accessibilityIdentifier("devices.trusted")
                    }
                } header: {
                    Text("Trusted Devices")
                } footer: {
                    Text("“\(model.identity.name)” is open to trusted devices while Relay is on screen.")
                }
            }
            if !model.revokedDevices.isEmpty {
                Section("Revoked") {
                    ForEach(model.revokedDevices) { device in
                        RevokedDeviceRow(device: device, model: model)
                    }
                }
            }
            if !isEmpty {
                PairingPolicySection(model: model)
                ConnectionModeSection(model: model, showsHealth: false)
            }
            if !model.transfers.isEmpty {
                Section("History") {
                    ForEach(model.transfers) { transfer in
                        TransferRow(transfer: transfer, model: model, preview: $preview)
                    }
                }
            }
        }
        .overlay { emptyState }
        .navigationTitle("Relay")
        .navigationDestination(for: TrustedRoute.self) { route in
            TrustedDeviceView(model: model, endpointID: route.endpointID, preview: $preview)
        }
        .toolbar {
            // Send and Receive work with anyone on this Wi-Fi, by word
            // code; trusted devices are reached from their own screen.
            ToolbarItemGroup(placement: .bottomBar) {
                Button("Receive", systemImage: "arrow.down.circle") { receivingCode = true }
                    .labelStyle(.titleAndIcon)
                    .accessibilityIdentifier("relay.receive")
                Spacer()
                Button("Send", systemImage: "arrow.up.circle") { sendingCode = true }
                    .labelStyle(.titleAndIcon)
                    .accessibilityIdentifier("relay.send")
            }
            ToolbarItem(placement: .topBarLeading) {
                Button("Rename This Device", systemImage: "pencil") { renaming = true }
                .accessibilityIdentifier("devices.rename")
            }
        }
        .sheet(isPresented: $receivingCode) {
            ReceiveCodeSheet(model: model, network: network, preview: $preview)
        }
        .sheet(isPresented: $sendingCode) {
            SendCodeSheet(model: model, network: network)
        }
        .forgeRename(
            "Rename This Device", isPresented: $renaming, name: model.identity.name,
            id: "rename", message: "Other devices see this name."
        ) { name in
            try await model.rename(to: name)
        }
    }

    /// What Relay tells the person once the screen is clear.
    private func notices(_ content: some View) -> some View {
        content
            // "Trust this device?" after a code transfer, once no sheet is up
            // (the code sheets end first; the offer waits for them).
            .alert(
                trustOfferTitle,
                isPresented: Binding(
                    get: { showingTrustOffer },
                    // Trust and Not Now answer it; nothing else closes it.
                    set: { _ in }
                ),
                presenting: model.trustOffer
            ) { offer in
                Button("Trust") { Task { await model.trust(offer) } }
                    .accessibilityIdentifier("trustOffer.trust")
                Button("Not Now", role: .cancel) { model.dismissTrustOffer() }
                    .accessibilityIdentifier("trustOffer.notNow")
            } message: { offer in
                if let introducer = offer.introducedBy {
                    Text("\(introducer) trusts it. You'll be able to send to each other without a code. You can revoke this at any time.")
                } else {
                    Text("You'll be able to send to each other without a code. You can revoke this at any time.")
                }
            }
            // Saved data that couldn't be read when Relay opened: said once
            // the screen is clear, until the person dismisses it.
            .alert(
                "Some Saved Data Couldn't Be Read",
                isPresented: Binding(
                    get: { showingSavedDataNotice },
                    set: { _ in }
                ),
                presenting: model.savedDataNotice
            ) { _ in
                Button("OK") { Task { await model.dismissSavedDataNotice() } }
                    .accessibilityIdentifier("savedData.ok")
            } message: { notice in
                Text(notice.message)
            }
            // An upgrade turned Relay's servers on: said once.
            .alert(
                "Relay Works Away From Home",
                isPresented: Binding(
                    get: { showingAwayNotice },
                    set: { _ in }
                ),
                presenting: model.awayNotice
            ) { _ in
                Button("OK") { Task { await model.dismissAwayNotice() } }
                    .accessibilityIdentifier("awayNotice.ok")
            } message: { notice in
                Text(notice)
            }
    }

    private var isEmpty: Bool {
        model.devices.isEmpty && model.transfers.isEmpty
    }

    /// Alerts and offers wait while any sheet is up, the rename sheet too.
    private var anySheet: Bool {
        receivingCode || sendingCode || renaming
    }

    private var showingTrustOffer: Bool {
        model.trustOffer != nil && !anySheet && model.alert == nil && model.savedDataNotice == nil
            && model.awayNotice == nil
    }

    private var showingSavedDataNotice: Bool {
        model.savedDataNotice != nil && !anySheet && model.alert == nil
    }

    private var showingAwayNotice: Bool {
        model.awayNotice != nil && model.savedDataNotice == nil && !anySheet && model.alert == nil
    }

    private var trustOfferTitle: String {
        model.trustOffer.map { "Trust “\($0.name)”?" } ?? "Trust This Device?"
    }

    @ViewBuilder private var emptyState: some View {
        if isEmpty {
            ForgeStateView.empty(
                "No Devices", systemImage: "arrow.left.arrow.right",
                description: "Send something with a code once, then trust the other device to send notes and files straight to it, encrypted.",
                action: ForgeStateAction("Send with a Code", identifier: "devices.sendFirst") { sendingCode = true }
            )
        }
    }

    private func open() {
        // Trusted devices reach this one on its iroh endpoint, found by
        // Bonjour (`d=1`); Rust keeps only the devices it trusts.
        network.watchDevices { [model] nearby in model.devicesNearby(nearby) }
        Task {
            let service = await model.deviceService()
            network.advertiseDevice(service)
        }
    }

    private func close() {
        network.advertiseDevice(nil)
        network.stopWatchingDevices()
    }
}

/// One note or file in the history. A received file opens in Quick Look; a
/// note can be copied.
struct TransferRow: View {
    let transfer: Transfer
    let model: RelayModel
    @Binding var preview: URL?
    @Environment(\.openURL) private var openURL

    var body: some View {
        Button {
            if case .file(_, _, let path?) = transfer.item {
                ReceivedItem.open(path, preview: &preview, openURL: openURL)
            }
        } label: {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: icon)
                    .foregroundStyle(transfer.direction == .sent ? Color.accentColor : .green)
                    .frame(width: 24)
                VStack(alignment: .leading, spacing: 2) {
                    Text(RelayFormat.title(transfer.item))
                        .lineLimit(3)
                    HStack {
                        Text(transfer.direction == .sent ? "To \(transfer.peerName)" : "From \(transfer.peerName)")
                        if case .file(_, let size, _) = transfer.item {
                            Text(RelayFormat.size(size))
                        }
                        Text(RelayFormat.date(transfer.atMs), format: .dateTime.hour().minute())
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    if case .done = transfer.status {} else {
                        Text(RelayFormat.status(transfer))
                            .font(.caption)
                            .foregroundStyle(isFailed ? .red : .secondary)
                    }
                }
            }
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("history.row")
        .contextMenu {
            if case .note(let text) = transfer.item {
                Button("Copy", systemImage: "doc.on.doc") { UIPasteboard.general.string = text }
                    .accessibilityIdentifier("history.copy")
            }
            if transfer.direction == .sent, isPending {
                Button("Cancel Sending", systemImage: "xmark.circle", role: .destructive) {
                    model.cancel(transfer)
                }
                .accessibilityIdentifier("history.cancel")
            }
            Button("Remove from History", systemImage: "trash", role: .destructive) {
                Task { await model.askToRemove(transfer) }
            }
            .accessibilityIdentifier("history.remove")
        }
    }

    private var icon: String {
        switch transfer.item {
        case .note: transfer.direction == .sent ? "text.bubble" : "text.bubble.fill"
        case .file: transfer.direction == .sent ? "arrow.up.doc" : "arrow.down.doc"
        }
    }

    /// Waiting in the outbox, or on its way.
    private var isPending: Bool {
        switch transfer.status {
        case .waiting, .inProgress: true
        case .done, .failed: false
        }
    }

    private var isFailed: Bool {
        if case .failed = transfer.status { return true }
        return false
    }
}
/// Opening a received file or folder: a file in Quick Look, a folder in the
/// Files app (Quick Look can't show a folder).
enum ReceivedItem {
    static func isFolder(_ path: String) -> Bool {
        var folder: ObjCBool = false
        return FileManager.default.fileExists(atPath: path, isDirectory: &folder) && folder.boolValue
    }

    /// The Files app at `path` (`shareddocuments://`), for something in this
    /// app's Documents.
    static func filesURL(_ path: String) -> URL? {
        let file = URL(fileURLWithPath: path).absoluteString
        return URL(string: file.replacingOccurrences(of: "file://", with: "shareddocuments://"))
    }

    static func open(_ path: String, preview: inout URL?, openURL: OpenURLAction) {
        if isFolder(path), let url = filesURL(path) {
            openURL(url)
        } else {
            preview = URL(fileURLWithPath: path)
        }
    }
}
#endif
