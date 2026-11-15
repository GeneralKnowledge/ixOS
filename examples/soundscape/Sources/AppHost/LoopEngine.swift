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
///
/// The metronome is one more player, looping Rust's click track (one pass
/// of the loop, a tick on every beat) straight into the main mixer, so it
/// is never in a recording or the mixdown. Recording always starts with a
/// one-bar count-in: the click track's first bar, then the loop.
@Observable
@MainActor
final class LoopEngine {
    enum Failure: LocalizedError {
        case noMicrophone
        case noInput
        case noSound

        var errorDescription: String? {
            switch self {
            case .noMicrophone: "Soundscape can't use the microphone. Allow it in Settings, then try again."
            case .noInput: "There's no microphone to record from."
            case .noSound: "No sound came from the microphone, so nothing was kept. Try again."
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
    @ObservationIgnored private let clickPlayer = AVAudioPlayerNode()
    @ObservationIgnored private var clickPath: String?
    @ObservationIgnored private var clickBuffer: AVAudioPCMBuffer?
    @ObservationIgnored private var metronome = false
    @ObservationIgnored private var barSeconds: Double = 0
    private(set) var isPlaying = false
    private(set) var isRecording = false
    /// The bar of clicks before a recording starts.
    private(set) var isCountingIn = false

    init() {
        engine.attach(clickPlayer)
    }

    /// Makes the engine match `project`: a strip per track with a recording,
    /// each loaded with one pass of the loop and set to the track's mix.
    /// Strips whose recording didn't change keep their buffer. `click` is
    /// the project's click track (`SoundscapeModel.clickTrack`), or `nil`.
    func load(_ project: ProjectDetail, click: URL?) {
        let wasPlaying = isPlaying
        let timingChanged = abs(project.loopSeconds - loopSeconds) > 0.0001
        loopSeconds = project.loopSeconds
        barSeconds = 4 * 60 / project.summary.tempoBpm
        var reloaded = timingChanged || metronome != project.summary.metronome
        metronome = project.summary.metronome
        if click?.path != clickPath {
            clickPath = click?.path
            clickBuffer = click.flatMap(Self.wholeFile)
            engine.disconnectNodeOutput(clickPlayer)
            if let buffer = clickBuffer {
                engine.connect(clickPlayer, to: engine.mainMixerNode, format: buffer.format)
            }
            reloaded = true
        }
        let wanted = Set(project.tracks.filter { $0.clip != nil }.map(\.id))
        for id in strips.keys where !wanted.contains(id) {
            detach(id)
        }
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
        clickPlayer.stop()
        if metronome, let click = clickBuffer {
            clickPlayer.scheduleBuffer(click, at: nil, options: .loops, completionHandler: nil)
            clickPlayer.play(at: start)
        }
        isPlaying = true
    }

    func stop() {
        for strip in strips.values { strip.player.stop() }
        clickPlayer.stop()
        isCountingIn = false
        recorder?.cancel()
        recorder = nil
        isPlaying = false
    }

    /// Records exactly `frames` frames (one pass of the loop at the input's
    /// rate) while the other tracks play, and returns the WAV file. A bar of
    /// clicks counts in first. The input's and output's latency are skipped,
    /// so what you played along to lines up with the loop.
    func record(frames: (Double) -> Int?) async throws -> URL {
        guard await AVAudioApplication.requestRecordPermission() else { throw Failure.noMicrophone }
        try ForgeAudioSession.activate(.record)
        stop()
        // The microphone joins the graph only when the engine starts with
        // it: an engine already running for playback (Play, then Record)
        // never feeds the tap, and the recording would wait forever.
        if engine.isRunning { engine.stop() }
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.channelCount > 0, format.sampleRate > 0, let length = frames(format.sampleRate) else {
            throw Failure.noInput
        }
        try startEngine()
        let session = AVAudioSession.sharedInstance()
        let latency = session.inputLatency + session.outputLatency + session.ioBufferDuration
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("wav")
        let countInStart = Self.startTime()
        let countIn = clickBuffer.flatMap { Self.prefix($0, seconds: barSeconds) }
        let start = countIn == nil
            ? countInStart
            : AVAudioTime(hostTime: countInStart.hostTime + AVAudioTime.hostTime(forSeconds: barSeconds))
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
        if let countIn, let click = clickBuffer {
            // Played back to back: the count-in bar, then the loop's clicks.
            clickPlayer.scheduleBuffer(countIn, at: nil, options: [], completionHandler: nil)
            if metronome {
                clickPlayer.scheduleBuffer(click, at: nil, options: .loops, completionHandler: nil)
            }
            clickPlayer.play(at: countInStart)
            isCountingIn = true
            let wait = barSeconds + Self.lead
            Task { [weak self] in
                try? await Task.sleep(for: .seconds(wait))
                self?.isCountingIn = false
            }
        }
        isPlaying = true
        isRecording = true
        // If the microphone stops feeding the tap (the audio route changed,
        // a call came in), give up rather than wait forever.
        let deadline = (countIn == nil ? 0 : barSeconds) + loopSeconds + Self.lead + latency + 3
        let watchdog = Task { [recorder] in
            guard (try? await Task.sleep(for: .seconds(deadline))) != nil else { return }
            recorder.fail(Failure.noSound)
        }
        defer {
            watchdog.cancel()
            input.removeTap(onBus: 0)
            isRecording = false
            stop()
        }
        return try await recorder.finished()
    }

    /// Ends the recording now, keeping what was recorded so far.
    func finishRecording() {
        recorder?.keep()
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

    /// A whole file (the click track) in one buffer.
    private static func wholeFile(_ url: URL) -> AVAudioPCMBuffer? {
        guard let file = try? AVAudioFile(forReading: url),
              let buffer = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: AVAudioFrameCount(file.length))
        else { return nil }
        do {
            try file.read(into: buffer)
        } catch {
            return nil
        }
        return buffer
    }

    /// The first `seconds` of `buffer` (the count-in bar), copied.
    private static func prefix(_ buffer: AVAudioPCMBuffer, seconds: Double) -> AVAudioPCMBuffer? {
        let frames = min(buffer.frameLength, AVAudioFrameCount((seconds * buffer.format.sampleRate).rounded()))
        guard frames > 0,
              let part = AVAudioPCMBuffer(pcmFormat: buffer.format, frameCapacity: frames),
              let from = buffer.floatChannelData, let to = part.floatChannelData
        else { return nil }
        let interleaved = buffer.format.isInterleaved
        let channels = Int(buffer.format.channelCount)
        for channel in 0..<(interleaved ? 1 : channels) {
            to[channel].update(from: from[channel], count: Int(frames) * (interleaved ? channels : 1))
        }
        part.frameLength = frames
        return part
    }

    /// How far ahead playback is scheduled.
    private static let lead = 0.15

    /// A moment just ahead, so every player can be scheduled before it.
    private static func startTime() -> AVAudioTime {
        AVAudioTime(hostTime: mach_absolute_time() + AVAudioTime.hostTime(forSeconds: lead))
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
        let done = takeLocked(buffer, at: when)
        lock.unlock()
        // After the lock, and after `takeLocked` let go of the file, so the
        // WAV file is closed (its header written) before anyone reads it.
        done?()
    }

    /// Called with the lock held; returns the finishing step to run after.
    private func takeLocked(_ buffer: AVAudioPCMBuffer, at when: AVAudioTime) -> (() -> Void)? {
        guard let file, result == nil else { return nil }
        let rate = buffer.format.sampleRate
        var skip = 0
        if when.isHostTimeValid {
            if when.hostTime < fromHostTime {
                let early = AVAudioTime.seconds(forHostTime: fromHostTime - when.hostTime)
                skip = Int((early * rate).rounded())
            }
        }
        let available = Int(buffer.frameLength)
        guard skip < available else { return nil }
        let count = min(available - skip, frames - written)
        guard count > 0, let part = Self.slice(buffer, from: skip, count: count) else { return nil }
        do {
            try file.write(from: part)
            written += count
            if written >= frames { return finish(.success(url)) }
        } catch {
            return finish(.failure(error))
        }
        return nil
    }

    func cancel() {
        fail(CancellationError())
    }

    /// Stops early and keeps what was recorded (the loop plays silence
    /// after it), or cancels if nothing was yet.
    func keep() {
        lock.lock()
        let done = finish(written > 0 ? .success(url) : .failure(CancellationError()))
        lock.unlock()
        done?()
    }

    func fail(_ error: Error) {
        lock.lock()
        let done = finish(.failure(error))
        lock.unlock()
        done?()
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

    /// Called with the lock held. Drops the file (the last reference once
    /// the caller's is gone, which closes it) and returns the step that
    /// resumes the waiting caller, to run once the lock is released.
    private func finish(_ outcome: Result<URL, Error>) -> (() -> Void)? {
        guard result == nil else { return nil }
        file = nil
        result = outcome
        let continuation = self.continuation
        self.continuation = nil
        let url = self.url
        return {
            if case .failure = outcome { try? FileManager.default.removeItem(at: url) }
            continuation?.resume(with: outcome)
        }
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
