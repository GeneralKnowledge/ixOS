@testable import PocketLibraryApp
@preconcurrency import PocketLibraryFFI
import AVFoundation
import XCTest

/// Read-aloud's pure parts. Speech itself needs a device to hear.
final class ReadAloudTests: XCTestCase {
    private func span(_ text: String, bold: Bool = false) -> ReaderSpan {
        ReaderSpan(text: text, bold: bold, italic: false, monospace: false, link: nil)
    }

    private func block(_ kind: ReaderBlockKind, _ spans: [ReaderSpan] = []) -> ReaderBlock {
        ReaderBlock(id: "", kind: kind, spans: spans)
    }

    func testSpokenTextIsTheShownText() {
        let paragraph = block(.paragraph, [span("It was a "), span("dark", bold: true), span(" night.")])
        XCTAssertEqual(ReadAloud.speakableText(paragraph), "It was a dark night.")
        // The marker is shown beside the item, not spoken.
        let item = block(.listItem(marker: "3."), [span("Three")])
        XCTAssertEqual(ReadAloud.speakableText(item), "Three")
        XCTAssertEqual(ReadAloud.speakableText(block(.separator)), "")
        XCTAssertEqual(ReadAloud.speakableText(block(.image(source: "/x.png", alt: "Map"))), "")
    }

    func testSpeedsMapToRisingRatesWithinRange() {
        let speeds: [Double] = [0.75, 1, 1.25, 1.5, 2, 3]
        let rates = speeds.map(ReadAloud.rate(for:))
        XCTAssertEqual(rates[1], AVSpeechUtteranceDefaultSpeechRate, accuracy: 0.0001)
        XCTAssertEqual(rates, rates.sorted())
        XCTAssertEqual(Set(rates).count, rates.count)
        XCTAssertLessThanOrEqual(rates.last!, AVSpeechUtteranceMaximumSpeechRate)
        XCTAssertGreaterThanOrEqual(rates.first!, AVSpeechUtteranceMinimumSpeechRate)
    }
}
