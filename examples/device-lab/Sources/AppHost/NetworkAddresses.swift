import Foundation

/// This phone's own IP addresses, by interface: Wi-Fi, cellular and VPN
/// (Tailscale among them). Apps may read these (`getifaddrs`); it is the
/// MAC address iOS hides. Plain Foundation, so it builds and tests anywhere.
public struct NetworkAddress: Equatable, Hashable, Sendable {
    public enum Kind: String, Sendable, CaseIterable {
        case wifi, cellular, vpn, other
    }

    public let kind: Kind
    /// The interface name (`en0`, `pdp_ip0`, `utun3`).
    public let interface: String
    public let address: String
    public let isIPv6: Bool

    /// Tailscale gives every device a 100.64.0.0/10 address and an
    /// fd7a:115c:a1e0::/48 one.
    public var isTailscale: Bool {
        if isIPv6 { return address.lowercased().hasPrefix("fd7a:115c:a1e0:") }
        let parts = address.split(separator: ".").compactMap { Int($0) }
        return parts.count == 4 && parts[0] == 100 && (64...127).contains(parts[1])
    }

    /// "Wi-Fi", "Cellular IPv6", "VPN (Tailscale)".
    public var label: String {
        let name = switch kind {
        case .wifi: "Wi-Fi"
        case .cellular: "Cellular"
        case .vpn: isTailscale ? "Tailscale" : "VPN"
        case .other: interface
        }
        return isIPv6 ? "\(name) IPv6" : name
    }

    public init(interface: String, address: String, isIPv6: Bool) {
        self.interface = interface
        self.address = address
        self.isIPv6 = isIPv6
        kind = Self.kind(of: interface)
    }

    static func kind(of interface: String) -> Kind {
        if interface.hasPrefix("en") { return .wifi }
        if interface.hasPrefix("pdp_ip") { return .cellular }
        if interface.hasPrefix("utun") || interface.hasPrefix("ipsec") || interface.hasPrefix("ppp") { return .vpn }
        return .other
    }

    /// Whether an address is worth showing: not loopback, and not an IPv6
    /// link-local one (fe80::), which every interface has.
    static func isShown(_ address: String, isIPv6: Bool) -> Bool {
        if isIPv6 { return !address.lowercased().hasPrefix("fe80:") && address != "::1" }
        return !address.hasPrefix("127.") && !address.hasPrefix("169.254.")
    }

    /// Every address the phone has now, Wi-Fi first, IPv4 before IPv6.
    public static func current() -> [NetworkAddress] {
        #if canImport(Darwin)
            var list: UnsafeMutablePointer<ifaddrs>?
            guard getifaddrs(&list) == 0, let first = list else { return [] }
            defer { freeifaddrs(list) }
            var found: [NetworkAddress] = []
            for pointer in sequence(first: first, next: { $0.pointee.ifa_next }) {
                let entry = pointer.pointee
                guard let sockaddr = entry.ifa_addr,
                      (entry.ifa_flags & UInt32(IFF_UP)) != 0
                else { continue }
                let family = Int32(sockaddr.pointee.sa_family)
                guard family == AF_INET || family == AF_INET6 else { continue }
                var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
                let length = socklen_t(family == AF_INET ? MemoryLayout<sockaddr_in>.size : MemoryLayout<sockaddr_in6>.size)
                guard getnameinfo(sockaddr, length, &host, socklen_t(host.count), nil, 0, NI_NUMERICHOST) == 0 else { continue }
                var address = String(cString: host)
                // Drop the scope suffix (`%en0`).
                if let percent = address.firstIndex(of: "%") { address = String(address[..<percent]) }
                let isIPv6 = family == AF_INET6
                guard isShown(address, isIPv6: isIPv6) else { continue }
                found.append(NetworkAddress(interface: String(cString: entry.ifa_name), address: address, isIPv6: isIPv6))
            }
            return sorted(found)
        #else
            return []
        #endif
    }

    static func sorted(_ addresses: [NetworkAddress]) -> [NetworkAddress] {
        let order = Kind.allCases
        return Array(Set(addresses)).sorted {
            let a = order.firstIndex(of: $0.kind)!, b = order.firstIndex(of: $1.kind)!
            if a != b { return a < b }
            if $0.isIPv6 != $1.isIPv6 { return !$0.isIPv6 }
            return ($0.interface, $0.address) < ($1.interface, $1.address)
        }
    }
}
