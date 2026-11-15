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
    }

    public var alert: ForgeAlert<Action>?
    /// Increases after each saved change; views use it as a haptic trigger.
    public private(set) var saveCount = 0

    public let core: RelayCore

    public init(core: RelayCore) {
        self.core = core
        identity = core.identity()
        reload()
    }

    /// Re-reads Rust's published state. Cheap: no command is waited for.
    public func reload() {
        identity = core.identity()
        peers = core.peers()
        transfers = core.transfers()
        port = core.listeningPort()
    }

    /// Whether anything is still on its way, so the screen refreshes its
    /// progress.
    public var isTransferring: Bool {
        transfers.contains { if case .inProgress = $0.status { true } else { false } }
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
        let cancellation = Cancellation()
        let peer = try await withTaskCancellationHandler {
            try await core.pair(code: code.trimmingCharacters(in: .whitespacesAndNewlines), cancellation: cancellation)
        } onCancel: {
            cancellation.cancel()
        }
        reload()
        saveCount += 1
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
        }
    }

    public func remove(_ transfer: Transfer) async {
        await perform { try await $0.removeTransfer(id: transfer.id) }
    }

    /// Reports a failure the views ran into themselves.
    public func report(_ error: Error) {
        log.warn("failed: \(error)")
        alert = .failure(error)
    }

    private func sending(_ send: (RelayCore, Cancellation) async throws -> Transfer) async -> Bool {
        let cancellation = Cancellation()
        // The transfer appears in the history at once; show it.
        reload()
        do {
            _ = try await withTaskCancellationHandler {
                try await send(core, cancellation)
            } onCancel: {
                cancellation.cancel()
            }
            reload()
            saveCount += 1
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
            saveCount += 1
        } catch {
            report(error)
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
                codeReceive = .received(receipt)
                saveCount += 1
            } catch {
                log.warn("code receive failed: \(error)")
                codeReceive = .failed(Self.message(error))
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
        case .Refused: return "The other device didn't accept. Pair again with a new code."
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
