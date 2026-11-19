#if os(iOS)
@preconcurrency import RelayFFI
import Foundation
import Network
import Observation

/// Bonjour for Relay (`_forgerelay._udp`): word-code senders and trusted
/// devices advertise their iroh port, and this finds them for Rust, which
/// owns every connection. This only finds addresses.
@Observable
@MainActor
final class RelayNetwork {

    /// Looks for a code sender (`_forgerelay._udp`) whose TXT record says
    /// code number `number`, for up to 10 s, then 1.5 s more for a second
    /// one. Each sender is resolved to an IPv4 "host:port" (Rust can't use a
    /// link-local IPv6 scope).
    func findCodeSender(number: UInt8) async -> RelayModel.CodeSender {
        let browser = NWBrowser(
            for: .bonjourWithTXTRecord(type: Self.codeServiceType, domain: nil),
            using: .udp
        )
        let matches = CodeMatches()
        browser.browseResultsChangedHandler = { results, _ in
            for result in results {
                guard case .bonjour(let txt) = result.metadata,
                      txt["n"] == String(number),
                      let id = txt["id"], !id.isEmpty
                else { continue }
                matches.add(id: id, endpoint: result.endpoint)
            }
        }
        browser.start(queue: .main)
        var waited = 0
        var firstSeenAt: Int?
        while waited < 100 {
            try? await Task.sleep(for: .milliseconds(100))
            waited += 1
            if matches.count > 0, firstSeenAt == nil { firstSeenAt = waited }
            if let first = firstSeenAt, waited - first >= 15 { break }
        }
        browser.cancel()
        let found = matches.all
        if found.count > 1 { return .several }
        guard let (id, endpoint) = found.first else { return .none }
        var addresses: [String] = []
        if let v4 = await Self.resolve(endpoint, ipv4Only: true) { addresses.append(v4) }
        if addresses.isEmpty, let any = await Self.resolve(endpoint, ipv4Only: false) { addresses.append(any) }
        if addresses.isEmpty { return .none }
        return .found(addresses: addresses, endpointID: id)
    }

    static let codeServiceType = "_forgerelay._udp"

    @ObservationIgnored private var codeAdvertiser: NetService?
    @ObservationIgnored private var codeAdvertised: RelayModel.CodeAdvert?
    @ObservationIgnored private var codeWatcher: NWBrowser?

    /// Advertises a code sender (`_forgerelay._udp`, TXT `v`, `n`, `id`, as
    /// the M0 spike does) on Rust's port, replacing the last advert; `nil`
    /// withdraws it.
    func advertiseCode(_ advert: RelayModel.CodeAdvert?) {
        guard advert != codeAdvertised else { return }
        codeAdvertiser?.stop()
        codeAdvertiser = nil
        codeAdvertised = advert
        guard let advert else { return }
        let name = "relay-\(advert.endpointID.prefix(8))-\(advert.number)"
        let service = NetService(domain: "local.", type: Self.codeServiceType + ".", name: name, port: Int32(advert.port))
        let txt: [String: Data] = [
            "v": Data("1".utf8),
            "n": Data(String(advert.number).utf8),
            "id": Data(advert.endpointID.utf8),
        ]
        _ = service.setTXTRecord(NetService.data(fromTXTRecord: txt))
        service.publish()
        codeAdvertiser = service
    }

    /// Publishes the code advert again. iOS drops an app's Bonjour
    /// registrations while it is suspended (for example while the person
    /// switches app to pass the code on), and `advertiseCode` alone would
    /// think it is still published, so the sheet calls this on return.
    func readvertiseCode(_ advert: RelayModel.CodeAdvert?) {
        codeAdvertised = nil
        advertiseCode(advert)
    }

    /// Reports every code sender advertising nearby (this device's own
    /// included; Rust leaves its own id out) until `stopWatchingCodeSenders`.
    func watchCodeSenders(_ report: @escaping @MainActor @Sendable ([NearbySender]) -> Void) {
        guard codeWatcher == nil else { return }
        let browser = NWBrowser(
            for: .bonjourWithTXTRecord(type: Self.codeServiceType, domain: nil),
            using: .udp
        )
        browser.browseResultsChangedHandler = { results, _ in
            let senders: [NearbySender] = results.compactMap { result -> NearbySender? in
                guard case .bonjour(let txt) = result.metadata,
                      let n = txt["n"].flatMap({ UInt8($0) }),
                      let id = txt["id"], !id.isEmpty
                else { return nil }
                return NearbySender(number: n, endpointId: id)
            }
            Task { @MainActor in report(senders) }
        }
        browser.start(queue: .main)
        codeWatcher = browser
    }

    func stopWatchingCodeSenders() {
        codeWatcher?.cancel()
        codeWatcher = nil
    }

    // MARK: Trusted devices (relay/device/1)

    @ObservationIgnored private var deviceAdvertiser: NetService?
    @ObservationIgnored private var deviceAdvertised: DeviceService?
    @ObservationIgnored private var deviceWatcher: NWBrowser?
    @ObservationIgnored private var deviceAddresses: [String: String] = [:]

    /// Advertises this device's iroh endpoint for trusted devices
    /// (`_forgerelay._udp`, TXT `v=1`, `d=1`, `id`; no `n`, so code
    /// receivers never take it for a code sender). `nil` withdraws it.
    func advertiseDevice(_ service: DeviceService?) {
        guard service != deviceAdvertised else { return }
        deviceAdvertiser?.stop()
        deviceAdvertiser = nil
        deviceAdvertised = service
        guard let service else { return }
        let name = "relay-\(service.endpointId.prefix(12))"
        let advert = NetService(domain: "local.", type: Self.codeServiceType + ".", name: name, port: Int32(service.port))
        let txt: [String: Data] = [
            "v": Data("1".utf8),
            "d": Data("1".utf8),
            "id": Data(service.endpointId.utf8),
        ]
        _ = advert.setTXTRecord(NetService.data(fromTXTRecord: txt))
        advert.publish()
        deviceAdvertiser = advert
    }

    /// Reports the devices advertising `d=1` nearby, each resolved to an
    /// IPv4 "host:port" (Rust can't use a link-local IPv6 scope), until
    /// `stopWatchingDevices`. Rust keeps only the ones it trusts.
    func watchDevices(_ report: @escaping @MainActor @Sendable ([NearbyDevice]) -> Void) {
        guard deviceWatcher == nil else { return }
        let browser = NWBrowser(
            for: .bonjourWithTXTRecord(type: Self.codeServiceType, domain: nil),
            using: .udp
        )
        browser.browseResultsChangedHandler = { [weak self] results, _ in
            let found: [(String, NWEndpoint)] = results.compactMap { result in
                guard case .bonjour(let txt) = result.metadata,
                      txt["d"] == "1",
                      let id = txt["id"], !id.isEmpty
                else { return nil }
                return (id, result.endpoint)
            }
            Task { @MainActor in
                guard let self else { return }
                var nearby: [NearbyDevice] = []
                for (id, endpoint) in found {
                    if let known = self.deviceAddresses[id] {
                        nearby.append(NearbyDevice(endpointId: id, addresses: [known]))
                    } else if let address = await Self.resolve(endpoint, ipv4Only: true) {
                        self.deviceAddresses[id] = address
                        nearby.append(NearbyDevice(endpointId: id, addresses: [address]))
                    }
                }
                // Forget where devices that left were.
                let ids = Set(found.map(\.0))
                self.deviceAddresses = self.deviceAddresses.filter { ids.contains($0.key) }
                report(nearby)
            }
        }
        browser.start(queue: .main)
        deviceWatcher = browser
    }

    func stopWatchingDevices() {
        deviceWatcher?.cancel()
        deviceWatcher = nil
        deviceAddresses = [:]
    }

    /// A Bonjour UDP service's "host:port": a UDP connection is ready once
    /// the name resolves, and sends nothing.
    private static func resolve(_ endpoint: NWEndpoint, ipv4Only: Bool) async -> String? {
        let parameters = NWParameters.udp
        if ipv4Only, let ip = parameters.defaultProtocolStack.internetProtocol as? NWProtocolIP.Options {
            ip.version = .v4
        }
        let connection = NWConnection(to: endpoint, using: parameters)
        let resolved: String? = await withCheckedContinuation { continuation in
            let once = Once(continuation)
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    once.resume(Self.hostPort(connection.currentPath?.remoteEndpoint))
                case .failed, .cancelled:
                    once.resume(nil)
                default:
                    break
                }
            }
            connection.start(queue: .main)
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) { once.resume(nil) }
        }
        connection.cancel()
        return resolved
    }

    nonisolated private static func hostPort(_ endpoint: NWEndpoint?) -> String? {
        guard case .hostPort(let host, let port) = endpoint else { return nil }
        let name: String
        switch host {
        case .ipv4(let address): name = "\(address)"
        case .ipv6(let address): name = "\(address)"
        case .name(let text, _): name = text
        @unknown default: return nil
        }
        // Rust can't use an IPv6 scope such as "%en0" in "host:port".
        let bare = name.split(separator: "%").first.map(String.init) ?? name
        return RelayFormat.address(host: bare, port: port.rawValue)
    }
}

/// Code senders seen so far, by endpoint id (Bonjour may report one twice).
private final class CodeMatches: @unchecked Sendable {
    private let lock = NSLock()
    private var found: [String: NWEndpoint] = [:]

    func add(id: String, endpoint: NWEndpoint) {
        lock.lock()
        found[id] = endpoint
        lock.unlock()
    }

    var count: Int {
        lock.lock()
        defer { lock.unlock() }
        return found.count
    }

    var all: [(String, NWEndpoint)] {
        lock.lock()
        defer { lock.unlock() }
        return found.map { ($0.key, $0.value) }
    }
}

/// Resumes a continuation once, from whichever callback comes first.
private final class Once: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<String?, Never>?

    init(_ continuation: CheckedContinuation<String?, Never>) {
        self.continuation = continuation
    }

    func resume(_ value: String?) {
        lock.lock()
        let pending = continuation
        continuation = nil
        lock.unlock()
        pending?.resume(returning: value)
    }
}
#endif
