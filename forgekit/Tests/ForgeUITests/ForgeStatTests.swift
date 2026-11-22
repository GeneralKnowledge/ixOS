@testable import ForgeUI
import XCTest

/// What VoiceOver hears for a `ForgeStat`.
final class ForgeStatTests: XCTestCase {
    func testTheSpokenValuePrefersTheCallersWords() {
        XCTAssertEqual(ForgeStatSpeech.value("4.2 km", unit: nil, spokenValue: "4.2 kilometres"), "4.2 kilometres")
    }

    func testOtherwiseTheValueAndUnitAsShown() {
        XCTAssertEqual(ForgeStatSpeech.value("29", unit: "km/h", spokenValue: nil), "29 km/h")
        XCTAssertEqual(ForgeStatSpeech.value("12", unit: "", spokenValue: ""), "12")
        XCTAssertEqual(ForgeStatSpeech.value("12", unit: nil, spokenValue: nil), "12")
    }
}
