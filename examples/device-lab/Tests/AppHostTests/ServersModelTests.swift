@testable import AppHost
import Foundation
import XCTest

/// The Servers screen's checks, with a fake probe instead of the network.
@MainActor
final class ServersModelTests: XCTestCase {
    private func server(_ app: String, _ address: String?) -> ServersModel.Server {
        ServersModel.Server(app: app, role: "Server", address: address, setUp: "Set it up")
    }

    func testAnAnswerIsUpAndAnErrorIsDownWithAReason() async {
        let model = ServersModel(
            servers: [server("Up", "100.72.135.124:8642"), server("Down", "http://192.168.1.169:8787")],
            probe: { url in
                if url.port == 8642 { return .milliseconds(42) }
                throw URLError(.cannotConnectToHost)
            })
        await model.checkAll()
        XCTAssertEqual(model.check(model.servers[0]), .answered(milliseconds: 42))
        XCTAssertEqual(
            model.check(model.servers[1]),
            .unreachable("Nothing is listening there: is the server running?"))
    }

    func testAServerWithoutAnAddressIsNotChecked() async {
        let model = ServersModel(servers: [server("None", nil)], probe: { _ in
            XCTFail("nothing to check")
            return .zero
        })
        await model.checkAll()
        XCTAssertEqual(model.check(model.servers[0]), .notChecked)
    }

    func testAddressesWithoutASchemeGetHttp() {
        XCTAssertEqual(ServersModel.url("100.72.135.124:8642")?.absoluteString, "http://100.72.135.124:8642")
        XCTAssertEqual(ServersModel.url("https://chat.example.org")?.absoluteString, "https://chat.example.org")
        XCTAssertNil(ServersModel.url("  "))
    }

    func testTailscaleAddressesAreRecognised() {
        XCTAssertTrue(ServersModel.isTailscale("100.72.135.124:8642"))
        XCTAssertFalse(ServersModel.isTailscale("192.168.1.169:8787"))
        XCTAssertFalse(ServersModel.isTailscale("100.200.1.1"))
    }
}
