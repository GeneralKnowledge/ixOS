@testable import RelayApp
@preconcurrency import RelayFFI
import ForgeAppKit
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
        XCTAssertEqual(b.feedback.kind, .success)

        let sent = await b.send(note: "Hello from B", to: peer, addresses: [RelayFormat.address(host: "127.0.0.1", port: port)])
        XCTAssertTrue(sent)
        XCTAssertEqual(b.transfers.first.map { RelayFormat.status($0) }, "Sent")

        a.reload()
        XCTAssertEqual(a.peers.count, 1)
        let received = try XCTUnwrap(a.transfers.first)
        XCTAssertEqual(received.direction, .received)
        XCTAssertEqual(RelayFormat.title(received.item), "Hello from B")
        // The note arrived by itself: a tap on A, once.
        XCTAssertEqual(a.feedback.kind, .success)
        let count = a.feedback.count
        a.reload()
        XCTAssertEqual(a.feedback.count, count)
    }

    /// Which haptic a new read of the history plays: received items that
    /// finished since the last read, never sends or old items.
    func testArrivalFeedback() {
        func transfer(_ id: UInt64, _ direction: Direction, _ status: TransferStatus) -> Transfer {
            Transfer(id: id, peerId: "p", peerName: "Phone", direction: direction,
                     item: .note(text: "Hi"), status: status, atMs: 0)
        }
        let running = transfer(1, .received, .inProgress(done: 10))
        let done = transfer(1, .received, .done)
        XCTAssertEqual(RelayModel.arrival(from: [running], to: [done]), .success)
        XCTAssertEqual(RelayModel.arrival(from: [], to: [done]), .success)
        XCTAssertNil(RelayModel.arrival(from: [done], to: [done]))
        XCTAssertNil(RelayModel.arrival(from: [], to: [transfer(2, .sent, .done)]))
        XCTAssertNil(RelayModel.arrival(from: [], to: [running]))
        XCTAssertEqual(
            RelayModel.arrival(from: [running], to: [transfer(1, .received, .failed(reason: "gone")), transfer(3, .received, .done)]),
            .failure)
        // A send waiting in the outbox says who for, and plays nothing.
        let waiting = transfer(4, .sent, .waiting)
        XCTAssertEqual(RelayFormat.status(waiting), "Waiting for Phone")
        XCTAssertNil(RelayModel.arrival(from: [waiting], to: [transfer(4, .sent, .done)]))
    }

    /// A code transfer, "Trust this device?" on both sides, then a note to
    /// the trusted device with no code; revoking refuses the next one.
    func testTrustAfterACodeTransferThenSendWithoutACode() async throws {
        let hostA = makeTemporaryAppHost()
        let hostB = makeTemporaryAppHost()
        await hostA.start()
        await hostB.start()
        let a = RelayModel(core: try XCTUnwrap(hostA.core))
        let b = RelayModel(core: try XCTUnwrap(hostB.core))

        let file = FileManager.default.temporaryDirectory.appendingPathComponent("trust-\(UUID().uuidString).txt")
        try Data("hello".utf8).write(to: file)
        defer { try? FileManager.default.removeItem(at: file) }
        a.send(fileAt: file.path)
        let advert = try await eventually { a.refreshCodeSend(); return a.codeAdvert }
        guard case .waiting(let code, _, _, _, _, _, _) = a.codeSend else { return XCTFail("no code") }
        await b.receive(code: code) { _ in
            .found(addresses: [RelayFormat.address(host: "127.0.0.1", port: advert.port)], endpointID: advert.endpointID)
        }
        guard case .received = b.codeReceive else { return XCTFail("\(b.codeReceive)") }

        let offerOnB = try XCTUnwrap(b.trustOffer)
        let offerOnA = try await eventually { a.refreshCodeSend(); return a.trustOffer }
        await b.trust(offerOnB)
        await a.trust(offerOnA)
        XCTAssertNil(b.trustOffer)
        XCTAssertEqual(b.trustedDevices.map(\.endpointId), [advert.endpointID])

        // B tells A where B is, as Bonjour would.
        let maybeService = await b.deviceService()
        let service = try XCTUnwrap(maybeService)
        a.devicesNearby([NearbyDevice(endpointId: service.endpointId, addresses: [RelayFormat.address(host: "127.0.0.1", port: service.port)])])
        let toB = try XCTUnwrap(a.device(service.endpointId))
        XCTAssertTrue(toB.nearby)
        let sent = await a.send(note: "No code", toDevice: toB)
        XCTAssertTrue(sent)
        b.reload()
        XCTAssertEqual(b.transfers.first.map { RelayFormat.title($0.item) }, "No code")

        // Revoked on B: A's next note is refused, with an alert on A.
        let aOnB = try XCTUnwrap(b.device(advert.endpointID))
        b.askToRevoke(aOnB)
        await b.handle(.revoke(endpointID: aOnB.endpointId))
        XCTAssertEqual(b.revokedDevices.count, 1)
        let refused = await a.send(note: "Again", toDevice: toB)
        XCTAssertFalse(refused)
        XCTAssertNotNil(a.alert)
    }

    /// Polls `value` for up to 15 s.
    private func eventually<T>(_ value: () -> T?) async throws -> T {
        for _ in 0..<750 {
            if let found = value() { return found }
            try await Task.sleep(nanoseconds: 20_000_000)
        }
        XCTFail("timed out")
        throw TimedOut()
    }

    func testFormatting() {
        XCTAssertEqual(RelayFormat.address(host: "192.168.1.5", port: 4000), "192.168.1.5:4000")
        XCTAssertEqual(RelayFormat.address(host: "fd00::1", port: 4000), "[fd00::1]:4000")
    }
}

private struct TimedOut: Error {}
