#if os(iOS)
import Foundation
import Network
import Observation

/// Bonjour for Relay: advertises this device's Rust-owned port as
/// `_forgerelay._tcp`, named by its device id, and browses for the same, so
/// a send goes to where a paired device is now. Rust owns the TCP port and
/// every connection; this only finds addresses.
@Observable
@MainActor
final class RelayNetwork {
    static let serviceType = "_forgerelay._tcp"

    /// Device ids seen on this network now.
    private(set) var nearby: Set<String> = []

    @ObservationIgnored private var browser: NWBrowser?
    @ObservationIgnored private var endpoints: [String: NWEndpoint] = [:]
    @ObservationIgnored private var advertiser: NetService?

    /// Advertises `port` under `name` and starts browsing.
    func start(name: String, port: UInt16) {
        if advertiser == nil {
            // NetService advertises a port opened elsewhere (Rust's);
            // NWListener would need to own the socket itself.
            let service = NetService(domain: "local.", type: Self.serviceType + ".", name: name, port: Int32(port))
            service.publish()
            advertiser = service
        }
        guard browser == nil else { return }
        let browser = NWBrowser(for: .bonjour(type: Self.serviceType, domain: nil), using: .tcp)
        browser.browseResultsChangedHandler = { [weak self] results, _ in
            var found: [String: NWEndpoint] = [:]
            for result in results {
                if case .service(let name, _, _, _) = result.endpoint {
                    found[name] = result.endpoint
                }
            }
            Task { @MainActor in
                self?.endpoints = found
                self?.nearby = Set(found.keys)
            }
        }
        browser.start(queue: .main)
        self.browser = browser
    }

    func stop() {
        advertiser?.stop()
        advertiser = nil
        browser?.cancel()
        browser = nil
        endpoints = [:]
        nearby = []
    }

    /// Where `deviceID` can be reached now, as "host:port" for Rust; empty
    /// when it isn't advertising (Rust then tries where it was last). The
    /// service is resolved by opening a connection and reading where it
    /// went, then closing it; Relay's listener drops a connection that
    /// sends nothing.
    func addresses(for deviceID: String) async -> [String] {
        guard let endpoint = endpoints[deviceID] else { return [] }
        let connection = NWConnection(to: endpoint, using: .tcp)
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
            // On the main queue, like every callback this class gets.
            connection.start(queue: .main)
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) { once.resume(nil) }
        }
        connection.cancel()
        return resolved.map { [$0] } ?? []
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

    /// This device's addresses on the local network, for a pairing code:
    /// Wi-Fi (and wired) IPv4 first, then IPv6, never loopback.
    nonisolated static func localHosts() -> [String] {
        var v4: [String] = []
        var v6: [String] = []
        var list: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&list) == 0, let first = list else { return [] }
        defer { freeifaddrs(list) }
        for pointer in sequence(first: first, next: { $0.pointee.ifa_next }) {
            let entry = pointer.pointee
            let flags = Int32(entry.ifa_flags)
            guard flags & IFF_UP != 0, flags & IFF_LOOPBACK == 0, let address = entry.ifa_addr else { continue }
            let interface = String(cString: entry.ifa_name)
            // Wi-Fi and Ethernet only: not the cellular or VPN interfaces.
            guard interface.hasPrefix("en") else { continue }
            let family = Int32(address.pointee.sa_family)
            guard family == AF_INET || family == AF_INET6 else { continue }
            var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            let length = socklen_t(family == AF_INET ? MemoryLayout<sockaddr_in>.size : MemoryLayout<sockaddr_in6>.size)
            guard getnameinfo(address, length, &host, socklen_t(host.count), nil, 0, NI_NUMERICHOST) == 0 else { continue }
            let text = String(cString: host)
            if family == AF_INET {
                v4.append(text)
            } else if !text.hasPrefix("fe80") {
                // Link-local IPv6 needs a scope Rust can't take.
                v6.append(text.split(separator: "%").first.map(String.init) ?? text)
            }
        }
        return v4 + v6
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
