import AppHost
@preconcurrency import ForgeFFI
import XCTest

/// The Logs screen's filter, search and share text, on fixed records.
@MainActor
final class LogsModelTests: XCTestCase {
    private let lines = [
        LogLine(timeMs: 1_000, level: .info, target: "weather::forecast", message: "refreshed"),
        LogLine(timeMs: 2_000, level: .warn, target: "soundscape::player", message: "interrupted"),
        LogLine(timeMs: 3_000, level: .error, target: "relay::transfer", message: "peer went away"),
    ]

    func testNewestFirstAndFilteredByLevel() {
        let model = LogsModel(read: { self.lines })
        model.refresh()
        XCTAssertEqual(model.shown.map(\.timeMs), [3_000, 2_000, 1_000])
        model.filter = .warnings
        XCTAssertEqual(model.shown.map(\.timeMs), [3_000, 2_000])
        model.filter = .errors
        XCTAssertEqual(model.shown.map(\.timeMs), [3_000])
    }

    func testSearchMatchesMessageOrSource() {
        let model = LogsModel(read: { self.lines })
        model.refresh()
        model.search = "SOUNDSCAPE"
        XCTAssertEqual(model.shown.map(\.message), ["interrupted"])
        model.search = "peer"
        XCTAssertEqual(model.shown.map(\.target), ["relay::transfer"])
    }

    func testShareTextIsOldestFirst() {
        let model = LogsModel(read: { self.lines })
        model.refresh()
        let text = model.shareText.split(separator: "\n")
        XCTAssertEqual(text.count, 3)
        XCTAssertTrue(text[0].hasSuffix("[weather::forecast] refreshed"))
        XCTAssertTrue(text[2].hasSuffix("[relay::transfer] peer went away"))
    }
}
