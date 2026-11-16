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
            RelayScreen(core: core, changes: host.changes)
        }
        .forgeTheme(.standard)
    }
}

/// Owns the screens' model for as long as the core is open.
private struct RelayScreen: View {
    @State private var model: RelayModel
    let changes: UInt64

    init(core: RelayCore, changes: UInt64) {
        _model = State(initialValue: RelayModel(core: core))
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
/// Screen 1: paired devices (with whether each is nearby now) and the
/// history. While it is on screen, Relay listens and advertises itself.
struct DevicesView: View {
    @Bindable var model: RelayModel
    let changes: UInt64

    @State private var network = RelayNetwork()
    @State private var showingCode = false
    @State private var scanning = false
    @State private var receivingCode = false
    @State private var sendingCode = false
    @State private var renaming = false
    @State private var newName = ""
    @State private var preview: URL?
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(model.peers) { peer in
                        NavigationLink(value: peer.deviceId) {
                            PeerRow(peer: peer, nearby: network.nearby.contains(peer.deviceId))
                        }
                        .accessibilityIdentifier("devices.peer")
                        .swipeActions {
                            Button("Forget", systemImage: "trash", role: .destructive) {
                                model.askToForget(peer)
                            }
                            .accessibilityIdentifier("devices.forget")
                        }
                    }
                } header: {
                    Text("Paired Devices")
                } footer: {
                    Text(listeningNote)
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
            .navigationDestination(for: String.self) { id in
                if let peer = model.peers.first(where: { $0.deviceId == id }) {
                    PeerView(model: model, network: network, peer: peer, preview: $preview)
                } else {
                    ContentUnavailableView("Device Forgotten", systemImage: "iphone.slash")
                }
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Menu {
                        Button("Show My Code", systemImage: "qrcode") { showingCode = true }
                            .accessibilityIdentifier("devices.showCode")
                        Button("Scan a Code", systemImage: "qrcode.viewfinder") { scanning = true }
                            .accessibilityIdentifier("devices.scan")
                        Divider()
                        Button("Receive with a Code", systemImage: "arrow.down.circle") { receivingCode = true }
                            .accessibilityIdentifier("devices.receiveCode")
                        Button("Send with a Code", systemImage: "arrow.up.circle") { sendingCode = true }
                            .accessibilityIdentifier("devices.sendCode")
                    } label: {
                        Label("Pair a Device", systemImage: "plus")
                    }
                    .accessibilityIdentifier("devices.pair")
                }
                ToolbarItem(placement: .topBarLeading) {
                    Button("Rename This Device", systemImage: "pencil") {
                        newName = model.identity.name
                        renaming = true
                    }
                    .accessibilityIdentifier("devices.rename")
                }
            }
            .sheet(isPresented: $showingCode) {
                ShowCodeSheet(model: model)
            }
            .sheet(isPresented: $scanning) {
                ScanCodeSheet(model: model)
            }
            .sheet(isPresented: $receivingCode) {
                ReceiveCodeSheet(model: model, network: network, preview: $preview)
            }
            .sheet(isPresented: $sendingCode) {
                SendCodeSheet(model: model, network: network)
            }
            .alert("Rename This Device", isPresented: $renaming) {
                TextField("Name", text: $newName)
                    .accessibilityIdentifier("rename.name")
                Button("Rename") { rename() }
                    .accessibilityIdentifier("rename.save")
                Button("Cancel", role: .cancel) {}
                    .accessibilityIdentifier("rename.cancel")
            } message: {
                Text("Other devices see this name.")
            }
            .quickLookPreview($preview)
            .sensoryFeedback(.success, trigger: model.saveCount)
            // Progress of what is on its way, twice a second.
            .task(id: model.isTransferring) {
                while model.isTransferring, (try? await Task.sleep(for: .milliseconds(500))) != nil {
                    model.reload()
                }
            }
        }
        // On the stack, not its first screen: the device screen it pushes asks
        // to forget a device, and an alert on the root showed behind it.
        // While a sheet is up, the sheet shows the alerts.
        .forgeAlert(Binding(
            get: { showingCode || scanning || receivingCode || sendingCode ? nil : model.alert },
            set: { model.alert = $0 }
        )) { action in Task { await model.handle(action) } }
        .onAppear(perform: open)
        .onDisappear(perform: close)
        .onChange(of: scenePhase) {
            // iOS suspends the port in the background anyway.
            if scenePhase == .active { open() } else if scenePhase == .background { close() }
        }
    }

    private var listeningNote: String {
        if model.port != nil {
            return "“\(model.identity.name)” is open to paired devices on this Wi-Fi while Relay is on screen."
        }
        return "Relay can't open its port, so other devices can't reach this one."
    }

    @ViewBuilder private var emptyState: some View {
        if model.peers.isEmpty && model.transfers.isEmpty {
            ForgeStateView.empty(
                "No Devices", systemImage: "arrow.left.arrow.right",
                description: "Pair once by QR code, then send notes and files straight to the other device, encrypted. Both need Relay open on the same Wi-Fi.",
                action: ForgeStateAction("Show My Code", identifier: "devices.showCodeFirst") { showingCode = true }
            )
        }
    }

    private func open() {
        if let port = model.startListening() {
            network.start(name: RelayFormat.serviceName(for: model.identity), port: port)
        }
    }

    private func close() {
        network.stop()
        model.stopListening()
    }

    private func rename() {
        let name = newName
        Task {
            do { try await model.rename(to: name) } catch { model.report(error) }
        }
    }
}

private struct PeerRow: View {
    let peer: Peer
    let nearby: Bool

    var body: some View {
        HStack {
            Image(systemName: "iphone.gen3")
                .foregroundStyle(nearby ? Color.accentColor : .secondary)
            VStack(alignment: .leading) {
                Text(peer.name)
                Text(nearby ? "Nearby" : "Not seen on this network")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// One note or file in the history. A received file opens in Quick Look; a
/// note can be copied.
struct TransferRow: View {
    let transfer: Transfer
    let model: RelayModel
    @Binding var preview: URL?

    var body: some View {
        Button {
            if case .file(_, _, let path?) = transfer.item {
                preview = URL(fileURLWithPath: path)
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
            Button("Remove from History", systemImage: "trash", role: .destructive) {
                Task { await model.remove(transfer) }
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

    private var isFailed: Bool {
        if case .failed = transfer.status { return true }
        return false
    }
}
#endif
