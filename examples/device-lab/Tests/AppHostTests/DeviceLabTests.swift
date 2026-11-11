import AppHost
@preconcurrency import ForgeFFI
import Foundation
import XCTest

/// The optional services' Swift adapters and the dashboard model, against
/// the app's real Rust core. Runs under `swift test` on a Mac: connectivity
/// and device status are real; motion, location and Bluetooth exist only on
/// iOS and must say so instead of failing.
final class DeviceLabTests: XCTestCase {
    override func setUp() {
        super.setUp()
        XCTAssertTrue(ForgeServices.install(keychainService: "forge-tests-\(UUID().uuidString)"))
    }

    func testOnlyTheSelectedAdaptersAreInstalled() {
        XCTAssertEqual(
            forgeSelectedServices.map(\.name).sorted(),
            ["Bluetooth", "Connectivity", "DeviceStatus", "Location", "Motion", "Transfers"])
    }

    func testConnectivityReportsAPath() {
        let status = ForgeConnectivityMonitor.shared.current()
        XCTAssertNotEqual(status.observedAtMs, 0)
        if status.reachability == .online {
            XCTAssertFalse(status.interfaces.isEmpty)
            XCTAssertNil(status.offlineReason)
        }
    }

    func testDeviceStatusReadsTheMachine() {
        let info = ForgeDeviceStatus.shared.info()
        XCTAssertFalse(info.modelIdentifier.isEmpty)
        XCTAssertGreaterThan(info.processorCount, 0)
        XCTAssertGreaterThan(info.physicalMemoryBytes, 0)
        let status = ForgeDeviceStatus.shared.status()
        XCTAssertGreaterThan(status.uptimeSeconds, 0)
        XCTAssertNotNil(status.memoryFootprintBytes)
        if let total = status.storageTotalBytes, let free = status.storageAvailableBytes {
            XCTAssertGreaterThan(total, 0)
            XCTAssertLessThanOrEqual(free, total)
        }
    }

    func testSensorsThatOnlyExistOnIOSSaySo() async {
        #if !os(iOS)
            let motion = ForgeMotionSensors.shared.availability()
            XCTAssertFalse(motion.accelerometer || motion.altimeter)
            let location = await ForgePermissions.shared.status(permission: .location)
            XCTAssertEqual(location, .unsupported)
            XCTAssertEqual(ForgeBluetoothMonitor.shared.state(), .unsupported)
        #endif
    }

    @MainActor
    func testModelFollowsRustAndStopsInTheBackground() async {
        let model = LabModel()
        model.setActive(true)
        XCTAssertTrue(model.snapshot.active)
        XCTAssertTrue(model.snapshot.network.available)
        XCTAssertTrue(model.snapshot.device.available)
        XCTAssertNotNil(model.snapshot.device.info)

        // The path monitor answers on its own queue.
        for _ in 0..<50 where model.snapshot.network.status == nil {
            try? await Task.sleep(for: .milliseconds(100))
            model.refresh()
        }
        XCTAssertNotNil(model.snapshot.network.status)
        XCTAssertFalse(model.snapshot.network.changes.isEmpty)

        await model.refreshPermissions()
        #if !os(iOS)
            XCTAssertEqual(model.snapshot.permissions.location, .unsupported)
            XCTAssertFalse(model.snapshot.accelerometer.available)
            XCTAssertFalse(model.snapshot.accelerometer.running)
        #endif

        model.setActive(false)
        XCTAssertFalse(model.snapshot.active)
        XCTAssertFalse(model.snapshot.location.running)
        model.reset()
    }
}
