@preconcurrency import RelayFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import Foundation
import Observation

private let log = ForgeLog("Relay")

/// What the screens show and the actions they take. It holds no rules:
/// trust, encryption, sending and the history are Relay's Rust core
/// (`rust/src/lib.rs`); this model asks it and shows what it says. Finding
/// devices (Bonjour) is `RelayNetwork`, which hands this model addresses.
/// Foundation and Observation only, so it compiles and tests on Linux.
@Observable
@MainActor
public final class RelayModel {
    public private(set) var identity: Identity
    /// Everything sent and received, newest first.
    public private(set) var transfers: [Transfer] = []

    /// Alerts the screens ask for: confirm, or a failure.
    public enum Action: Equatable, Sendable {
        case remove(transfer: UInt64)
        case revoke(endpointID: String)
        case forgetDevice(endpointID: String)
    }

    public var alert: ForgeAlert<Action>?
    /// Haptics: success when something is done (trusted, sent, or a file
    /// or note finished arriving), failure with the alert or message that
    /// says why.
    public private(set) var feedback = ForgeFeedback()

    public let core: RelayCore

    public init(core: RelayCore) {
        self.core = core
        identity = core.identity()
        reload(announcing: false)
    }

    // MARK: The background

    /// Whether the app is in the background, where iOS ends Relay's
    /// adverts and browsing. The devices screen closes them when this turns
    /// true and opens them again when it turns false.
    public private(set) var isAway = false
    /// Counts returns from the background. An open send code is advertised
    /// again on each, because iOS dropped the advert while the app was away
    /// (for example while the person switched app to pass the code on).
    public private(set) var returns = 0

    /// Follows the app's phases through `host`, for as long as this model
    /// lives.
    public func follow(_ host: ForgeHost<RelayCore>) {
        host.onBackground(self) { [weak self] in self?.wentAway() }
        host.onResume(self) { [weak self] _ in self?.cameBack() }
    }

    func wentAway() {
        isAway = true
    }

    func cameBack() {
        isAway = false
        returns &+= 1
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
        let previous = transfers
        transfers = core.transfers()
        if announcing, !isReceivingCode, let kind = Self.arrival(from: previous, to: transfers) {
            feedback.play(kind)
        }
        devices = core.devices()
        policy = core.pairingPolicy()
        connectionMode = core.connectionMode()
        trustOffer = core.trustOffer()
        savedDataNotice = core.savedDataNotice()
    }

    /// Saved data Relay couldn't read when it opened, and where it is
    /// kept: shown until the person dismisses it.
    public private(set) var savedDataNotice: SavedDataNotice?

    /// The person has read the notice.
    public func dismissSavedDataNotice() async {
        savedDataNotice = nil
        await perform { try await $0.dismissSavedDataNotice() }
    }

    /// Whether anything is still on its way, so the screen refreshes its
    /// progress.
    public var isTransferring: Bool {
        transfers.contains { if case .inProgress = $0.status { true } else { false } }
    }

    /// Whether anything waits in the outbox for a trusted device, so the
    /// screen notices when it goes.
    public var isWaiting: Bool {
        transfers.contains { $0.status == .waiting }
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
            case .none, .some(.inProgress), .some(.waiting): wasRunning = true
            case .some(.done), .some(.failed): wasRunning = false
            }
            guard wasRunning else { continue }
            switch transfer.status {
            case .done: kind = kind ?? .success
            case .failed: kind = .failure
            case .inProgress, .waiting: break
            }
        }
        return kind
    }

    public func rename(to name: String) async throws {
        try await core.renameDevice(name: name)
        reload()
    }

    /// Runs a confirmed action.
    public func handle(_ action: Action) async {
        switch action {
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
    /// Which routes reach trusted devices (Settings).
    public private(set) var connectionMode: ConnectionMode = .hybrid
    /// "Trust this device?" after a code transfer, until answered.
    public private(set) var trustOffer: TrustOffer?

    public var trustedDevices: [Device] { devices.filter(\.trusted) }
    public var revokedDevices: [Device] { devices.filter { !$0.trusted } }

    public func device(_ endpointID: String) -> Device? {
        devices.first { $0.endpointId == endpointID }
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

    /// Asks a trusted device what it says about itself (its platform and
    /// what it offers) and re-reads the list; failing to reach it is not
    /// an error the person needs to see, it just stays as it was.
    public func refreshStatus(of device: Device) async {
        do {
            _ = try await core.deviceStatus(endpointId: device.endpointId)
        } catch {
            log.info("no status from a trusted device: \(error)")
        }
        devices = core.devices()
    }

    /// Re-reads the devices, so presence ages ("reachable" becomes
    /// "offline" a few minutes after the last connection).
    public func refreshDevices() {
        devices = core.devices()
    }

    /// "Nearby", "Reachable, seen 2 min ago" or "Offline, seen 3 h ago".
    nonisolated public static func whereNow(_ device: Device, now: Date = Date()) -> String {
        let word: String
        switch device.presence {
        case .nearby: return "Nearby"
        case .reachable: word = "Reachable"
        case .offline: word = "Offline"
        }
        guard let ms = device.lastSeenMs else { return "\(word), not reached yet" }
        let seconds = max(0, now.timeIntervalSince1970 - Double(ms) / 1000)
        let ago: String
        switch seconds {
        case ..<60: ago = "just now"
        case ..<3600: ago = "\(Int(seconds / 60)) min ago"
        case ..<172_800: ago = "\(Int(seconds / 3600)) h ago"
        default: ago = "\(Int(seconds / 86_400)) days ago"
        }
        return "\(word), seen \(ago)"
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
        // The next device waiting to be trusted, if any.
        refreshTrustOffer()
    }

    /// The person answered Not Now: nothing is remembered, and the next
    /// device waiting to be trusted, if any, is offered.
    public func dismissTrustOffer() {
        core.dismissTrustOffer()
        trustOffer = core.trustOffer()
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

    /// Whether `device` speaks for your devices here: what it lists is
    /// offered to you, and what it revoked is revoked here, saying so.
    public func setIntroduces(_ on: Bool, for device: Device) async {
        await perform { try await $0.setIntroduces(endpointId: device.endpointId, on: on) }
    }

    /// Whether `device` may see which devices this one trusts.
    public func setSeesDevices(_ on: Bool, for device: Device) async {
        await perform { try await $0.setSeesDevices(endpointId: device.endpointId, on: on) }
    }

    /// Asks an introducer for its devices now. Returns what changed, in
    /// words, or nil when it failed (the failure is shown as an alert).
    public func syncDevices(with device: Device) async -> String? {
        var said: String?
        do {
            let synced = try await core.syncDevices(endpointId: device.endpointId)
            said = Self.synced(synced, from: device.name)
            reload()
            feedback.play(.success)
        } catch {
            // Nothing was being sent, so not the usual "Not Sent".
            log.warn("failed: \(error)")
            alert = .failure(ForgeFailure(
                title: "Couldn't Check Its Devices",
                message: (error as? RelayError).map(Self.describe) ?? error.localizedDescription,
                isRetryable: false
            ))
            feedback.play(.failure)
        }
        refreshTrustOffer()
        return said
    }

    public static func synced(_ synced: DevicesSynced, from name: String) -> String {
        var parts: [String] = []
        if synced.offered > 0 {
            parts.append(synced.offered == 1 ? "1 device to trust" : "\(synced.offered) devices to trust")
        }
        if synced.revoked > 0 {
            parts.append(synced.revoked == 1 ? "1 device revoked" : "\(synced.revoked) devices revoked")
        }
        return parts.isEmpty ? "Nothing new from \(name)." : "From \(name): " + parts.joined(separator: ", ") + "."
    }

    public func setPolicy(_ policy: PairingPolicy) async {
        await perform { try await $0.setPairingPolicy(policy: policy) }
    }

    public func setConnectionMode(_ mode: ConnectionMode) async {
        await perform { try await $0.setConnectionMode(mode: mode) }
    }

    /// How a trusted device can be reached, in words: nearby, why Relay
    /// couldn't reach it last time, or the route it was last reached by.
    public static func reach(_ device: Device) -> String {
        if device.nearby {
            return "\(device.name) is nearby. No code needed."
        }
        if let problem = device.problem {
            return problem
        }
        if device.presence == .reachable {
            return "\(device.name) can be reached. No code needed."
        }
        if let route = device.route {
            return "Last connected: \(route). Relay tries it there again when something is waiting."
        }
        return "\(device.name) isn't showing on this network. Open Relay on it, on the same Wi-Fi."
    }

    public static func explain(_ mode: ConnectionMode) -> String {
        switch mode {
        case .hybrid:
            "Relay tries the local network first, then a private network such as Tailscale, then the internet and Relay's own servers, if they are on."
        case .private:
            "Relay tries the local network, then a private network such as Tailscale. It never connects over the open internet or through a relay."
        case .local:
            "Relay only connects to devices on the same network."
        }
    }

    /// Sends files and folders (in one folder when several) to a trusted
    /// device. Relay copies them in first, so they can wait in the outbox
    /// and the picked copies can go when this returns.
    @discardableResult
    public func send(paths: [String], toDevice device: Device) async -> Bool {
        await sendingToDevice { try await $0.sendToDevice(endpointId: device.endpointId, paths: paths) }
    }

    /// Stops a send to a trusted device that is on its way, or takes it
    /// off the outbox while it waits.
    public func cancel(_ transfer: Transfer) {
        core.cancelTransfer(id: transfer.id)
    }

    private func sendingToDevice(_ send: (RelayCore) async throws -> Transfer) async -> Bool {
        reload()
        do {
            let sent = try await send(core)
            reload()
            // A waiting item is queued, not sent: no success tap yet.
            if sent.status == .done { feedback.play(.success) }
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
        case .WordCode(let reason): return reason.prefix(1).uppercased() + reason.dropFirst() + "."
        case .UnknownPeer: return "This device doesn't trust that one any more."
        case .NotFound: return "That's no longer in the history."
        case .Unreachable: return "Can't reach the other device. Check that it has Relay open, on the same Wi-Fi."
        case .Refused: return "The other device didn’t accept this one. Send with a code instead."
        case .Declined(let reason): return "The other device declined: \(reason)."
        case .Connection: return "The connection broke. Try again."
        case .File: return "The file couldn't be read or kept."
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
        case .Refused: return "Not Accepted"
        case .WordCode: return "Check the Code"
        case .UnknownPeer, .NotFound: return "Not Found"
        case .Unreachable, .Connection, .Declined: return "Not Sent"
        case .File: return "File Problem"
        case .InUse: return "Already Open"
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

    /// "Waiting for MacBook", "Sending 40%", "Sent", "Received", "Failed: …".
    public static func status(_ transfer: Transfer) -> String {
        switch transfer.status {
        case .waiting:
            return "Waiting for \(transfer.peerName)"
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

    /// Where to reach a device found by Bonjour, as Rust takes it.
    public static func address(host: String, port: UInt16) -> String {
        // An IPv6 host may carry a scope ("fe80::1%en0"); Rust wants brackets.
        host.contains(":") ? "[\(host)]:\(port)" : "\(host):\(port)"
    }
}
