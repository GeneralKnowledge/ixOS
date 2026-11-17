@preconcurrency import RelayFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import Foundation
import Observation

private let log = ForgeLog("Relay")

/// What the screens show and the actions they take. It holds no rules:
/// pairing, encryption, sending and the history are Relay's Rust core
/// (`rust/src/lib.rs`); this model asks it and shows what it says. Finding
/// devices (Bonjour) is `RelayNetwork`, which hands this model addresses.
/// Foundation and Observation only, so it compiles and tests on Linux.
@Observable
@MainActor
public final class RelayModel {
    public private(set) var identity: Identity
    /// Paired devices, by name.
    public private(set) var peers: [Peer] = []
    /// Everything sent and received, newest first.
    public private(set) var transfers: [Transfer] = []
    /// The port Relay listens on, while it does.
    public private(set) var port: UInt16?
    /// The open pairing code, while one is shown.
    public private(set) var invitation: Invitation?

    /// Alerts the screens ask for: confirm, or a failure.
    public enum Action: Equatable, Sendable {
        case forget(deviceID: String)
        case remove(transfer: UInt64)
        case revoke(endpointID: String)
        case forgetDevice(endpointID: String)
    }

    public var alert: ForgeAlert<Action>?
    /// Haptics: success when something is done (paired, sent, or a file
    /// or note finished arriving), failure with the alert or message that
    /// says why.
    public private(set) var feedback = ForgeFeedback()

    public let core: RelayCore

    public init(core: RelayCore) {
        self.core = core
        identity = core.identity()
        reload(announcing: false)
    }

    /// Re-reads Rust's published state. Cheap: no command is waited for.
    public func reload() {
        reload(announcing: true)
    }

    /// `announcing`: whether a transfer that finished arriving since the
    /// last read plays its haptic. False when the action that received it
    /// plays its own (a code receive), so it isn't felt twice.
    private func reload(announcing: Bool) {
        identity = core.identity()
        peers = core.peers()
        let previous = transfers
        transfers = core.transfers()
        if announcing, !isReceivingCode, let kind = Self.arrival(from: previous, to: transfers) {
            feedback.play(kind)
        }
        port = core.listeningPort()
        devices = core.devices()
        policy = core.pairingPolicy()
        trustOffer = core.trustOffer()
    }

    /// Whether anything is still on its way, so the screen refreshes its
    /// progress.
    public var isTransferring: Bool {
        transfers.contains { if case .inProgress = $0.status { true } else { false } }
    }

    /// The haptic for what arrived between two reads of the history: success
    /// when a received file or note finished (one tap however many did),
    /// failure when one stopped part way. Sends are left to the action that
    /// started them. The first read (`init`) announces nothing.
    nonisolated static func arrival(from previous: [Transfer], to current: [Transfer]) -> ForgeFeedback.Kind? {
        let before = Dictionary(previous.map { ($0.id, $0.status) }, uniquingKeysWith: { a, _ in a })
        var kind: ForgeFeedback.Kind?
        for transfer in current where transfer.direction == .received {
            // New since the last read, or still on its way then.
            let wasRunning: Bool
            switch before[transfer.id] {
            case .none, .some(.inProgress): wasRunning = true
            case .some(.done), .some(.failed): wasRunning = false
            }
            guard wasRunning else { continue }
            switch transfer.status {
            case .done: kind = kind ?? .success
            case .failed: kind = .failure
            case .inProgress: break
            }
        }
        return kind
    }

    public func transfers(with peer: Peer) -> [Transfer] {
        transfers.filter { $0.peerId == peer.deviceId }
    }

    // MARK: Listening and pairing

    /// Opens Relay's port for paired devices; returns it, or `nil` (logged:
    /// the screen shows "not reachable" rather than an alert).
    @discardableResult
    public func startListening() -> UInt16? {
        if let port = core.listeningPort() { return port }
        do {
            port = try core.startListening(port: 0)
        } catch {
            log.warn("listening failed: \(error)")
            port = nil
        }
        return port
    }

    public func stopListening() {
        core.stopListening()
        port = nil
    }

    /// A pairing code for this device at `hosts` (its local addresses).
    @discardableResult
    public func makeInvitation(hosts: [String]) -> Invitation? {
        startListening()
        do {
            invitation = try core.createInvitation(hosts: hosts)
        } catch {
            report(error)
            invitation = nil
        }
        return invitation
    }

    public func cancelInvitation() {
        core.cancelInvitation()
        invitation = nil
    }

    /// Pairs with the device whose code was scanned or pasted. Throws, so
    /// the scanner can keep going and say why.
    public func pair(code: String) async throws -> Peer {
        let peer = try await withRustCancellation { cancellation in
            try await core.pair(code: code.trimmingCharacters(in: .whitespacesAndNewlines), cancellation: cancellation)
        }
        reload()
        feedback.play(.success)
        return peer
    }

    /// Called when this device's code was used: the other device paired.
    public func invitationUsed() {
        invitation = nil
        reload()
    }

    public func askToForget(_ peer: Peer) {
        alert = .confirm(
            "Forget “\(peer.name)”?",
            message: "It can't send to this device, or receive from it, until you pair again. Its history stays.",
            confirm: "Forget Device",
            action: .forget(deviceID: peer.deviceId),
            identifier: "peers.confirmForget",
            destructive: true
        )
    }

    public func rename(to name: String) async throws {
        try await core.renameDevice(name: name)
        reload()
    }

    // MARK: Sending

    /// Sends a note to `peer` at `addresses` (from Bonjour; empty means
    /// where it was last reached). `false` (and an alert) if it failed.
    @discardableResult
    public func send(note: String, to peer: Peer, addresses: [String]) async -> Bool {
        await sending { core, cancellation in
            try await core.sendNote(deviceId: peer.deviceId, text: note, addresses: addresses, cancellation: cancellation)
        }
    }

    /// Sends the file at `url`, which must stay until the transfer ends.
    @discardableResult
    public func send(file url: URL, to peer: Peer, addresses: [String]) async -> Bool {
        await sending { core, cancellation in
            try await core.sendFile(deviceId: peer.deviceId, path: url.path, addresses: addresses, cancellation: cancellation)
        }
    }

    /// Runs a confirmed action.
    public func handle(_ action: Action) async {
        switch action {
        case .forget(let id):
            await perform { try await $0.forgetPeer(deviceId: id) }
        case .remove(let id):
            await perform { try await $0.removeTransfer(id: id) }
        case .revoke(let id):
            await perform { try await $0.revokeDevice(endpointId: id) }
        case .forgetDevice(let id):
            await perform { try await $0.forgetDevice(endpointId: id) }
        }
    }

    public func remove(_ transfer: Transfer) async {
        await perform { try await $0.removeTransfer(id: transfer.id) }
    }

    /// Reports a failure the views ran into themselves.
    public func report(_ error: Error) {
        log.warn("failed: \(error)")
        alert = .failure(error)
        feedback.play(.failure)
    }

    private func sending(_ send: (RelayCore, Cancellation) async throws -> Transfer) async -> Bool {
        // The transfer appears in the history at once; show it.
        reload()
        do {
            _ = try await withRustCancellation { cancellation in
                try await send(core, cancellation)
            }
            reload()
            feedback.play(.success)
            return true
        } catch {
            reload()
            if case RelayError.Stopped = error { return false }
            report(error)
            return false
        }
    }

    private func perform(_ command: (RelayCore) async throws -> Void) async {
        do {
            try await command(core)
            reload()
            feedback.play(.success)
        } catch {
            report(error)
        }
    }

    // MARK: Trusted devices (brief PR 3)

    /// Trusted and revoked devices, by name. Trust is keyed by the
    /// device's endpoint id in Rust (`rust/src/trust.rs`), never its name.
    public private(set) var devices: [Device] = []
    /// What happens after a code transfer (Settings).
    public private(set) var policy: PairingPolicy = .askToPair
    /// "Trust this device?" after a code transfer, until answered.
    public private(set) var trustOffer: TrustOffer?

    public var trustedDevices: [Device] { devices.filter(\.trusted) }
    public var revokedDevices: [Device] { devices.filter { !$0.trusted } }

    public func device(_ endpointID: String) -> Device? {
        devices.first { $0.endpointId == endpointID }
    }

    public func transfers(with device: Device) -> [Transfer] {
        transfers.filter { $0.peerId == device.endpointId }
    }

    /// Where this device takes trusted connections, for Bonjour; `nil` if
    /// Relay's endpoint couldn't start.
    public func deviceService() async -> DeviceService? {
        do {
            return try await core.deviceService()
        } catch {
            log.warn("no device service: \(error)")
            return nil
        }
    }

    /// The trusted devices Bonjour sees now, with where they are.
    public func devicesNearby(_ nearby: [NearbyDevice]) {
        core.devicesNearby(devices: nearby)
        devices = core.devices()
    }

    /// Re-reads the trust offer: the sending side learns the other's name
    /// a moment after its code send ends.
    public func refreshTrustOffer() {
        trustOffer = core.trustOffer()
    }

    /// The person answered Trust.
    public func trust(_ offer: TrustOffer) async {
        trustOffer = nil
        await perform { try await $0.trustDevice(endpointId: offer.endpointId) }
    }

    /// The person answered Not Now: nothing is remembered.
    public func dismissTrustOffer() {
        core.dismissTrustOffer()
        trustOffer = nil
    }

    /// Trust Again, for a revoked device: the only way it is trusted again.
    public func trustAgain(_ device: Device) async {
        await perform { try await $0.trustDevice(endpointId: device.endpointId) }
    }

    public func askToRevoke(_ device: Device) {
        alert = .confirm(
            "Revoke “\(device.name)”?",
            message: "It can't send to this device without a code, and a code transfer never trusts it again. Only you can, from this list.",
            confirm: "Revoke",
            action: .revoke(endpointID: device.endpointId),
            identifier: "trusted.confirmRevoke",
            destructive: true
        )
    }

    public func askToForget(_ device: Device) {
        alert = .confirm(
            "Forget “\(device.name)”?",
            message: device.trusted
                ? "It leaves this list and can't send without a code. Its history stays."
                : "It leaves this list and stays revoked. Its history stays.",
            confirm: "Forget Device",
            action: .forgetDevice(endpointID: device.endpointId),
            identifier: "trusted.confirmForget",
            destructive: true
        )
    }

    /// Whether `device` may send files and notes here.
    public func setReceives(_ on: Bool, from device: Device) async {
        await perform { try await $0.setReceivesFrom(endpointId: device.endpointId, on: on) }
    }

    public func setPolicy(_ policy: PairingPolicy) async {
        await perform { try await $0.setPairingPolicy(policy: policy) }
    }

    /// Sends a note to a trusted device, no code. `false` (and an alert)
    /// if it failed.
    @discardableResult
    public func send(note: String, toDevice device: Device) async -> Bool {
        await sendingToDevice { try await $0.sendNoteToDevice(endpointId: device.endpointId, text: note) }
    }

    /// Sends files and folders (in one folder when several) to a trusted
    /// device. They must stay until the send ends.
    @discardableResult
    public func send(paths: [String], toDevice device: Device) async -> Bool {
        await sendingToDevice { try await $0.sendToDevice(endpointId: device.endpointId, paths: paths) }
    }

    /// Stops a send to a trusted device that is on its way.
    public func cancel(_ transfer: Transfer) {
        core.cancelTransfer(id: transfer.id)
    }

    private func sendingToDevice(_ send: (RelayCore) async throws -> Transfer) async -> Bool {
        reload()
        do {
            _ = try await send(core)
            reload()
            feedback.play(.success)
            return true
        } catch {
            reload()
            if case RelayError.Stopped = error { return false }
            report(error)
            return false
        }
    }

    // MARK: Receiving with a word code (Relay M1)

    /// Where a code receive is: the screen shows this.
    public enum CodeReceive: Equatable, Sendable {
        case idle
        /// Looking for the sender on this Wi-Fi.
        case finding
        /// Connected; `size` once the sender has named the file.
        case receiving(size: CodeSize?, since: Date)
        case received(CodeReceipt)
        case failed(String)
    }

    /// What Bonjour found for a code's number (`RelayNetwork` looks).
    public enum CodeSender: Equatable, Sendable {
        case found(addresses: [String], endpointID: String)
        case none
        /// More than one sender uses the number: guessing would spend a
        /// stranger's code, so stop.
        case several
    }

    public private(set) var codeReceive: CodeReceive = .idle

    public var isReceivingCode: Bool {
        switch codeReceive {
        case .finding, .receiving: true
        default: false
        }
    }

    /// Finds the sender of `code` with `find` (Bonjour, by the code's
    /// number), then receives its file.
    public func receive(code: String, find: (UInt8) async -> CodeSender) async {
        let number: UInt8
        do {
            number = try wordCodeNumber(code: code)
        } catch {
            codeReceive = .failed(Self.message(error))
            return
        }
        codeReceive = .finding
        switch await find(number) {
        case .none:
            codeReceive = .failed("No sender with code number \(number) on this Wi-Fi. Check the sender is still waiting, on the same network.")
        case .several:
            codeReceive = .failed("Two senders nearby use number \(number). Ask the sender for a new code.")
        case .found(let addresses, let endpointID):
            codeReceive = .receiving(size: nil, since: Date())
            do {
                let receipt = try await core.receiveWithCode(code: code, addresses: addresses, endpointId: endpointID)
                reload(announcing: false)
                codeReceive = .received(receipt)
                feedback.play(.success)
                refreshTrustOffer()
            } catch {
                log.warn("code receive failed: \(error)")
                codeReceive = .failed(Self.message(error))
                feedback.play(.failure)
            }
        }
    }

    /// Re-reads the size of the file on its way.
    public func refreshCodeReceive() {
        if case .receiving(_, let since) = codeReceive {
            codeReceive = .receiving(size: core.codeReceiveSize(), since: since)
        }
    }

    public func cancelCodeReceive() {
        core.cancelCodeReceive()
    }

    public func resetCodeReceive() {
        if !isReceivingCode { codeReceive = .idle }
    }

    // MARK: Sending with a word code (Relay M1)

    /// Where a send with a code is; Rust's `CodeSend`, re-read while it runs.
    public private(set) var codeSend: CodeSend = .idle

    public var isSendingCode: Bool {
        Self.isRunning(codeSend)
    }

    /// What to advertise with Bonjour while there is a code: its number,
    /// Rust's port and endpoint id.
    public struct CodeAdvert: Equatable, Sendable {
        public let number: UInt8
        public let port: UInt16
        public let endpointID: String
    }

    public var codeAdvert: CodeAdvert? {
        switch codeSend {
        case .waiting(_, let number, let port, let endpointID, _, _, _),
             .sending(_, let number, let port, let endpointID, _, _):
            CodeAdvert(number: number, port: port, endpointID: endpointID)
        default:
            nil
        }
    }

    /// Starts sending the file or folder at `path` with a new code.
    public func send(fileAt path: String) {
        send(paths: [path])
    }

    /// Starts sending the files and folders at `paths` with one new code;
    /// the receiver gets them in one folder when there are several.
    public func send(paths: [String]) {
        do {
            try core.sendPathsWithCode(paths: paths)
            codeSend = core.codeSendStatus()
        } catch {
            log.warn("code send failed to start: \(error)")
            codeSend = .failed(reason: Self.message(error))
        }
    }

    /// Shows that a picked file is being made ready (copied in) before Rust
    /// has it.
    public func preparing(name: String) {
        codeSend = .preparing(name: name, size: 0)
    }

    public func failSend(_ reason: String) {
        codeSend = .failed(reason: reason)
    }

    public func refreshCodeSend() {
        let now = core.codeSendStatus()
        // Still copying the picked file in: Rust hasn't been asked yet.
        if case .idle = now, case .preparing = codeSend { return }
        codeSend = now
        refreshTrustOffer()
    }

    /// Other code senders Bonjour sees, so Rust avoids their numbers.
    public func codeSendersNearby(_ senders: [NearbySender]) {
        core.codeSendNearby(senders: senders)
    }

    public func cancelCodeSend() {
        core.cancelCodeSend()
    }

    /// When the sheet opens: picks up a send still running in Rust (its code
    /// is still good), otherwise starts clean.
    public func resetCodeSend() {
        let now = core.codeSendStatus()
        if Self.isRunning(now) {
            codeSend = now
        } else if !isSendingCode {
            codeSend = .idle
        }
    }

    nonisolated private static func isRunning(_ send: CodeSend) -> Bool {
        switch send {
        case .preparing, .waiting, .sending: true
        default: false
        }
    }

    nonisolated private static func message(_ error: Error) -> String {
        if let relay = error as? RelayError {
            switch relay {
            case .Declined(let reason), .Unreachable(let reason), .Connection(let reason), .File(let reason):
                return reason.prefix(1).uppercased() + reason.dropFirst() + "."
            default:
                return describe(relay)
            }
        }
        return "\(error)"
    }

    // MARK: Words for the screen

    nonisolated public static func describe(_ error: RelayError) -> String {
        switch error {
        case .EmptyName: return "Give this device a name."
        case .NameTooLong(let max): return "Names can be up to \(max) characters."
        case .EmptyNote: return "Write something to send."
        case .NoteTooLong(let max): return "Notes can be up to \(max) characters."
        case .FileTooLarge: return "Files can be up to 2 GB."
        case .BadCode: return "That isn't a Relay pairing code."
        case .CodeExpired: return "That pairing code has expired. Make a new one on the other device."
        case .WordCode(let reason): return reason.prefix(1).uppercased() + reason.dropFirst() + "."
        case .UnknownPeer: return "This device isn't paired with that one any more."
        case .NotFound: return "That's no longer in the history."
        case .Unreachable: return "Can't reach the other device. Check that it has Relay open, on the same Wi-Fi."
        case .Refused: return "The other device didn’t accept this one. Pair again, or send with a code."
        case .Declined(let reason): return "The other device declined: \(reason)."
        case .Connection: return "The connection broke. Try again."
        case .File: return "The file couldn't be read or kept."
        case .AlreadyListening: return "Relay is already open for other devices."
        case .InUse: return "Relay is open in another copy of the app."
        case .Corrupt: return "Relay's saved data couldn't be read."
        case .NewerData: return "Relay's data was saved by a newer version of the app."
        case .Storage: return "Couldn't save. Try again."
        case .Stopped: return "Stopped."
        }
    }

    nonisolated public static func title(_ error: RelayError) -> String {
        switch error {
        case .EmptyName, .NameTooLong, .EmptyNote, .NoteTooLong, .FileTooLarge: return "Check the Details"
        case .BadCode, .CodeExpired, .Refused: return "Couldn't Pair"
        case .WordCode: return "Check the Code"
        case .UnknownPeer, .NotFound: return "Not Found"
        case .Unreachable, .Connection, .Declined: return "Not Sent"
        case .File: return "File Problem"
        case .AlreadyListening, .InUse: return "Already Open"
        case .Corrupt, .NewerData, .Storage: return "Couldn't Save"
        case .Stopped: return "Stopped"
        }
    }
}

extension RelayError: ForgeDescribedError {
    public var failure: ForgeFailure {
        let retryable: Bool
        switch self {
        case .Unreachable, .Connection, .Storage: retryable = true
        default: retryable = false
        }
        return ForgeFailure(
            title: RelayModel.title(self),
            message: RelayModel.describe(self),
            isRetryable: retryable
        )
    }

    public var isCancellation: Bool {
        if case .Stopped = self { return true }
        return false
    }
}

extension Peer: Identifiable {
    public var id: String { deviceId }
}

extension Transfer: Identifiable {}

extension Device: Identifiable {
    public var id: String { endpointId }
}

// MARK: Formatting (Foundation only, so it tests on Linux)

public enum RelayFormat {
    public static func date(_ ms: Int64) -> Date {
        Date(timeIntervalSince1970: TimeInterval(ms) / 1000)
    }

    /// "1.2 MB".
    public static func size(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .file)
    }

    /// What a history row says: the note, or the file's name.
    public static func title(_ item: Item) -> String {
        switch item {
        case .note(let text): return text
        case .file(let name, _, _): return name
        }
    }

    /// "Sending 40%", "Sent", "Received", "Failed: …".
    public static func status(_ transfer: Transfer) -> String {
        switch transfer.status {
        case .inProgress(let done):
            let verb = transfer.direction == .sent ? "Sending" : "Receiving"
            if case .file(_, let size, _) = transfer.item, size > 0 {
                return "\(verb) \(Int(Double(done) * 100 / Double(size)))%"
            }
            return verb + "…"
        case .done:
            return transfer.direction == .sent ? "Sent" : "Received"
        case .failed(let reason):
            return "Failed: \(reason)"
        }
    }

    /// The service name a device advertises: its id, so a browser can tell
    /// which paired device it found.
    public static func serviceName(for identity: Identity) -> String {
        identity.deviceId
    }

    /// Where to reach a device found by Bonjour, as Rust takes it.
    public static func address(host: String, port: UInt16) -> String {
        // An IPv6 host may carry a scope ("fe80::1%en0"); Rust wants brackets.
        host.contains(":") ? "[\(host)]:\(port)" : "\(host):\(port)"
    }
}
