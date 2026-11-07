// Forge native services: connectivity (optional: `native_services!(connectivity)`).
// Source of truth: crates/forge-services/swift/optional/. Copied into an app's
// Generated/ForgeFFI only when its Rust crate selects the service.

import Foundation
import Network

extension ForgeServices {
    static func installConnectivity() throws {
        installNativeConnectivity(monitor: ForgeConnectivityMonitor.shared)
    }
}

/// `ConnectivityMonitor` backed by `NWPathMonitor`. One monitor runs for the
/// whole process (it costs nothing while the path does not change); Rust's
/// watchers get each change through the sink while they listen.
public final class ForgeConnectivityMonitor: ConnectivityMonitor, @unchecked Sendable {
    public static let shared = ForgeConnectivityMonitor()

    private let monitor = NWPathMonitor()
    private let queue = DispatchQueue(label: "org.forgemobile.services.connectivity")
    private let lock = NSLock()
    private var latest: ConnectivityStatus?
    private var sink: ConnectivitySink?

    private init() {
        monitor.pathUpdateHandler = { [weak self] path in
            self?.update(path)
        }
        monitor.start(queue: queue)
    }

    public func current() -> ConnectivityStatus {
        lock.lock()
        let latest = self.latest
        lock.unlock()
        return latest ?? Self.status(from: monitor.currentPath)
    }

    public func start(sink: ConnectivitySink) {
        lock.lock()
        self.sink = sink
        lock.unlock()
        sink.send(status: current())
    }

    public func stop() {
        lock.lock()
        sink = nil
        lock.unlock()
    }

    private func update(_ path: NWPath) {
        let status = Self.status(from: path)
        lock.lock()
        latest = status
        let sink = self.sink
        lock.unlock()
        sink?.send(status: status)
    }

    static func status(from path: NWPath) -> ConnectivityStatus {
        let reachability: Reachability
        switch path.status {
        case .satisfied: reachability = .online
        case .unsatisfied: reachability = .offline
        case .requiresConnection: reachability = .requiresConnection
        @unknown default: reachability = .unknown
        }

        // availableInterfaces is in order of preference; keep the ones the
        // current route uses.
        var interfaces: [NetworkInterface] = []
        for interface in path.availableInterfaces where path.usesInterfaceType(interface.type) {
            let kind: NetworkInterface
            switch interface.type {
            case .wifi: kind = .wifi
            case .cellular: kind = .cellular
            case .wiredEthernet: kind = .wired
            case .loopback: kind = .loopback
            default: kind = .other
            }
            if !interfaces.contains(kind) { interfaces.append(kind) }
        }

        var offlineReason: OfflineReason?
        if path.status != .satisfied {
            switch path.unsatisfiedReason {
            case .notAvailable: offlineReason = .notAvailable
            case .cellularDenied: offlineReason = .cellularDenied
            case .wifiDenied: offlineReason = .wifiDenied
            case .localNetworkDenied: offlineReason = .localNetworkDenied
            default: offlineReason = .other
            }
        }

        return ConnectivityStatus(
            reachability: reachability,
            interfaces: interfaces,
            isExpensive: path.isExpensive,
            isConstrained: path.isConstrained,
            supportsIpv4: path.supportsIPv4,
            supportsIpv6: path.supportsIPv6,
            supportsDns: path.supportsDNS,
            offlineReason: offlineReason,
            observedAtMs: Int64(Date().timeIntervalSince1970 * 1000))
    }
}
