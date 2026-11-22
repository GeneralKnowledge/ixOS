@testable import RelayApp
@preconcurrency import RelayFFI
import ForgeAppKit
import ForgeKit
import Foundation
import XCTest

/// The screens' model, on real Rust cores in temporary folders talking
/// over localhost where two devices are needed.
@MainActor
final class RelayModelTests: XCTestCase {
    /// The background tells the screens to withdraw Relay's adverts;
    /// coming back tells them to advertise again, with how long the app was
    /// away. The model follows the host's phases, so no view has to watch
    /// for them.
    func testTheBackgroundWithdrawsAndComingBackAdvertisesAgain() async throws {
        let host = makeTemporaryAppHost()
        await host.start()
        var now = Date(timeIntervalSince1970: 1_000)
        host.clock = { now }
        let model = RelayModel(core: try XCTUnwrap(host.core))
        model.follow(host)

        host.report(.inactive)
        host.report(.background)
        XCTAssertTrue(model.isAway)
        XCTAssertNil(host.lastResume)

        now += 90
        host.report(.inactive)
        XCTAssertFalse(model.isAway, "the screens advertise again")
        XCTAssertEqual(model.returns, 1, "an open send code is advertised again")
        XCTAssertEqual(host.lastResume?.away, 90)
        host.report(.active)
        XCTAssertEqual(model.returns, 1, "one return, however many phases follow")

        // Pulling down Control Center is not a return from the background.
        host.report(.inactive)
        host.report(.active)
        XCTAssertEqual(model.returns, 1)
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
        // Files go with no code (text is a message in the conversation).
        let map = FileManager.default.temporaryDirectory.appendingPathComponent("map-\(UUID().uuidString).txt")
        try Data("north".utf8).write(to: map)
        defer { try? FileManager.default.removeItem(at: map) }
        let sent = await a.send(paths: [map.path], toDevice: toB)
        XCTAssertTrue(sent)
        b.reload()
        XCTAssertEqual(b.transfers.first.map { RelayFormat.title($0.item) }, map.lastPathComponent)

        // Revoked on B: A's next send is refused, with an alert on A.
        let aOnB = try XCTUnwrap(b.device(advert.endpointID))
        b.askToRevoke(aOnB)
        await b.handle(.revoke(endpointID: aOnB.endpointId))
        XCTAssertEqual(b.revokedDevices.count, 1)
        let refused = await a.send(paths: [map.path], toDevice: toB)
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

    /// What a sync with an introducer changed, in words.
    func testSyncedText() {
        XCTAssertEqual(RelayModel.synced(DevicesSynced(offered: 0, revoked: 0), from: "laptop"),
                       "Nothing new from laptop.")
        XCTAssertEqual(RelayModel.synced(DevicesSynced(offered: 1, revoked: 2), from: "laptop"),
                       "From laptop: 1 device to trust, 2 devices revoked.")
    }

    /// How a device can be reached, in words: nearby wins, then why it
    /// wasn't reached, then reachable, then the route it was last reached by.
    func testReachText() {
        func device(nearby: Bool, route: String?, problem: String?, presence: Presence = .offline) -> Device {
            Device(endpointId: "id", name: "create", trusted: true, sinceMs: 0, receivesFrom: true, mayAsk: false,
                   introduces: false, seesDevices: false, introducedBy: nil, revokedBy: nil, person: nil,
                   nearby: nearby, presence: nearby ? .nearby : presence, lastSeenMs: nil,
                   platform: nil, offers: [], route: route, problem: problem)
        }
        XCTAssertEqual(RelayModel.reach(device(nearby: true, route: "Private network", problem: "x")),
                       "create is nearby. No code needed.")
        XCTAssertEqual(RelayModel.reach(device(nearby: false, route: "Private network", problem: "create didn't answer.")),
                       "create didn't answer.")
        XCTAssertTrue(RelayModel.reach(device(nearby: false, route: "Private network", problem: nil))
            .hasPrefix("Last connected: Private network."))
        XCTAssertEqual(RelayModel.reach(device(nearby: false, route: "Private network", problem: nil, presence: .reachable)),
                       "create can be reached. No code needed.")
        XCTAssertTrue(RelayModel.reach(device(nearby: false, route: nil, problem: nil))
            .contains("isn't showing on this network"))
    }
}

private struct TimedOut: Error {}
