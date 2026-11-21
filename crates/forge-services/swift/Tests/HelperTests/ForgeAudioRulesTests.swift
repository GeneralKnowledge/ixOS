@testable import ForgeServicesHelpers
import XCTest

/// The rules `ForgeNowPlaying` follows for calls, Siri and headphones:
/// Soundscape's, as they passed on the iPhone (#306).
final class ForgeAudioRulesTests: XCTestCase {
    func testACallPausesAndHandsTheSoundBack() {
        var rules = ForgeAudioRules()
        XCTAssertEqual(rules.react(to: .interruptionBegan, playing: true), .pause)
        XCTAssertTrue(rules.interruptedWhilePlaying)
        // Paused by the interruption, so "playing" is false when it ends.
        XCTAssertEqual(rules.react(to: .interruptionEnded(shouldResume: true), playing: false), .play)
        XCTAssertFalse(rules.interruptedWhilePlaying)
    }

    func testAnotherAppsMusicDoesNotHandItBack() {
        var rules = ForgeAudioRules()
        XCTAssertEqual(rules.react(to: .interruptionBegan, playing: true), .pause)
        XCTAssertEqual(rules.react(to: .interruptionEnded(shouldResume: false), playing: false), .nothing)
    }

    func testSomethingPausedStaysPausedAfterACall() {
        var rules = ForgeAudioRules()
        XCTAssertEqual(rules.react(to: .interruptionBegan, playing: false), .nothing)
        XCTAssertEqual(rules.react(to: .interruptionEnded(shouldResume: true), playing: false), .nothing)
    }

    func testAnEndWithoutABeginningPlaysNothing() {
        var rules = ForgeAudioRules()
        XCTAssertEqual(rules.react(to: .interruptionEnded(shouldResume: true), playing: false), .nothing)
    }

    func testHeadphonesGoingPauseAndNeverResume() {
        var rules = ForgeAudioRules()
        XCTAssertEqual(rules.react(to: .outputGone, playing: true), .pause)
        XCTAssertEqual(rules.react(to: .outputGone, playing: false), .nothing)
        // A call after headphones went doesn't bring the sound back either.
        XCTAssertEqual(rules.react(to: .interruptionBegan, playing: false), .nothing)
        XCTAssertEqual(rules.react(to: .interruptionEnded(shouldResume: true), playing: false), .nothing)
    }
}
