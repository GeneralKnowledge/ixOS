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
                Text(device.person.map { "\($0) (\(device.name))" } ?? device.name)
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
            ForgeButton("Trust Again") {
                await model.trustAgain(device)
            }
            .buttonStyle(.borderless)
            .accessibilityIdentifier("revoked.trustAgain")
        }
        .forgeRowActions {
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
    var health: ServerHealth = .off
    /// False where the screen shows the servers-down notice elsewhere
    /// (Devices, at the top), so it isn't shown twice.
    var showsDown = true
    /// Whether the servers answer now, for the Servers screen, which reads
    /// it again while it is open.
    let readHealth: () -> ServerHealth
    let choose: (ConnectionMode) -> Void
    let chooseServers: (RelayServers) -> Void

    init(model: RelayModel, showsHealth: Bool = true) {
        mode = model.connectionMode
        servers = model.servers
        health = model.serverHealth
        showsDown = showsHealth
        readHealth = { [core = model.core] in core.serverHealth() }
        choose = { mode in Task { await model.setConnectionMode(mode) } }
        chooseServers = { servers in Task { await model.setServers(servers) } }
    }

    init(
        mode: ConnectionMode,
        servers: RelayServers,
        health: ServerHealth,
        readHealth: @escaping () -> ServerHealth,
        choose: @escaping (ConnectionMode) -> Void,
        chooseServers: @escaping (RelayServers) -> Void
    ) {
        self.mode = mode
        self.servers = servers
        self.health = health
        self.readHealth = readHealth
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
                    RelayServersView(servers: servers, readHealth: readHealth, choose: chooseServers)
                } label: {
                    LabeledContent("Servers", value: Self.serversName(servers))
                }
                .accessibilityIdentifier("devices.servers")
                ServerHealthRow(health: health, showsDown: showsDown)
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

/// While Relay checks the servers (for a few seconds after a change, or
/// while it opens), says so, so the screen doesn't look idle; then the
/// servers-down notice, or nothing once they answer.
struct ServerHealthRow: View {
    let health: ServerHealth
    var showsDown = true

    var body: some View {
        switch health {
        case .connecting:
            ServersCheckingRow()
        case .unreachable where showsDown:
            ServersDownNotice()
        default:
            EmptyView()
        }
    }
}

/// "Checking the servers…" with a spinner.
struct ServersCheckingRow: View {
    var body: some View {
        HStack(spacing: 8) {
            ProgressView()
            Text("Checking the servers…")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("devices.serversChecking")
    }
}

/// Relay's servers can't be reached (N2): on Devices (at the top) and in
/// Settings, under Connections.
struct ServersDownNotice: View {
    var body: some View {
        ForgeNotice(
            "The servers can't be reached right now. Devices on the same Wi-Fi or Tailscale still can.",
            systemImage: "exclamationmark.triangle"
        )
        .listRowInsets(EdgeInsets())
        .listRowBackground(Color.clear)
        .accessibilityIdentifier("devices.serversDown")
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
    @State private var health: ServerHealth = .off

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
        ConnectionModeSection(
            mode: mode, servers: servers, health: health, readHealth: { core.serverHealth() }
        ) { chosen in
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
        // The relay can still be connecting after a change, or go down
        // and come back: read it again while Settings is on screen.
        .task {
            while !Task.isCancelled {
                health = core.serverHealth()
                try? await Task.sleep(for: .seconds(3))
            }
        }
    }
}

/// Advanced, under Everywhere: Relay's servers (the default), your own
/// relay and lookup server, or none. Applies at once (N2).
struct RelayServersView: View {
    let readHealth: () -> ServerHealth
    let choose: (RelayServers) -> Void
    @State private var choice: RelayModel.ServerChoice
    @State private var relayURL: String
    @State private var lookupURL: String
    @State private var health: ServerHealth = .off

    init(
        servers: RelayServers,
        readHealth: @escaping () -> ServerHealth,
        choose: @escaping (RelayServers) -> Void
    ) {
        self.readHealth = readHealth
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
            // The servers just chosen are checked here, where they were
            // chosen: checking, then down or nothing, without leaving.
            if health == .connecting || health == .unreachable {
                Section {
                    ServerHealthRow(health: health)
                }
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
        .task {
            while !Task.isCancelled {
                health = readHealth()
                try? await Task.sleep(for: .seconds(1))
            }
        }
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
    /// "This Is a Person" turned on, before a name is saved.
    @State private var namingPerson = false
    @State private var personName = ""
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
                        // Copying the picked files in can take a moment.
                        Label {
                            Text(sending ? "Sending…" : "Send Files")
                        } icon: {
                            if sending {
                                ProgressView()
                            } else {
                                Image(systemName: "doc.badge.plus")
                            }
                        }
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
                    Label {
                        Text(asking ? "Asking…" : "Ask Again")
                    } icon: {
                        if asking {
                            ProgressView()
                        } else {
                            Image(systemName: "arrow.clockwise")
                        }
                    }
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
                Toggle("Can Open My Site", isOn: Binding(
                    get: { device.grants.contains(.sites) },
                    set: { on in Task { await model.setGrant(.sites, on, for: device) } }
                ))
                .accessibilityIdentifier("trusted.opensSite")
                if RelayModel.sharesSite(device) {
                    NavigationLink {
                        RelaySiteView(device: device, model: model)
                    } label: {
                        Label("Open Its Site", systemImage: "globe")
                    }
                    .accessibilityIdentifier("trusted.openSite")
                }
            } header: {
                Text("Sites")
            } footer: {
                Text(RelayModel.sharesSite(device)
                     ? "\(device.name) shares a site with this device. Only devices it trusts and lets in can open it, never the internet."
                     : "When on, \(device.name) can open a site this device shares. Trust alone never lets it in.")
            }
            Section {
                Toggle("This Is a Person", isOn: Binding(
                    get: { device.person != nil || namingPerson },
                    set: { on in
                        if on {
                            personName = device.person ?? ""
                            namingPerson = true
                        } else {
                            namingPerson = false
                            Task { await model.setPerson(nil, for: device) }
                        }
                    }
                ))
                .accessibilityIdentifier("trusted.isPerson")
                if device.person != nil || namingPerson {
                    TextField("Their Name", text: $personName)
                        .textContentType(.name)
                        .submitLabel(.done)
                        .onAppear {
                            if personName.isEmpty { personName = device.person ?? "" }
                        }
                        .onSubmit {
                            let name = personName.trimmingCharacters(in: .whitespacesAndNewlines)
                            guard !name.isEmpty else { return }
                            Task {
                                await model.setPerson(name, for: device)
                                namingPerson = false
                            }
                        }
                        .accessibilityIdentifier("trusted.personName")
                }
            } header: {
                Text("Whose Device")
            } footer: {
                Text(device.person.map { "\(device.name) belongs to \($0). It can send messages and files, but never introduces devices or sees yours. Chats shows their name." }
                     ?? (namingPerson
                         ? "Type their name and press Done."
                         : "One of your own devices. Turn on if it belongs to someone you trust."))
            }
            if device.person == nil {
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
        }
        .navigationTitle(device.name)
        .navigationBarTitleDisplayMode(.inline)
        // Ask each time the screen opens, offline or not: what it offers
        // (a site) may have changed since, and "offline" may only mean
        // nothing has connected lately. Ask Again does it later.
        .task(id: device.endpointId) {
            asking = true
            await model.refreshStatus(of: device)
            asking = false
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
