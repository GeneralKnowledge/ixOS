@testable import FieldKitApp
@preconcurrency import FieldKitFFI
import XCTest

/// The model against the real Rust core on a temporary folder (no location
/// service is installed here, so recording keeps no fixes).
@MainActor
final class TripsModelTests: XCTestCase {
    func testStartAddNoteAndStop() async throws {
        let host = makeTemporaryAppHost()
        await host.start()
        let core = try XCTUnwrap(host.core)
        let model = TripsModel(core: core)
        XCTAssertTrue(model.trips.isEmpty)

        try await model.start(title: "Coast path")
        let trip = try XCTUnwrap(model.recording)
        XCTAssertEqual(trip.title, "Coast path")
        XCTAssertNotNil(model.locationProblem)

        try await model.addNote(to: trip.id, text: "Stile is broken")
        XCTAssertEqual(model.recording?.noteCount, 1)

        await model.handle(.stop)
        XCTAssertNil(model.recording)
        XCTAssertEqual(model.trips.count, 1)
    }

    func testATripHasElevationAndPaceCharts() async throws {
        let host = makeTemporaryAppHost()
        await host.start()
        let core = try XCTUnwrap(host.core)
        let model = TripsModel(core: core)
        try await model.start(title: "Hill")
        let trip = try XCTUnwrap(model.recording)
        // 120 fixes about 11 m and 5 s apart, climbing 1 m each: about 1.3 km.
        let samples = (0..<120).map { i in
            LocationSample(
                latitude: 51.5 + 0.0001 * Double(i), longitude: -0.12,
                altitudeM: 20 + Double(i), horizontalAccuracyM: 8,
                timestampMs: Int64(i) * 5000)
        }
        _ = try await core.record(samples: samples)
        await model.handle(.stop)

        let loaded = try await model.load(trip.id)
        XCTAssertEqual(loaded.track.count, 120)
        XCTAssertEqual(loaded.profile.elevation.first?.altitudeM, 20)
        XCTAssertEqual(loaded.profile.elevation.last?.altitudeM, 139)
        XCTAssertEqual(loaded.profile.splits.map(\.number), [1, 2])
        let pace = try XCTUnwrap(loaded.profile.splits.first?.secondsPerKm)
        XCTAssertEqual(pace, 450, accuracy: 5)
        XCTAssertEqual(TripFormat.pace(secondsPerKm: pace), "7:30 /km")
    }

    func testFormatting() {
        XCTAssertEqual(TripFormat.duration(milliseconds: 65_000), "1:05")
        XCTAssertEqual(TripFormat.duration(milliseconds: 3_725_000), "1:02:05")
        XCTAssertEqual(TripFormat.pace(secondsPerKm: 359.6), "6:00 /km")
    }
}
