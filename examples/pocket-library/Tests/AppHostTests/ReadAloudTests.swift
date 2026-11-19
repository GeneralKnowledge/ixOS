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

    /// The lock screen's progress: where the current block starts in the
    /// chapter, and the chapter's length, at the reading speed.
    func testLockScreenTimesFollowTheChapter() {
        let texts = [String(repeating: "a", count: 150), "", String(repeating: "b", count: 300)]
        let start = ReadAloud.estimatedTimes(texts: texts, block: 0, speed: 1)
        XCTAssertEqual(start.elapsed, 0)
        XCTAssertEqual(start.duration, 30, accuracy: 0.001)
        let last = ReadAloud.estimatedTimes(texts: texts, block: 2, speed: 1)
        XCTAssertEqual(last.elapsed, 10, accuracy: 0.001)
        // Twice as fast: half the time.
        XCTAssertEqual(ReadAloud.estimatedTimes(texts: texts, block: 2, speed: 2).duration, 15, accuracy: 0.001)
        // Nothing loaded, or no block yet.
        XCTAssertEqual(ReadAloud.estimatedTimes(texts: [], block: nil, speed: 1).duration, 0)
        XCTAssertEqual(ReadAloud.estimatedTimes(texts: texts, block: nil, speed: 1).elapsed, 0)
    }

    /// A paused chapter shows where the voice stopped, not where its block
    /// began.
    func testLockScreenTimeIncludesTheSpokenPartOfTheBlock() {
        let texts = [String(repeating: "a", count: 150), "", String(repeating: "b", count: 300)]
        XCTAssertEqual(ReadAloud.estimatedTimes(texts: texts, block: 2, speed: 1, within: 75).elapsed, 15, accuracy: 0.001)
        // Never past the end.
        XCTAssertEqual(ReadAloud.estimatedTimes(texts: texts, block: 2, speed: 1, within: 9999).elapsed, 30, accuracy: 0.001)
    }

    /// The lock screen's progress bar and skip buttons: a time maps back to
    /// the block that holds it, by the same estimate as the duration.
    func testTimeMapsBackToTheBlockHoldingIt() {
        let texts = [String(repeating: "a", count: 150), "", String(repeating: "b", count: 300)]
        XCTAssertEqual(ReadAloud.block(at: 0, texts: texts, speed: 1), 0)
        XCTAssertEqual(ReadAloud.block(at: 9.9, texts: texts, speed: 1), 0)
        // The empty block is never chosen.
        XCTAssertEqual(ReadAloud.block(at: 10, texts: texts, speed: 1), 2)
        XCTAssertEqual(ReadAloud.block(at: 5, texts: texts, speed: 2), 2)
        // Before the start and past the end stay in the chapter.
        XCTAssertEqual(ReadAloud.block(at: -3, texts: texts, speed: 1), 0)
        XCTAssertEqual(ReadAloud.block(at: 500, texts: texts, speed: 1), 2)
        XCTAssertNil(ReadAloud.block(at: 1, texts: ["", ""], speed: 1))
        // Round trip: each block's start time maps to that block.
        for index in [0, 2] {
            let start = ReadAloud.estimatedTimes(texts: texts, block: index, speed: 2).elapsed
            XCTAssertEqual(ReadAloud.block(at: start, texts: texts, speed: 2), index)
        }
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
