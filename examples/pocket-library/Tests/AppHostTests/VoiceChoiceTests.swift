@testable import PocketLibraryApp
import Foundation
import XCTest

/// Choosing the read-aloud voice. Plain Swift, so it also runs on Linux.
final class VoiceChoiceTests: XCTestCase {
    private func voice(
        _ name: String, _ language: String, _ quality: VoiceCandidate.Quality = .standard, personal: Bool = false
    ) -> VoiceCandidate {
        VoiceCandidate(id: "\(name).\(language)", name: name, language: language, quality: quality, isPersonal: personal)
    }

    func testBestPrefersPremiumThenEnhancedOverTheStandardVoice() {
        let daniel = voice("Daniel", "en-GB")
        let ava = voice("Ava", "en-US", .enhanced)
        let zoe = voice("Zoe", "en-US", .premium)
        XCTAssertEqual(VoiceChoice.best([daniel, ava], language: "en-GB"), ava)
        XCTAssertEqual(VoiceChoice.best([daniel, ava, zoe], language: "en-GB"), zoe)
        XCTAssertEqual(VoiceChoice.best([daniel], language: "en-GB"), daniel)
    }

    func testSameQualityPrefersTheDevicesRegion() {
        let ava = voice("Ava", "en-US", .enhanced)
        let serena = voice("Serena", "en-GB", .enhanced)
        XCTAssertEqual(VoiceChoice.best([ava, serena], language: "en-GB"), serena)
        XCTAssertEqual(VoiceChoice.best([ava, serena], language: "en_US"), ava)
    }

    func testOtherLanguagesAreLeftOutAndEnglishIsTheFallback() {
        let thomas = voice("Thomas", "fr-FR", .premium)
        let daniel = voice("Daniel", "en-GB")
        XCTAssertEqual(VoiceChoice.ranked([thomas, daniel], language: "en-GB"), [daniel])
        XCTAssertEqual(VoiceChoice.ranked([thomas, daniel], language: "fr-CA"), [thomas])
        XCTAssertEqual(VoiceChoice.ranked([thomas, daniel], language: "ja-JP"), [daniel])
    }

    func testPersonalVoiceIsListedFirstButNeverPickedForTheUser() {
        let mine = voice("Reuben", "en-GB", personal: true)
        let zoe = voice("Zoe", "en-US", .premium)
        XCTAssertEqual(VoiceChoice.ranked([zoe, mine], language: "en-GB"), [mine, zoe])
        XCTAssertEqual(VoiceChoice.best([zoe, mine], language: "en-GB"), zoe)
        XCTAssertNil(VoiceChoice.best([mine], language: "en-GB"))
    }

    func testDownloadHintOnlyWhenEveryVoiceIsStandard() {
        let daniel = voice("Daniel", "en-GB")
        XCTAssertTrue(VoiceChoice.onlyStandard([daniel], language: "en-GB"))
        XCTAssertTrue(VoiceChoice.onlyStandard([daniel, voice("Me", "en-GB", personal: true)], language: "en-GB"))
        XCTAssertFalse(VoiceChoice.onlyStandard([daniel, voice("Ava", "en-US", .enhanced)], language: "en-GB"))
    }

    func testLabelNamesTheQuality() {
        let premium = VoiceChoice.label(for: voice("Ava", "en-US", .premium))
        XCTAssertTrue(premium.hasPrefix("Ava (Premium, "), premium)
        let personal = VoiceChoice.label(for: voice("Me", "en-GB", personal: true))
        XCTAssertTrue(personal.hasPrefix("Me (Personal Voice, "), personal)
        XCTAssertFalse(VoiceChoice.label(for: voice("Daniel", "en-GB")).contains("Premium"))
    }
}
