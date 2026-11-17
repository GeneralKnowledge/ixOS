@preconcurrency import SoundscapeFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import Foundation
import Observation

private let log = ForgeLog("Soundscape")

/// What the screens show and the actions they take. It holds no rules:
/// limits, the mix in AVAudioEngine's units and the mixdown are
/// Soundscape's Rust core (`rust/src/lib.rs`); this model asks it and shows
/// what it says. Foundation and Observation only, so it compiles and tests
/// on Linux.
@Observable
@MainActor
public final class SoundscapeModel {
    /// Every project, most recently changed first, as Rust last reported it.
    public private(set) var projects: [ProjectSummary] = []

    /// Alerts the screens ask for: delete after confirming, or a failure.
    public enum Action: Equatable, Sendable {
        case deleteProject(UInt64)
        case removeTrack(project: UInt64, track: UInt64)
        case clearClip(project: UInt64, track: UInt64)
    }

    public var alert: ForgeAlert<Action>?
    /// Success after each saved change.
    public private(set) var feedback = ForgeFeedback()
    /// Whether a mixdown is running, and how to stop it.
    public private(set) var isMixing = false
    private var mixing: Cancellation?

    public let core: SoundscapeCore

    public init(core: SoundscapeCore) {
        self.core = core
        reload()
    }

    /// Re-reads Rust's published projects. Cheap: no command is waited for.
    public func reload() {
        projects = core.projects()
    }

    /// A project with its tracks and their mix, or `nil` once deleted.
    public func detail(_ id: UInt64) -> ProjectDetail? {
        try? core.project(id: id)
    }

    /// How many frames to record at `sampleRate` for one pass of the loop.
    public func loopFrames(_ id: UInt64, sampleRate: Double) -> Int? {
        guard sampleRate > 0, let frames = try? core.loopFrames(id: id, sampleRate: UInt32(sampleRate.rounded())) else {
            return nil
        }
        return Int(frames)
    }

    // MARK: Projects

    /// A new project with one empty track; throws so the sheet can stay
    /// open and say what to change.
    public func create(title: String, tempo: Double, bars: Int) async throws -> ProjectDetail {
        let detail = try await core.createProject(title: title, tempoBpm: tempo, bars: UInt32(max(1, bars)))
        reload()
        feedback.play(.success)
        return detail
    }

    public func rename(_ id: UInt64, to title: String) async throws {
        try await core.renameProject(id: id, title: title)
        reload()
    }

    public func setTiming(_ id: UInt64, tempo: Double, bars: Int) async throws -> ProjectDetail {
        let detail = try await core.setTiming(id: id, tempoBpm: tempo, bars: UInt32(max(1, bars)))
        reload()
        return detail
    }

    public func askToDelete(_ project: ProjectSummary) {
        alert = .confirm(
            "Delete “\(project.title)”?",
            message: "Its tracks and recordings are deleted too. This can't be undone.",
            confirm: "Delete Project",
            action: .deleteProject(project.id),
            identifier: "projects.confirmDelete",
            destructive: true
        )
    }

    public func askToRemove(_ track: Track, from project: UInt64) {
        alert = .confirm(
            "Remove “\(track.name)”?",
            message: track.clip == nil ? nil : "Its recording is deleted too.",
            confirm: "Remove Track",
            action: .removeTrack(project: project, track: track.id),
            identifier: "tracks.confirmRemove",
            destructive: true
        )
    }

    public func askToClear(_ track: Track, in project: UInt64) {
        alert = .confirm(
            "Delete the recording on “\(track.name)”?",
            message: "The track stays, ready to record again.",
            confirm: "Delete Recording",
            action: .clearClip(project: project, track: track.id),
            identifier: "tracks.confirmClear",
            destructive: true
        )
    }

    /// Runs a confirmed action; returns the project after a track change.
    @discardableResult
    public func handle(_ action: Action) async -> ProjectDetail? {
        switch action {
        case .deleteProject(let id):
            _ = await perform { core in try await core.deleteProject(id: id) }
            return nil
        case .removeTrack(let project, let track):
            return await perform { core in try await core.removeTrack(id: project, trackId: track) }
        case .clearClip(let project, let track):
            return await perform { core in try await core.clearClip(id: project, trackId: track) }
        }
    }

    // MARK: Tracks

    public func addTrack(to project: UInt64) async -> ProjectDetail? {
        await perform { core in try await core.addTrack(id: project, name: nil) }
    }

    public func renameTrack(_ project: UInt64, track: UInt64, to name: String) async throws -> ProjectDetail {
        let detail = try await core.renameTrack(id: project, trackId: track, name: name)
        reload()
        return detail
    }

    /// Turns the project's metronome on or off.
    public func setMetronome(_ project: UInt64, on: Bool) async -> ProjectDetail? {
        await perform(haptic: false) { core in
            try await core.setMetronome(id: project, on: on)
        }
    }

    /// One pass of the project's metronome as a WAV file at `sampleRate`,
    /// or `nil` (logged: the loop still plays, just without clicks).
    public func clickTrack(_ project: UInt64, sampleRate: Double) async -> URL? {
        do {
            let path = try await core.clickTrack(id: project, sampleRate: UInt32(sampleRate.rounded()))
            return URL(fileURLWithPath: path)
        } catch {
            log.warn("click track failed: \(error)")
            return nil
        }
    }

    /// The whole mixer strip at once; Rust checks it and answers with the
    /// track's mix in AVAudioEngine's units.
    public func setSettings(_ project: UInt64, track: UInt64, _ settings: TrackSettings) async -> ProjectDetail? {
        await perform(haptic: false) { core in
            try await core.setTrackSettings(id: project, trackId: track, settings: settings)
        }
    }

    /// Makes the recording at `url` the track's loop. Rust copies it, so
    /// the caller deletes its own file afterwards.
    public func setClip(_ project: UInt64, track: UInt64, from url: URL) async -> ProjectDetail? {
        await perform { core in
            try await core.setClip(id: project, trackId: track, sourcePath: url.path)
        }
    }

    /// A track's waveform as `buckets` peaks from 0 to 1. Cancelling the
    /// calling task (the row scrolled away) stops it in Rust.
    public func waveform(_ project: UInt64, track: UInt64, buckets: Int) async -> [Float] {
        let cancellation = Cancellation()
        do {
            return try await withTaskCancellationHandler {
                try await core.waveform(id: project, trackId: track, buckets: UInt32(buckets), cancellation: cancellation)
            } onCancel: {
                cancellation.cancel()
            }
        } catch {
            if !Self.isStop(error) { log.warn("waveform failed: \(error)") }
            return []
        }
    }

    /// Mixes the audible tracks into a WAV file to share; `nil` (and an
    /// alert, unless stopped) if it failed.
    public func mixdown(_ project: UInt64, passes: Int) async -> Mixdown? {
        guard !isMixing else { return nil }
        let cancellation = Cancellation()
        mixing = cancellation
        isMixing = true
        defer {
            isMixing = false
            mixing = nil
        }
        do {
            return try await core.mixdown(id: project, passes: UInt32(passes), cancellation: cancellation)
        } catch {
            if !Self.isStop(error) { report(error) }
            return nil
        }
    }

    public func stopMixdown() {
        mixing?.cancel()
    }

    /// Reports a failure the views ran into themselves (recording, audio).
    public func report(_ error: Error) {
        log.warn("failed: \(error)")
        alert = .failure(error)
    }

    private static func isStop(_ error: Error) -> Bool {
        if case SoundscapeError.Stopped = error { return true }
        return error is CancellationError
    }

    private func perform<T>(
        haptic: Bool = true,
        _ command: (SoundscapeCore) async throws -> T
    ) async -> T? {
        do {
            let value = try await command(core)
            reload()
            if haptic { feedback.play(.success) }
            return value
        } catch {
            log.warn("command failed: \(error)")
            alert = .failure(error)
            return nil
        }
    }

    // MARK: Words for the screen

    nonisolated public static func describe(_ error: SoundscapeError) -> String {
        switch error {
        case .EmptyName: return "Give it a name."
        case .NameTooLong(let max): return "Names can be up to \(max) characters."
        case .NotFound: return "That project or track no longer exists."
        case .TooManyTracks(let max): return "A project can have up to \(max) tracks."
        case .OutOfRange: return "That setting is out of range."
        case .Audio: return "That recording can't be used. Try recording it again."
        case .Silent: return "There's nothing to hear yet. Record a track, or unmute one."
        case .InUse: return "Soundscape is open in another copy of the app."
        case .Corrupt: return "Your projects couldn't be read."
        case .NewerData: return "Your projects were saved by a newer version of Soundscape."
        case .Storage: return "Couldn't save. Try again."
        case .BadAddress(let reason): return Self.sentence(reason)
        case .Unreachable(let reason):
            // The system's own reason goes last, so a cause on the phone
            // (such as App Transport Security) is visible, not just a guess.
            return "Can't reach the take server. Check that it's running on the computer and that this phone is on Tailscale (or the same network)."
                + (reason.isEmpty ? "" : "\n\n" + Self.sentence(reason))
        case .Server(let reason): return Self.sentence(reason)
        case .NotInLibrary: return "That isn't in your library any more. Pull down to refresh it."
        case .Stopped: return "Stopped."
        }
    }

    /// Rust's reasons start lower case; the screen shows a sentence.
    nonisolated static func sentence(_ text: String) -> String {
        guard let first = text.first else { return text }
        let capital = first.uppercased() + text.dropFirst()
        return capital.hasSuffix(".") || capital.hasSuffix("?") ? capital : capital + "."
    }

    nonisolated public static func title(_ error: SoundscapeError) -> String {
        switch error {
        case .EmptyName, .NameTooLong, .OutOfRange: return "Check the Details"
        case .NotFound: return "Not Found"
        case .TooManyTracks: return "Can't Add a Track"
        case .Audio: return "Recording Not Used"
        case .Silent: return "Nothing to Export"
        case .InUse: return "Already Open"
        case .Corrupt, .NewerData, .Storage: return "Couldn't Save"
        case .BadAddress: return "Check the Address"
        case .Unreachable: return "Can't Reach the Server"
        case .Server: return "The Server Said No"
        case .NotInLibrary: return "Not in the Library"
        case .Stopped: return "Stopped"
        }
    }
}

extension SoundscapeError: ForgeDescribedError {
    public var failure: ForgeFailure {
        let retryable: Bool
        switch self {
        case .Storage, .Unreachable: retryable = true
        default: retryable = false
        }
        return ForgeFailure(
            title: SoundscapeModel.title(self),
            message: SoundscapeModel.describe(self),
            isRetryable: retryable
        )
    }

    public var isCancellation: Bool {
        if case .Stopped = self { return true }
        return false
    }
}

extension ProjectSummary: Identifiable {}
extension Track: Identifiable {}

// MARK: Formatting (Foundation only, so it tests on Linux)

public enum SoundFormat {
    /// The name a new project starts with: "Loop, 14 Nov".
    public static func defaultTitle(now: Date = Date()) -> String {
        "Loop, \(now.formatted(.dateTime.day().month(.abbreviated)))"
    }

    public static func date(_ ms: Int64) -> Date {
        Date(timeIntervalSince1970: TimeInterval(ms) / 1000)
    }

    /// "120 BPM · 4 bars".
    public static func timing(tempo: Double, bars: UInt32) -> String {
        "\(Int(tempo.rounded())) BPM · \(bars) \(bars == 1 ? "bar" : "bars")"
    }

    /// "+3 dB", "0 dB", "−12 dB".
    public static func gain(_ db: Double) -> String {
        let rounded = Int(db.rounded())
        if rounded > 0 { return "+\(rounded) dB" }
        if rounded < 0 { return "−\(-rounded) dB" }
        return "0 dB"
    }

    /// "L 40", "C", "R 25".
    public static func pan(_ pan: Double) -> String {
        let percent = Int((pan * 100).rounded())
        if percent < 0 { return "L \(-percent)" }
        if percent > 0 { return "R \(percent)" }
        return "C"
    }

    /// "0:08.0".
    public static func duration(ms: Int64) -> String {
        let tenths = (ms + 50) / 100
        let seconds = tenths / 10
        return "\(seconds / 60):\(String(format: "%02d", seconds % 60)).\(tenths % 10)"
    }

    /// "−0.4 dBFS".
    public static func peak(_ db: Double) -> String {
        let value = (db * 10).rounded() / 10
        return value < 0 ? "−\(String(format: "%.1f", -value)) dBFS" : "\(String(format: "%.1f", value)) dBFS"
    }

    /// The echo the Echo switch turns on: an eighth note, a few repeats.
    public static let defaultEcho = Echo(beats: 0.5, feedback: 0.35, mix: 0.3)
}
