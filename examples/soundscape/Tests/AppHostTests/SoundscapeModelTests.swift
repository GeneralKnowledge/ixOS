@testable import SoundscapeApp
@preconcurrency import SoundscapeFFI
import XCTest

/// The model against the real Rust core on a temporary folder, with a WAV
/// file written here standing in for a recording.
@MainActor
final class SoundscapeModelTests: XCTestCase {
    func testRecordMixAndMute() async throws {
        let host = makeTemporaryAppHost()
        await host.start()
        let core = try XCTUnwrap(host.core)
        let model = SoundscapeModel(core: core)
        XCTAssertTrue(model.projects.isEmpty)

        let project = try await model.create(title: "Morning", tempo: 120, bars: 1)
        let id = project.summary.id
        XCTAssertEqual(model.projects.count, 1)
        XCTAssertEqual(project.loopSeconds, 2, accuracy: 0.001)
        let track = try XCTUnwrap(project.tracks.first)

        // Nothing recorded: the mixdown has nothing to hear.
        let silent = await model.mixdown(id, passes: 1)
        XCTAssertNil(silent)
        XCTAssertNotNil(model.alert)
        model.alert = nil

        let frames = try XCTUnwrap(model.loopFrames(id, sampleRate: 8000))
        XCTAssertEqual(frames, 16000)
        let wav = try Self.tone(frames: frames, sampleRate: 8000)
        let recorded = try XCTUnwrap(await model.setClip(id, track: track.id, from: wav))
        XCTAssertNotNil(recorded.tracks.first?.clip)

        let peaks = await model.waveform(id, track: track.id, buckets: 16)
        XCTAssertEqual(peaks.count, 16)
        let mixdown = try XCTUnwrap(await model.mixdown(id, passes: 2))
        XCTAssertEqual(mixdown.durationMs, 4000)

        var muted = track.settings
        muted.muted = true
        let quiet = try XCTUnwrap(await model.setSettings(id, track: track.id, muted))
        XCTAssertEqual(quiet.tracks.first?.audible, false)
        XCTAssertEqual(quiet.tracks.first?.mix.volume, 0)
    }

    func testFormatting() {
        XCTAssertEqual(SoundFormat.gain(3.2), "+3 dB")
        XCTAssertEqual(SoundFormat.gain(-12), "−12 dB")
        XCTAssertEqual(SoundFormat.pan(-0.4), "L 40")
        XCTAssertEqual(SoundFormat.pan(0), "C")
        XCTAssertEqual(SoundFormat.duration(ms: 8000), "0:08.0")
        XCTAssertEqual(SoundFormat.timing(tempo: 120, bars: 1), "120 BPM · 1 bar")
    }

    /// A 16-bit mono WAV file of a quiet tone.
    private static func tone(frames: Int, sampleRate: Int) throws -> URL {
        var data = Data()
        func put<T: FixedWidthInteger>(_ value: T) {
            withUnsafeBytes(of: value.littleEndian) { data.append(contentsOf: $0) }
        }
        let bytes = frames * 2
        data.append(contentsOf: Array("RIFF".utf8)); put(UInt32(36 + bytes))
        data.append(contentsOf: Array("WAVE".utf8))
        data.append(contentsOf: Array("fmt ".utf8)); put(UInt32(16))
        put(UInt16(1)); put(UInt16(1)); put(UInt32(sampleRate)); put(UInt32(sampleRate * 2))
        put(UInt16(2)); put(UInt16(16))
        data.append(contentsOf: Array("data".utf8)); put(UInt32(bytes))
        for frame in 0..<frames {
            put(Int16(sin(Double(frame) / 8) * 8000))
        }
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("wav")
        try data.write(to: url)
        return url
    }
}
