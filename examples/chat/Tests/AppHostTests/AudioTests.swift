import AVFoundation
@preconcurrency import ChatFFI
import XCTest

/// An AAC `.m4a` file of a 440 Hz tone, quiet until `loudFrom` seconds and
/// loud after: a stand-in for a recording (CI machines have no microphone).
func toneFile(seconds: Double, loudFrom: Double, in test: XCTestCase) throws -> URL {
    let url = URL(fileURLWithPath: try temporaryDirectory(test))
        .appendingPathComponent("tone-\(UUID().uuidString).m4a")
    let settings: [String: Any] = [
        AVFormatIDKey: kAudioFormatMPEG4AAC,
        AVSampleRateKey: 24_000,
        AVNumberOfChannelsKey: 1,
        AVEncoderBitRateKey: 32_000,
    ]
    // The file is finished when `file` is released, at the end of this scope.
    do {
        let file = try AVAudioFile(
            forWriting: url, settings: settings, commonFormat: .pcmFormatFloat32, interleaved: false)
        let format = file.processingFormat
        let frames = AVAudioFrameCount(seconds * format.sampleRate)
        let buffer = try XCTUnwrap(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames))
        buffer.frameLength = frames
        let samples = try XCTUnwrap(buffer.floatChannelData)[0]
        for index in 0..<Int(frames) {
            let time = Double(index) / format.sampleRate
            let amplitude: Float = time < loudFrom ? 0.05 : 0.8
            samples[index] = amplitude * Float(sin(2 * Double.pi * 440 * time))
        }
        try file.write(from: buffer)
    }
    return url
}

/// Forge's audio module (crates/forge-services/swift/ForgeAudio.swift),
/// which Chat gets because its forge.toml declares the microphone.
@MainActor
final class AudioTests: XCTestCase {
    func testDurationAndWaveformOfAnAudioFile() async throws {
        let url = try toneFile(seconds: 2, loudFrom: 1, in: self)
        XCTAssertEqual(try ForgeAudio.duration(of: url), 2, accuracy: 0.1)

        let bars = try await ForgeAudio.waveform(of: url, bars: 20)
        XCTAssertEqual(bars.count, 20)
        XCTAssertEqual(bars.max() ?? 0, 1, accuracy: 0.001, "scaled to the loudest slice")
        XCTAssertLessThan(bars[2...7].max() ?? 1, 0.3, "the quiet first second")
        XCTAssertGreaterThan(bars[12...17].min() ?? 0, 0.7, "the loud second second")

        XCTAssertThrowsError(try ForgeAudio.duration(of: url.appendingPathExtension("missing")))
    }

    func testTheMicrophoneIsAPermission() async {
        XCTAssertTrue(ForgeServices.install(keychainService: "chat-tests-\(UUID().uuidString)"))
        XCTAssertTrue(forgeSelectedServices.contains { $0.name == "Audio" }, "Chat selects audio")
        let status = await ForgePermissions.shared.status(permission: .microphone)
        XCTAssertNotEqual(status, .unsupported, "installing the audio service registers the microphone")
    }

    func testThePlayerPlaysOneFileAtATime() async throws {
        let first = try toneFile(seconds: 2, loudFrom: 0, in: self)
        let second = try toneFile(seconds: 1, loudFrom: 0, in: self)
        let player = ForgeAudioPlayer()
        defer { player.stop() }
        do {
            try player.play(first)
        } catch ForgeAudioError.couldNotPlay {
            throw XCTSkip("this machine has no audio output")
        }
        XCTAssertEqual(player.current, first)
        XCTAssertTrue(player.isPlaying)
        XCTAssertEqual(player.duration, 2, accuracy: 0.1)
        await waitUntil("playing moves forward") { player.position > 0.1 }
        XCTAssertGreaterThan(player.progress(of: first), 0)
        XCTAssertEqual(player.progress(of: second), 0)

        try player.toggle(second)
        XCTAssertEqual(player.current, second, "starting one stops the other")
        try player.toggle(second)
        XCTAssertFalse(player.isPlaying, "toggling the playing file pauses it")
        player.seek(to: 0.5)
        XCTAssertEqual(player.position, 0.5, accuracy: 0.1)
        player.stop()
        XCTAssertNil(player.current)
    }
}
