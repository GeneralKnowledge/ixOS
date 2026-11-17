@testable import AppHost
import XCTest

/// Naming and ordering the phone's own IP addresses, on fixed values.
final class NetworkAddressTests: XCTestCase {
    func testInterfacesGetTheirKindAndTailscaleIsRecognised() {
        let wifi = NetworkAddress(interface: "en0", address: "192.168.1.42", isIPv6: false)
        let cell = NetworkAddress(interface: "pdp_ip0", address: "10.20.30.40", isIPv6: false)
        let tailscale = NetworkAddress(interface: "utun4", address: "100.101.102.103", isIPv6: false)
        let tailscale6 = NetworkAddress(interface: "utun4", address: "fd7a:115c:a1e0::1", isIPv6: true)
        let otherVPN = NetworkAddress(interface: "utun2", address: "10.8.0.2", isIPv6: false)
        XCTAssertEqual(wifi.label, "Wi-Fi")
        XCTAssertEqual(cell.label, "Cellular")
        XCTAssertEqual(tailscale.label, "Tailscale")
        XCTAssertEqual(tailscale6.label, "Tailscale IPv6")
        XCTAssertEqual(otherVPN.label, "VPN")
    }

    func testLoopbackAndLinkLocalAreHidden() {
        XCTAssertFalse(NetworkAddress.isShown("127.0.0.1", isIPv6: false))
        XCTAssertFalse(NetworkAddress.isShown("169.254.3.4", isIPv6: false))
        XCTAssertFalse(NetworkAddress.isShown("fe80::1", isIPv6: true))
        XCTAssertTrue(NetworkAddress.isShown("2a02:c7c::1", isIPv6: true))
    }

    func testWifiFirstThenCellularThenVPNAndIPv4BeforeIPv6() {
        let sorted = NetworkAddress.sorted([
            NetworkAddress(interface: "utun4", address: "100.101.102.103", isIPv6: false),
            NetworkAddress(interface: "en0", address: "2a02:c7c::1", isIPv6: true),
            NetworkAddress(interface: "pdp_ip0", address: "10.20.30.40", isIPv6: false),
            NetworkAddress(interface: "en0", address: "192.168.1.42", isIPv6: false),
            NetworkAddress(interface: "en0", address: "192.168.1.42", isIPv6: false),
        ])
        XCTAssertEqual(sorted.map(\.address), ["192.168.1.42", "2a02:c7c::1", "10.20.30.40", "100.101.102.103"])
    }
}
