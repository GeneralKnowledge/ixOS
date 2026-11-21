#if os(iOS)
@preconcurrency import RelayFFI
import ForgeAppKit
import ForgeUI
import SwiftUI
import UniformTypeIdentifiers

/// A trusted device in the navigation stack, by endpoint id (paired
/// devices are pushed by their `String` device id).
struct TrustedRoute: Hashable {
    let endpointID: String
}

/// The trust look: a filled shield and the word, so it never rests on
/// colour alone.
struct TrustBadge: View {
    var body: some View {
        Label("Trusted", systemImage: "checkmark.shield.fill")
            .font(.caption)
            .forgeForeground(.success)
    }
}

/// One trusted device in the list: its name, the trust look and whether
/// it is on this network now.
struct TrustedDeviceRow: View {
    let device: Device

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: "laptopcomputer.and.iphone")
                .foregroundStyle(device.presence == .offline ? .secondary : Color.accentColor)
                .frame(width: 28)
            VStack(alignment: .leading, spacing: 2) {
                Text(device.name)
                HStack(spacing: 8) {
                    TrustBadge()
                    Text(RelayModel.whereNow(device))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// A revoked device: listed so the person can see it is refused, and
/// trust it again (the only way it ever is) or forget it.
struct RevokedDeviceRow: View {
    let device: Device
    let model: RelayModel

    var body: some View {
        HStack {
            Image(systemName: "xmark.shield")
                .foregroundStyle(.secondary)
                .frame(width: 28)
            VStack(alignment: .leading, spacing: 2) {
                Text(device.name)
                Text(device.revokedBy.map { "Revoked by \($0): can't send here without a code" }
                     ?? "Revoked: can't send here without a code")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button("Trust Again") {
                Task { await model.trustAgain(device) }
            }
            .buttonStyle(.borderless)
            .accessibilityIdentifier("revoked.trustAgain")
        }
        .swipeActions {
            Button("Forget", systemImage: "trash", role: .destructive) {
                model.askToForget(device)
            }
            .accessibilityIdentifier("revoked.forget")
        }
    }
}

/// What happens after a code transfer (Rust's `PairingPolicy`), on the
/// Devices tab and in Settings.
struct PairingPolicySection: View {
    let policy: PairingPolicy
    let choose: (PairingPolicy) -> Void

    init(model: RelayModel) {
        policy = model.policy
        choose = { policy in Task { await model.setPolicy(policy) } }
    }

    init(policy: PairingPolicy, choose: @escaping (PairingPolicy) -> Void) {
        self.policy = policy
        self.choose = choose
    }

    var body: some View {
        Section {
            Picker("After a Code Transfer", selection: Binding(get: { policy }, set: choose)) {
                Text("Ask to Trust").tag(PairingPolicy.askToPair)
                Text("Trust Again Automatically").tag(PairingPolicy.pairAutomatically)
                Text("Codes Only").tag(PairingPolicy.codesOnly)
            }
            .accessibilityIdentifier("devices.policy")
        } header: {
            Text("Pairing")
        } footer: {
            Text(Self.explain(policy))
        }
    }

    static func explain(_ policy: PairingPolicy) -> String {
        switch policy {
        case .askToPair:
            "After a code transfer, Relay asks whether to trust the other device, so you can send to each other without a code."
        case .pairAutomatically:
            "A device you trusted before, then forgot, is trusted again after a code transfer. New devices are asked about. A revoked device never is."
        case .codesOnly:
            "A code moves one transfer and nothing is remembered."
        }
    }
}

/// Which routes reach trusted devices (Rust's `ConnectionMode`), on the
/// Devices tab and in Settings: one choice in plain words (N2). Relay's
/// servers belong to Everywhere; their Advanced screen sets other ones.
struct ConnectionModeSection: View {
    let mode: ConnectionMode
    let servers: RelayServers
    let choose: (ConnectionMode) -> Void
    let chooseServers: (RelayServers) -> Void

    init(model: RelayModel) {
        mode = model.connectionMode
        servers = model.servers
        choose = { mode in Task { await model.setConnectionMode(mode) } }
        chooseServers = { servers in Task { await model.setServers(servers) } }
    }

    init(
        mode: ConnectionMode,
        servers: RelayServers,
        choose: @escaping (ConnectionMode) -> Void,
        chooseServers: @escaping (RelayServers) -> Void
    ) {
        self.mode = mode
        self.servers = servers
        self.choose = choose
        self.chooseServers = chooseServers
    }

    var body: some View {
        Section {
            Picker("Reach Your Devices", selection: Binding(get: { mode }, set: choose)) {
                Text("Everywhere").tag(ConnectionMode.hybrid)
                Text("Private Networks").tag(ConnectionMode.private)
                Text("This Network Only").tag(ConnectionMode.local)
            }
            .accessibilityIdentifier("devices.connectionMode")
            if mode == .hybrid {
                NavigationLink {
                    RelayServersView(servers: servers, choose: chooseServers)
                } label: {
                    LabeledContent("Servers", value: Self.serversName(servers))
                }
                .accessibilityIdentifier("devices.servers")
            }
        } header: {
            Text("Connections")
        } footer: {
            Text(RelayModel.explain(mode, servers: servers))
        }
    }

    static func serversName(_ servers: RelayServers) -> String {
        switch RelayModel.ServerChoice(servers) {
        case .relays: "Relay's"
        case .own: "Your Own"
        case .none: "None"
        }
    }
}

/// The pairing policy and connection mode in the Settings tab, which has
/// no `RelayModel`: it asks Relay's core directly. The Devices tab
/// re-reads them after the change, like any other.
struct RelaySettingsSection: View {
    let core: RelayCore
    @State private var policy: PairingPolicy = .askToPair
    @State private var mode: ConnectionMode = .hybrid
    @State private var servers = RelayServers(relayUrl: nil, lookupUrl: nil)

    var body: some View {
        PairingPolicySection(policy: policy) { chosen in
            policy = chosen
            Task {
                do {
                    try await core.setPairingPolicy(policy: chosen)
                } catch {
                    policy = core.pairingPolicy()
                }
            }
        }
        .onAppear { policy = core.pairingPolicy() }
        ConnectionModeSection(mode: mode, servers: servers) { chosen in
            mode = chosen
            Task {
                do {
                    try await core.setConnectionMode(mode: chosen)
                } catch {
                    mode = core.connectionMode()
                }
            }
        } chooseServers: { chosen in
            servers = chosen
            Task {
                do {
                    try await core.setServers(servers: chosen)
                } catch {
                    servers = core.servers()
                }
            }
        }
        .onAppear {
            mode = core.connectionMode()
            servers = core.servers()
        }
    }
}

/// Advanced, under Everywhere: Relay's servers (the default), your own
/// relay and lookup server, or none. Applies at once (N2).
struct RelayServersView: View {
    let choose: (RelayServers) -> Void
    @State private var choice: RelayModel.ServerChoice
    @State private var relayURL: String
    @State private var lookupURL: String

    init(servers: RelayServers, choose: @escaping (RelayServers) -> Void) {
        self.choose = choose
        let choice = RelayModel.ServerChoice(servers)
        _choice = State(initialValue: choice)
        _relayURL = State(initialValue: choice == .own ? servers.relayUrl ?? "" : "")
        _lookupURL = State(initialValue: choice == .own ? servers.lookupUrl ?? "" : "")
    }

    var body: some View {
        Form {
            Section {
                Picker("Servers", selection: $choice) {
                    Text("Relay's Servers").tag(RelayModel.ServerChoice.relays)
                    Text("Your Own Servers").tag(RelayModel.ServerChoice.own)
                    Text("No Servers").tag(RelayModel.ServerChoice.none)
                }
                .pickerStyle(.inline)
                .labelsHidden()
                .accessibilityIdentifier("servers.choice")
            } footer: {
                Text(footer)
            }
            if choice == .own {
                Section("Your Own Servers") {
                    TextField("Relay, https://…", text: $relayURL)
                        .textContentType(.URL)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("servers.relayURL")
                    TextField("Lookup, https://…/pkarr", text: $lookupURL)
                        .textContentType(.URL)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("servers.lookupURL")
                    Button("Use These Servers") { choose(ownServers) }
                        .disabled(ownServers.relayUrl == nil && ownServers.lookupUrl == nil)
                        .accessibilityIdentifier("servers.useOwn")
                }
            }
        }
        .navigationTitle("Servers")
        .onChange(of: choice) { _, chosen in
            switch chosen {
            case .relays: choose(ownRelayServers())
            case .none: choose(RelayServers(relayUrl: nil, lookupUrl: nil))
            case .own: break
            }
        }
    }

    private var ownServers: RelayServers {
        func url(_ text: String) -> String? {
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? nil : trimmed
        }
        return RelayServers(relayUrl: url(relayURL), lookupUrl: url(lookupURL))
    }

    private var footer: String {
        switch choice {
        case .relays:
            "relay.avedeus.ovh passes on what no direct way reaches, and lookup.avedeus.ovh tells your trusted devices where to find this one. Both see that your devices connect, never what they send."
        case .own:
            "A relay and lookup server you run (iroh-relay and a pkarr server). The command line sets them with relay servers."
        case .none:
            "Your devices reach each other only when one can connect straight to the other."
        }
    }
}

/// One trusted device: message it or send it files without a code (both
/// land in its conversation in Chats), choose whether it may send here,
/// revoke or forget.
struct TrustedDeviceView: View {
    @Bindable var model: RelayModel
    let endpointID: String
    @Binding var preview: URL?

    @Environment(\.openDeviceConversation) private var openConversation
    @State private var sending = false
    @State private var pickingFiles = false
    @State private var asking = false
    @State private var syncing = false
    @State private var synced: String?

    /// "iPhone or iPad", "Linux", "Mac": the platform Rust reports, in
    /// words.
    static func platformName(_ os: String) -> String {
        switch os {
        case "ios": "iPhone or iPad"
        case "macos": "Mac"
        case "linux": "Linux"
        case "windows": "Windows"
        case "android": "Android"
        default: os
        }
    }

    var body: some View {
        if let device = model.device(endpointID), device.trusted {
            content(device)
        } else {
            ForgeStateView.empty("No Longer Trusted", systemImage: "xmark.shield")
        }
    }

    private func content(_ device: Device) -> some View {
        List {
            Section {
                HStack {
                    if let openConversation {
                        Button {
                            openConversation(device.endpointId)
                        } label: {
                            Label("Message", systemImage: "bubble.left")
                        }
                        .accessibilityIdentifier("trusted.message")
                    }
                    Spacer()
                    Button {
                        pickingFiles = true
                    } label: {
                        Label("Send Files", systemImage: "doc.badge.plus")
                    }
                    .disabled(sending)
                    .accessibilityIdentifier("trusted.sendFiles")
                }
                .buttonStyle(.borderless)
            } header: {
                TrustBadge()
            } footer: {
                Text(RelayModel.reach(device))
            }
            Section {
                LabeledContent("Where", value: RelayModel.whereNow(device))
                    .accessibilityIdentifier("trusted.where")
                LabeledContent("Platform", value: device.platform.map(Self.platformName) ?? "Not known yet")
                    .accessibilityIdentifier("trusted.platform")
                LabeledContent("Offers", value: device.offers.isEmpty ? "Not known yet" : device.offers.joined(separator: ", "))
                    .accessibilityIdentifier("trusted.offers")
                Button {
                    Task {
                        asking = true
                        await model.refreshStatus(of: device)
                        asking = false
                    }
                } label: {
                    Label(asking ? "Asking…" : "Ask Again", systemImage: "arrow.clockwise")
                }
                .disabled(asking)
                .accessibilityIdentifier("trusted.askStatus")
            } header: {
                Text("About This Device")
            } footer: {
                Text("What \(device.name) says about itself. It grants nothing here.")
            }
            Section {
                Toggle("Can Send to This Device", isOn: Binding(
                    get: { device.receivesFrom },
                    set: { on in Task { await model.setReceives(on, from: device) } }
                ))
                .accessibilityIdentifier("trusted.receivesFrom")
            } footer: {
                Text(device.receivesFrom
                     ? "\(device.name) can send messages and files here without a code."
                     : "\(device.name) is still trusted, but what it sends here is declined.")
            }
            Section {
                Toggle("Introduces Devices", isOn: Binding(
                    get: { device.introduces },
                    set: { on in Task { await model.setIntroduces(on, for: device) } }
                ))
                .accessibilityIdentifier("trusted.introduces")
                Toggle("Can See My Devices", isOn: Binding(
                    get: { device.seesDevices },
                    set: { on in Task { await model.setSeesDevices(on, for: device) } }
                ))
                .accessibilityIdentifier("trusted.seesDevices")
                if device.introduces {
                    Button {
                        Task {
                            syncing = true
                            synced = await model.syncDevices(with: device)
                            syncing = false
                        }
                    } label: {
                        Label(syncing ? "Asking…" : "Check Its Devices Now", systemImage: "arrow.triangle.2.circlepath")
                    }
                    .disabled(syncing)
                    .accessibilityIdentifier("trusted.syncDevices")
                }
                if let introducer = device.introducedBy {
                    LabeledContent("Introduced By", value: introducer)
                        .accessibilityIdentifier("trusted.introducedBy")
                }
            } header: {
                Text("Your Devices")
            } footer: {
                Text(synced ?? (device.introduces
                     ? "Devices \(device.name) trusts are offered here for you to trust, and devices it revokes are revoked here too."
                     : "Turn on for your own devices, so you pair and revoke once. Both devices need it on: Introduces here, Can See My Devices there."))
            }
        }
        .navigationTitle(device.name)
        .navigationBarTitleDisplayMode(.inline)
        // Ask once when the device can be reached; Ask Again does it later.
        .task(id: device.endpointId) {
            if device.presence != .offline {
                asking = true
                await model.refreshStatus(of: device)
                asking = false
            }
        }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("Revoke Trust", systemImage: "xmark.shield", role: .destructive) {
                        model.askToRevoke(device)
                    }
                    .accessibilityIdentifier("trusted.revoke")
                    Button("Forget Device", systemImage: "trash", role: .destructive) {
                        model.askToForget(device)
                    }
                    .accessibilityIdentifier("trusted.forget")
                } label: {
                    Label("More", systemImage: "ellipsis.circle")
                }
                .accessibilityIdentifier("trusted.more")
            }
        }
        .fileImporter(isPresented: $pickingFiles, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
            switch result {
            case .success(let urls): sendFiles(urls, to: device)
            case .failure(let error): model.report(error)
            }
        }
    }

    /// Picked files are copied in first (`ForgePickedFile`), keeping their
    /// names, and the copies are deleted once the send ends.
    private func sendFiles(_ picked: [URL], to device: Device) {
        guard !picked.isEmpty else { return }
        sending = true
        Task {
            var copies: [ForgePickedFile] = []
            defer {
                for copy in copies { copy.release() }
                sending = false
            }
            do {
                for url in picked {
                    copies.append(try await ForgePickedFile.copy(of: url))
                }
            } catch {
                model.report(error)
                return
            }
            await model.send(paths: copies.map(\.url.path), toDevice: device)
        }
    }
}
#endif
