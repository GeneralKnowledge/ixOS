#if os(iOS)
@preconcurrency import AVFoundation
@preconcurrency import SoundscapeFFI
import Foundation
import Observation

/// Plays a project's loops and records new ones, on AVAudioEngine.
///
/// Each track with a recording is an `AVAudioPlayerNode` looping one pass of
/// the loop, through an `AVAudioUnitEQ` low-pass band and an
/// `AVAudioUnitDelay`, into the main mixer. Every number comes from Rust's
/// `Track.mix`, in these units already, so the mixdown Rust renders sounds
/// the same. All players start at one host time, so the loops stay in step.
@Observable
@MainActor
final class LoopEngine {
    enum Failure: LocalizedError {
        case noMicrophone
        case noInput

        var errorDescription: String? {
            switch self {
            case .noMicrophone: "Soundscape can't use the microphone. Allow it in Settings, then try again."
            case .noInput: "There's no microphone to record from."
            }
        }
    }

    private struct Strip {
        let player = AVAudioPlayerNode()
        let tone = AVAudioUnitEQ(numberOfBands: 1)
        let echo = AVAudioUnitDelay()
        var clipPath: String?
        var buffer: AVAudioPCMBuffer?
    }

    @ObservationIgnored private let engine = AVAudioEngine()
    @ObservationIgnored private var strips: [UInt64: Strip] = [:]
    @ObservationIgnored private var loopSeconds: Double = 0
    @ObservationIgnored private var recorder: Recorder?
    private(set) var isPlaying = false
    private(set) var isRecording = false

    /// Makes the engine match `project`: a strip per track with a recording,
    /// each loaded with one pass of the loop and set to the track's mix.
    /// Strips whose recording didn't change keep their buffer.
    func load(_ project: ProjectDetail) {
        let wasPlaying = isPlaying
        let timingChanged = abs(project.loopSeconds - loopSeconds) > 0.0001
        loopSeconds = project.loopSeconds
        let wanted = Set(project.tracks.filter { $0.clip != nil }.map(\.id))
        for id in strips.keys where !wanted.contains(id) {
            detach(id)
        }
        var reloaded = timingChanged
        for track in project.tracks {
            guard let clip = track.clip else { continue }
            var strip = strips[track.id] ?? attach(track.id)
            if strip.clipPath != clip.path || timingChanged {
                strip.buffer = Self.loopBuffer(clip, seconds: project.loopSeconds)
                strip.clipPath = clip.path
                // A recording's sample rate decides the player's format.
                engine.disconnectNodeOutput(strip.player)
                if let buffer = strip.buffer {
                    engine.connect(strip.player, to: strip.tone, format: buffer.format)
                }
                reloaded = true
            }
            apply(track.mix, to: strip)
            strips[track.id] = strip
        }
        if wasPlaying && reloaded {
            stop()
            try? play()
        }
    }

    /// Sets one track's mix while it plays (after the mixer saved it).
    func apply(_ track: Track) {
        guard let strip = strips[track.id] else { return }
        apply(track.mix, to: strip)
    }

    /// Plays every loaded loop from the top, in step.
    func play() throws {
        try ForgeAudioSession.activate(.record)
        try startEngine()
        let start = Self.startTime()
        for strip in strips.values {
            guard let buffer = strip.buffer else { continue }
            strip.player.stop()
            strip.player.scheduleBuffer(buffer, at: nil, options: .loops, completionHandler: nil)
            strip.player.play(at: start)
        }
        isPlaying = true
    }

    func stop() {
        for strip in strips.values { strip.player.stop() }
        recorder?.cancel()
        recorder = nil
        isPlaying = false
    }

    /// Records exactly `frames` frames (one pass of the loop at the input's
    /// rate) while the other tracks play, and returns the WAV file. The
    /// input's and output's latency are skipped, so what you played along to
    /// lines up with the loop.
    func record(frames: (Double) -> Int?) async throws -> URL {
        guard await AVAudioApplication.requestRecordPermission() else { throw Failure.noMicrophone }
        try ForgeAudioSession.activate(.record)
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.channelCount > 0, format.sampleRate > 0, let length = frames(format.sampleRate) else {
            throw Failure.noInput
        }
        stop()
        try startEngine()
        let session = AVAudioSession.sharedInstance()
        let latency = session.inputLatency + session.outputLatency + session.ioBufferDuration
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("wav")
        let start = Self.startTime()
        let recorder = try Recorder(
            url: url, format: format, frames: length,
            fromHostTime: start.hostTime + AVAudioTime.hostTime(forSeconds: latency)
        )
        self.recorder = recorder
        input.installTap(onBus: 0, bufferSize: 4096, format: format, block: Self.tap(recorder))
        for strip in strips.values {
            guard let buffer = strip.buffer else { continue }
            strip.player.scheduleBuffer(buffer, at: nil, options: .loops, completionHandler: nil)
            strip.player.play(at: start)
        }
        isPlaying = true
        isRecording = true
        defer {
            input.removeTap(onBus: 0)
            isRecording = false
            stop()
        }
        return try await recorder.finished()
    }

    /// Made outside the main actor: the engine calls it on its own thread.
    nonisolated private static func tap(_ recorder: Recorder) -> AVAudioNodeTapBlock {
        { buffer, when in recorder.take(buffer, at: when) }
    }

    // MARK: Plumbing

    private func startEngine() throws {
        if !engine.isRunning {
            engine.prepare()
            try engine.start()
        }
    }

    private func attach(_ id: UInt64) -> Strip {
        let strip = Strip()
        let band = strip.tone.bands[0]
        band.filterType = .lowPass
        band.bandwidth = 0.5
        engine.attach(strip.player)
        engine.attach(strip.tone)
        engine.attach(strip.echo)
        engine.connect(strip.tone, to: strip.echo, format: nil)
        engine.connect(strip.echo, to: engine.mainMixerNode, format: nil)
        return strip
    }

    private func detach(_ id: UInt64) {
        guard let strip = strips.removeValue(forKey: id) else { return }
        strip.player.stop()
        engine.detach(strip.player)
        engine.detach(strip.tone)
        engine.detach(strip.echo)
    }

    private func apply(_ mix: TrackMix, to strip: Strip) {
        strip.player.volume = mix.volume
        strip.player.pan = mix.pan
        let band = strip.tone.bands[0]
        if let cutoff = mix.lowPassHz {
            band.frequency = cutoff
            band.bypass = false
        } else {
            band.bypass = true
        }
        if let seconds = mix.delaySeconds {
            strip.echo.bypass = false
            strip.echo.delayTime = seconds
            strip.echo.feedback = mix.delayFeedbackPercent
            strip.echo.wetDryMix = mix.delayWetDryPercent
        } else {
            strip.echo.bypass = true
        }
    }

    /// One pass of the loop: the recording, cut or followed by silence.
    private static func loopBuffer(_ clip: Clip, seconds: Double) -> AVAudioPCMBuffer? {
        guard let file = try? AVAudioFile(forReading: URL(fileURLWithPath: clip.path)) else { return nil }
        let format = file.processingFormat
        let frames = AVAudioFrameCount(max(1, (seconds * format.sampleRate).rounded()))
        guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames) else { return nil }
        let toRead = min(frames, AVAudioFrameCount(file.length))
        do {
            try file.read(into: buffer, frameCount: toRead)
        } catch {
            return nil
        }
        // AVAudioPCMBuffer's memory starts zeroed: the rest is silence.
        buffer.frameLength = frames
        return buffer
    }

    /// A moment just ahead, so every player can be scheduled before it.
    private static func startTime() -> AVAudioTime {
        AVAudioTime(hostTime: mach_absolute_time() + AVAudioTime.hostTime(forSeconds: 0.15))
    }
}

/// Writes the input to a WAV file from a host time until it has `frames`
/// frames. Fed on the audio engine's tap thread; finished on the caller's.
private final class Recorder: @unchecked Sendable {
    private let lock = NSLock()
    private var file: AVAudioFile?
    private let url: URL
    private let frames: Int
    private let fromHostTime: UInt64
    private var written = 0
    private var continuation: CheckedContinuation<URL, Error>?
    private var result: Result<URL, Error>?

    init(url: URL, format: AVAudioFormat, frames: Int, fromHostTime: UInt64) throws {
        self.url = url
        self.frames = frames
        self.fromHostTime = fromHostTime
        // 16-bit PCM, which Rust's WAV reader takes, at the input's rate.
        let settings: [String: Any] = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVSampleRateKey: format.sampleRate,
            AVNumberOfChannelsKey: format.channelCount,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsFloatKey: false,
            AVLinearPCMIsBigEndianKey: false,
        ]
        file = try AVAudioFile(
            forWriting: url, settings: settings,
            commonFormat: format.commonFormat, interleaved: format.isInterleaved
        )
    }

    /// One buffer from the tap: the part after the start time, up to the
    /// frames still wanted.
    func take(_ buffer: AVAudioPCMBuffer, at when: AVAudioTime) {
        lock.lock()
        defer { lock.unlock() }
        guard let file, result == nil else { return }
        let rate = buffer.format.sampleRate
        var skip = 0
        if when.isHostTimeValid {
            if when.hostTime < fromHostTime {
                let early = AVAudioTime.seconds(forHostTime: fromHostTime - when.hostTime)
                skip = Int((early * rate).rounded())
            }
        }
        let available = Int(buffer.frameLength)
        guard skip < available else { return }
        let count = min(available - skip, frames - written)
        guard count > 0, let part = Self.slice(buffer, from: skip, count: count) else { return }
        do {
            try file.write(from: part)
            written += count
            if written >= frames { finish(.success(url)) }
        } catch {
            finish(.failure(error))
        }
    }

    func cancel() {
        lock.lock()
        defer { lock.unlock() }
        finish(.failure(CancellationError()))
    }

    func finished() async throws -> URL {
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                lock.lock()
                if let result {
                    lock.unlock()
                    continuation.resume(with: result)
                } else {
                    self.continuation = continuation
                    lock.unlock()
                }
            }
        } onCancel: {
            cancel()
        }
    }

    /// Called with the lock held.
    private func finish(_ outcome: Result<URL, Error>) {
        guard result == nil else { return }
        // Releasing the file closes it, so it is complete on disk.
        file = nil
        if case .failure = outcome { try? FileManager.default.removeItem(at: url) }
        result = outcome
        continuation?.resume(with: outcome)
        continuation = nil
    }

    private static func slice(_ buffer: AVAudioPCMBuffer, from start: Int, count: Int) -> AVAudioPCMBuffer? {
        if start == 0 {
            buffer.frameLength = AVAudioFrameCount(count)
            return buffer
        }
        guard
            let part = AVAudioPCMBuffer(pcmFormat: buffer.format, frameCapacity: AVAudioFrameCount(count)),
            let from = buffer.floatChannelData, let to = part.floatChannelData
        else { return nil }
        let channels = Int(buffer.format.channelCount)
        let stride = buffer.stride
        for channel in 0..<(buffer.format.isInterleaved ? 1 : channels) {
            let width = buffer.format.isInterleaved ? channels : 1
            (to[channel]).update(from: from[channel].advanced(by: start * stride), count: count * width)
        }
        part.frameLength = AVAudioFrameCount(count)
        return part
    }
}
#endif
