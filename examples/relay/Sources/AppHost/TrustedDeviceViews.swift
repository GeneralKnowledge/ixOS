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
            .foregroundStyle(.green)
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
                Text("Revoked: can't send here without a code")
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
/// Devices tab and in Settings.
struct ConnectionModeSection: View {
    let mode: ConnectionMode
    let choose: (ConnectionMode) -> Void

    init(model: RelayModel) {
        mode = model.connectionMode
        choose = { mode in Task { await model.setConnectionMode(mode) } }
    }

    init(mode: ConnectionMode, choose: @escaping (ConnectionMode) -> Void) {
        self.mode = mode
        self.choose = choose
    }

    var body: some View {
        Section {
            Picker("Reach Trusted Devices", selection: Binding(get: { mode }, set: choose)) {
                Text("Any Route").tag(ConnectionMode.hybrid)
                Text("Local and Private Networks").tag(ConnectionMode.private)
                Text("Local Network Only").tag(ConnectionMode.local)
            }
            .accessibilityIdentifier("devices.connectionMode")
        } header: {
            Text("Connections")
        } footer: {
            Text(RelayModel.explain(mode))
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
        ConnectionModeSection(mode: mode) { chosen in
            mode = chosen
            Task {
                do {
                    try await core.setConnectionMode(mode: chosen)
                } catch {
                    mode = core.connectionMode()
                }
            }
        }
        .onAppear { mode = core.connectionMode() }
        RelayServersSection(core: core)
    }
}

/// Whether Relay uses its own relay and lookup server, so trusted devices
/// find each other away from any shared network. Off until turned on.
struct RelayServersSection: View {
    let core: RelayCore
    @State private var on = false

    var body: some View {
        Section {
            Toggle("Use Relay's Servers", isOn: Binding(get: { on }, set: { chosen in
                on = chosen
                Task {
                    do {
                        try await core.setServers(servers: chosen ? ownRelayServers() : RelayServers(relayUrl: nil, lookupUrl: nil))
                    } catch {
                        on = core.servers() == ownRelayServers()
                    }
                }
            }))
            .accessibilityIdentifier("settings.relayServers")
        } footer: {
            Text(on
                 ? "Trusted devices can find each other from anywhere, through relay.avedeus.ovh. What they send stays encrypted end to end. Takes effect the next time Relay opens."
                 : "Relay reaches trusted devices on local and private networks, such as Tailscale, only.")
        }
        .onAppear { on = core.servers() == ownRelayServers() }
    }
}

/// One trusted device: send it a note or files without a code, choose
/// whether it may send here, see what went between you, revoke or forget.
struct TrustedDeviceView: View {
    @Bindable var model: RelayModel
    let endpointID: String
    @Binding var preview: URL?

    @State private var note = ""
    @State private var sending = false
    @State private var pickingFiles = false
    @State private var asking = false
    @FocusState private var writing: Bool

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
                TextField("Write a note", text: $note, axis: .vertical)
                    .lineLimit(3...8)
                    .focused($writing)
                    .accessibilityIdentifier("trusted.note")
                HStack {
                    Button {
                        sendNote(to: device)
                    } label: {
                        Label("Send Note", systemImage: "paperplane")
                    }
                    .disabled(sending || note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .accessibilityIdentifier("trusted.sendNote")
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
                     ? "\(device.name) can send notes and files here without a code."
                     : "\(device.name) is still trusted, but what it sends here is declined.")
            }
            let history = model.transfers(with: device)
            if !history.isEmpty {
                Section("Between You") {
                    ForEach(history) { transfer in
                        TransferRow(transfer: transfer, model: model, preview: $preview)
                    }
                }
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

    private func sendNote(to device: Device) {
        let text = note
        sending = true
        Task {
            if await model.send(note: text, toDevice: device) {
                note = ""
                writing = false
            }
            sending = false
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
