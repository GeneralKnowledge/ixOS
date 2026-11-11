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

    func testFormatting() {
        XCTAssertEqual(TripFormat.duration(milliseconds: 65_000), "1:05")
        XCTAssertEqual(TripFormat.duration(milliseconds: 3_725_000), "1:02:05")
    }
}
