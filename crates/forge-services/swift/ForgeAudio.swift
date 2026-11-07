// Forge native services: recording and playing audio files.
// Source of truth: crates/forge-services/swift/ForgeAudio.swift.
//
// Optional: `forge build` / scripts/generate-bindings.sh copy this file only
// into apps that declare `microphone` under [ios.permissions] in forge.toml,
// so apps that never record do not carry microphone code.

import AVFoundation
import Foundation
import Observation

/// Audio for Forge apps: the microphone permission, a recorder, a player
/// and waveforms. Recordings are AAC in an `.m4a` file, mono, about 240 KB a
/// minute, which every Apple device plays.
///
/// Call `ForgeAudio.install()` once at launch, after
/// `ForgeServices.install()`, so Rust's `NativePermission::Microphone`
/// answers.
public enum ForgeAudio {
    /// Registers the microphone with `ForgePermissions`.
    public static func install() {
        ForgePermissions.shared.register(MicrophonePermissionAdapter(), for: .microphone)
    }

    /// The loudest sample of each of `bars` equal slices of the file, scaled
    /// so the loudest slice is 1. Decoded off the main actor, a chunk at a
    /// time.
    public static func waveform(of url: URL, bars: Int) async throws -> [Float] {
        try await Task.detached(priority: .userInitiated) {
            try peaks(of: url, bars: bars)
        }.value
    }

    /// The file's length in seconds.
    public static func duration(of url: URL) throws -> TimeInterval {
        let file = try AVAudioFile(forReading: url)
        let rate = file.processingFormat.sampleRate
        return rate > 0 ? Double(file.length) / rate : 0
    }

    static func peaks(of url: URL, bars: Int) throws -> [Float] {
        let file = try AVAudioFile(forReading: url)
        let total = file.length
        guard bars > 0, total > 0 else { return [] }
        let format = file.processingFormat
        let chunk: AVAudioFrameCount = 8192
        guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: chunk) else {
            throw ForgeAudioError.unreadable
        }
        var peaks = [Float](repeating: 0, count: bars)
        var frame: Int64 = 0
        let channels = Int(format.channelCount)
        while frame < total {
            try file.read(into: buffer, frameCount: chunk)
            let count = Int(buffer.frameLength)
            guard count > 0, let data = buffer.floatChannelData else { break }
            for index in 0..<count {
                // The file's length is an estimate for compressed formats:
                // clamp to the last bar.
                let bar = min(bars - 1, Int((frame + Int64(index)) * Int64(bars) / total))
                var sample: Float = 0
                for channel in 0..<channels {
                    sample = max(sample, abs(data[channel][index]))
                }
                if sample > peaks[bar] { peaks[bar] = sample }
            }
            frame += Int64(count)
        }
        let loudest = peaks.max() ?? 0
        return loudest > 0 ? peaks.map { $0 / loudest } : peaks
    }
}

/// Why recording or playing failed.
public enum ForgeAudioError: LocalizedError, Equatable {
    /// The user said no to the microphone (or it is restricted). Only the
    /// Settings app can change it: offer `ForgePermissions.openSettings()`.
    case microphoneDenied
    case couldNotRecord
    case couldNotPlay
    case unreadable

    public var errorDescription: String? {
        switch self {
        case .microphoneDenied:
            return String(localized: "Microphone access is off for this app. You can turn it on in Settings.")
        case .couldNotRecord: return String(localized: "Recording could not start.")
        case .couldNotPlay: return String(localized: "This audio could not be played.")
        case .unreadable: return String(localized: "This audio file could not be read.")
        }
    }
}

/// The app's audio session: what kind of audio it is making now. On iOS
/// this sets `AVAudioSession`; elsewhere it does nothing.
public enum ForgeAudioSession {
    public enum Use {
        /// Playing speech, even with the ring/silent switch on silent, and
        /// pausing other apps' audio.
        case playback
        /// Recording from the microphone, playing through the speaker.
        case record
    }

    public static func activate(_ use: Use) throws {
        #if os(iOS)
            let session = AVAudioSession.sharedInstance()
            switch use {
            case .playback:
                try session.setCategory(.playback, mode: .spokenAudio)
            case .record:
                try session.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker])
            }
            try session.setActive(true)
        #endif
    }

    /// Lets other apps' audio resume.
    public static func deactivate() {
        #if os(iOS)
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        #endif
    }
}

/// The microphone, for `ForgePermissions`.
struct MicrophonePermissionAdapter: ForgePermissionAdapter {
    func status() async -> PermissionStatus {
        switch AVAudioApplication.shared.recordPermission {
        case .undetermined: return .notDetermined
        case .denied: return .denied
        case .granted: return .granted
        @unknown default: return .unsupported
        }
    }

    func request() async -> PermissionStatus {
        if await status() == .notDetermined {
            _ = await AVAudioApplication.requestRecordPermission()
        }
        return await status()
    }
}

/// Records one audio file at a time from the microphone, with a live level
/// meter. Asks for the microphone on the first `start()`.
///
/// A recording ends when the app calls `stop()` or `cancel()`, or by itself
/// when it reaches `maxDuration` or the system interrupts it (a phone
/// call); then `onFinish` gets the recording.
@MainActor
@Observable
public final class ForgeAudioRecorder {
    /// A finished recording: an `.m4a` file in the temporary directory. Move
    /// or copy it somewhere lasting; the system may clear it.
    public struct Recording: Equatable, Sendable {
        public let url: URL
        public let duration: TimeInterval
        /// The meter readings taken while recording, 0 to 1, 20 a second.
        public let levels: [Float]
    }

    public private(set) var isRecording = false
    /// Seconds recorded so far.
    public private(set) var elapsed: TimeInterval = 0
    /// Meter readings so far, 0 to 1, 20 a second: a live waveform.
    public private(set) var levels: [Float] = []

    public let maxDuration: TimeInterval
    /// Shorter recordings are thrown away (a tap by mistake).
    public let minDuration: TimeInterval
    /// Called when a recording ends by itself (the limit or an
    /// interruption), with the recording or nil if it was too short.
    @ObservationIgnored public var onFinish: ((Recording?) -> Void)?

    @ObservationIgnored private var recorder: AVAudioRecorder?
    @ObservationIgnored private var ticker: Task<Void, Never>?

    public init(maxDuration: TimeInterval = 300, minDuration: TimeInterval = 0.5) {
        self.maxDuration = maxDuration
        self.minDuration = minDuration
    }

    /// Starts recording into a new file. Throws `microphoneDenied` if the
    /// user does not allow the microphone.
    public func start() async throws {
        guard !isRecording else { return }
        let permission = await MicrophonePermissionAdapter().request()
        guard permission == .granted else { throw ForgeAudioError.microphoneDenied }
        try ForgeAudioSession.activate(.record)
        do {
            let folder = FileManager.default.temporaryDirectory
                .appendingPathComponent("ForgeAudio", isDirectory: true)
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let url = folder.appendingPathComponent("Recording-\(UUID().uuidString).m4a")
            let settings: [String: Any] = [
                AVFormatIDKey: kAudioFormatMPEG4AAC,
                AVSampleRateKey: 24_000,
                AVNumberOfChannelsKey: 1,
                AVEncoderBitRateKey: 32_000,
                AVEncoderAudioQualityKey: AVAudioQuality.high.rawValue,
            ]
            let recorder = try AVAudioRecorder(url: url, settings: settings)
            recorder.isMeteringEnabled = true
            guard recorder.record(forDuration: maxDuration) else {
                throw ForgeAudioError.couldNotRecord
            }
            self.recorder = recorder
        } catch {
            ForgeAudioSession.deactivate()
            throw error
        }
        isRecording = true
        elapsed = 0
        levels = []
        ticker = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(50))
                guard let self, self.tick() else { return }
            }
        }
    }

    /// Ends the recording and returns it, or nil if it is shorter than
    /// `minDuration` (the file is then deleted).
    @discardableResult
    public func stop() -> Recording? {
        guard let recorder else { return nil }
        let recorded = max(elapsed, recorder.currentTime)
        recorder.stop()
        let url = recorder.url
        let levels = self.levels
        finish()
        let duration = (try? ForgeAudio.duration(of: url)) ?? recorded
        guard duration >= minDuration else {
            try? FileManager.default.removeItem(at: url)
            return nil
        }
        return Recording(url: url, duration: duration, levels: levels)
    }

    /// Ends the recording and deletes it.
    public func cancel() {
        guard let recorder else { return }
        recorder.stop()
        recorder.deleteRecording()
        finish()
    }

    /// One meter reading; false once the recording has ended.
    private func tick() -> Bool {
        guard let recorder else { return false }
        guard recorder.isRecording else {
            // The limit was reached, or the system interrupted us.
            onFinish?(stop())
            return false
        }
        recorder.updateMeters()
        let decibels = recorder.averagePower(forChannel: 0)
        levels.append(max(0, min(1, (decibels + 50) / 50)))
        elapsed = recorder.currentTime
        return true
    }

    private func finish() {
        ticker?.cancel()
        ticker = nil
        recorder = nil
        isRecording = false
        ForgeAudioSession.deactivate()
    }
}

/// Plays audio files, one at a time across the app (`shared`): starting one
/// stops the other. Pauses when the system interrupts it or the headphones
/// are unplugged; returns to the start when a file ends.
@MainActor
@Observable
public final class ForgeAudioPlayer {
    public static let shared = ForgeAudioPlayer()

    /// The file loaded now, playing or paused.
    public private(set) var current: URL?
    public private(set) var isPlaying = false
    /// Seconds into `current`.
    public private(set) var position: TimeInterval = 0
    /// Length of `current` in seconds.
    public private(set) var duration: TimeInterval = 0

    @ObservationIgnored private var player: AVAudioPlayer?
    @ObservationIgnored private let delegate = PlayerDelegate()
    @ObservationIgnored private var ticker: Task<Void, Never>?
    @ObservationIgnored private var routeObserver: NSObjectProtocol?

    public init() {
        delegate.finished = { [weak self] in
            Task { @MainActor in self?.stop() }
        }
        #if os(iOS)
            routeObserver = NotificationCenter.default.addObserver(
                forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main
            ) { [weak self] note in
                let reason = note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt
                guard reason == AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue else {
                    return
                }
                MainActor.assumeIsolated { self?.pause() }
            }
        #endif
    }

    /// How far `url` has played, 0 to 1 (0 unless it is `current`).
    public func progress(of url: URL) -> Double {
        guard current == url, duration > 0 else { return 0 }
        return min(1, position / duration)
    }

    /// Plays `url` from where it was paused, or from the start.
    public func play(_ url: URL) throws {
        if current != url || player == nil {
            stop()
            let player = try AVAudioPlayer(contentsOf: url)
            player.delegate = delegate
            player.prepareToPlay()
            self.player = player
            current = url
            duration = player.duration
            position = 0
        }
        try ForgeAudioSession.activate(.playback)
        guard let player, player.play() else {
            stop()
            throw ForgeAudioError.couldNotPlay
        }
        isPlaying = true
        startTicker()
    }

    /// Play if `url` is paused or another file is loaded; pause if it is
    /// playing.
    public func toggle(_ url: URL) throws {
        if current == url && isPlaying {
            pause()
        } else {
            try play(url)
        }
    }

    public func pause() {
        guard let player else { return }
        player.pause()
        position = player.currentTime
        isPlaying = false
        ticker?.cancel()
        ticker = nil
    }

    /// Moves `current` to `fraction` (0 to 1) of its length.
    public func seek(to fraction: Double) {
        guard let player else { return }
        player.currentTime = max(0, min(1, fraction)) * player.duration
        position = player.currentTime
    }

    /// Unloads the file and lets other apps' audio resume.
    public func stop() {
        let wasLoaded = player != nil
        player?.stop()
        player = nil
        current = nil
        isPlaying = false
        position = 0
        duration = 0
        ticker?.cancel()
        ticker = nil
        if wasLoaded { ForgeAudioSession.deactivate() }
    }

    private func startTicker() {
        ticker?.cancel()
        ticker = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(50))
                guard let self, let player = self.player else { return }
                self.position = player.currentTime
                if !player.isPlaying {
                    // Interrupted (a call): stay paused where it stopped.
                    self.isPlaying = false
                    return
                }
            }
        }
    }
}

/// AVAudioPlayer's delegate must be an NSObject.
private final class PlayerDelegate: NSObject, AVAudioPlayerDelegate {
    var finished: () -> Void = {}

    func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        finished()
    }
}
