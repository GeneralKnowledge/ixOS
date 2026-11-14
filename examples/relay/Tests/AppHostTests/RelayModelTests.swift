@testable import RelayApp
@preconcurrency import RelayFFI
import XCTest

/// Two devices, each a real Rust core on its own temporary folder, talking
/// over localhost: pair by code, then send a note.
@MainActor
final class RelayModelTests: XCTestCase {
    func testPairAndSendANote() async throws {
        let hostA = makeTemporaryAppHost()
        let hostB = makeTemporaryAppHost()
        await hostA.start()
        await hostB.start()
        let a = RelayModel(core: try XCTUnwrap(hostA.core))
        let b = RelayModel(core: try XCTUnwrap(hostB.core))
        defer {
            a.stopListening()
            b.stopListening()
        }

        let port = try XCTUnwrap(a.startListening())
        let invitation = try XCTUnwrap(a.makeInvitation(hosts: ["127.0.0.1"]))
        let peer = try await b.pair(code: invitation.code)
        XCTAssertEqual(peer.deviceId, a.identity.deviceId)
        XCTAssertEqual(b.peers.count, 1)

        let sent = await b.send(note: "Hello from B", to: peer, addresses: [RelayFormat.address(host: "127.0.0.1", port: port)])
        XCTAssertTrue(sent)
        XCTAssertEqual(b.transfers.first.map { RelayFormat.status($0) }, "Sent")

        a.reload()
        XCTAssertEqual(a.peers.count, 1)
        let received = try XCTUnwrap(a.transfers.first)
        XCTAssertEqual(received.direction, .received)
        XCTAssertEqual(RelayFormat.title(received.item), "Hello from B")
    }

    func testFormatting() {
        XCTAssertEqual(RelayFormat.address(host: "192.168.1.5", port: 4000), "192.168.1.5:4000")
        XCTAssertEqual(RelayFormat.address(host: "fd00::1", port: 4000), "[fd00::1]:4000")
    }
}
