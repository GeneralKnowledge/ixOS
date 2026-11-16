@preconcurrency import SoundscapeFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import Foundation
import Observation

private let log = ForgeLog("Listen")

/// What the Listen screens show: the take server's songs, one song's takes,
/// and the actions on them. The rules (the address, the requests, matched
/// loudness) are Soundscape's Rust core (`rust/src/listen.rs`); playback is
/// `TakePlayer`. Foundation and Observation only, so it compiles and tests
/// on Linux.
@Observable
@MainActor
public final class ListenModel {
    public enum Phase: Equatable {
        /// No server address yet.
        case needsServer
        case loading
        case ready
        case failed(String)
    }

    public enum Action: Equatable, Sendable {
        case forgetServer
    }

    /// The take server's address, as Rust saved it.
    public private(set) var server: String?
    public private(set) var songs: [SongSummary] = []
    public private(set) var phase: Phase = .needsServer
    public var alert: ForgeAlert<Action>?
    /// Increases after each pick or rating; views use it as a haptic trigger.
    public private(set) var markCount = 0

    public let core: SoundscapeCore

    public init(core: SoundscapeCore) {
        self.core = core
        server = core.listenServer()
        phase = server == nil ? .needsServer : .loading
    }

    // MARK: Server

    /// Saves the address the person typed; throws Rust's reason when it
    /// can't be used, so the form can say what to change.
    public func setServer(_ address: String) async throws {
        server = try await core.setListenServer(address: address)
        songs = []
        // The Listen screen reloads when the server changes.
        phase = server == nil ? .needsServer : .loading
    }

    public func askToForgetServer() {
        alert = .confirm(
            "Forget the take server?",
            message: "Your picks stay on the server. You can add it again with its address.",
            confirm: "Forget",
            action: .forgetServer,
            identifier: "listen.confirmForget",
            destructive: true
        )
    }

    public func handle(_ action: Action) async {
        switch action {
        case .forgetServer:
            do {
                try await setServer("")
            } catch {
                report(error)
            }
        }
    }

    // MARK: Songs

    /// Asks the server for its songs. `fresh` makes it look for new takes
    /// now (pull to refresh). Cancelling the calling task stops the request.
    public func loadSongs(fresh: Bool = false) async {
        guard server != nil else {
            phase = .needsServer
            return
        }
        if songs.isEmpty { phase = .loading }
        let cancellation = Cancellation()
        do {
            let loaded = try await withTaskCancellationHandler {
                try await core.listenSongs(fresh: fresh, cancellation: cancellation)
            } onCancel: {
                cancellation.cancel()
            }
            songs = loaded
            phase = .ready
        } catch {
            if Self.isStop(error) { return }
            log.warn("songs failed: \(error)")
            if songs.isEmpty {
                phase = .failed(Self.message(error))
            } else {
                alert = .failure(error)
            }
        }
    }

    /// One song's takes, or `nil` with the reason in `failure`.
    public func song(_ slug: String) async -> Result<SongDetail, ListenFailure> {
        let cancellation = Cancellation()
        do {
            let song = try await withTaskCancellationHandler {
                try await core.listenSong(slug: slug, cancellation: cancellation)
            } onCancel: {
                cancellation.cancel()
            }
            return .success(song)
        } catch {
            if !Self.isStop(error) { log.warn("song failed: \(error)") }
            return .failure(ListenFailure(message: Self.message(error), stopped: Self.isStop(error)))
        }
    }

    // MARK: Picks and ratings

    /// Picks `take` for its song (or clears the pick), on the server.
    public func pick(_ take: Take, in song: SongDetail, _ picked: Bool = true) async -> SongDetail? {
        await mark(take, in: song, TakeMark(picked: picked, stars: nil, note: nil))
    }

    /// 0 to 5 stars; tapping the take's current number clears them.
    public func rate(_ take: Take, in song: SongDetail, stars: Int) async -> SongDetail? {
        let value = stars == Int(take.stars) ? 0 : max(0, min(5, stars))
        return await mark(take, in: song, TakeMark(picked: nil, stars: UInt8(value), note: nil))
    }

    public func note(_ take: Take, in song: SongDetail, _ text: String) async -> SongDetail? {
        await mark(take, in: song, TakeMark(picked: nil, stars: nil, note: text))
    }

    private func mark(_ take: Take, in song: SongDetail, _ mark: TakeMark) async -> SongDetail? {
        do {
            let updated = try await core.markTake(slug: song.slug, takeId: take.id, mark: mark)
            markCount += 1
            // The list shows each song's pick and how many takes are rated.
            if let i = songs.firstIndex(where: { $0.slug == song.slug }) {
                let picked = updated.takes.first(where: \.picked)
                songs[i].pickLabel = picked?.label
                songs[i].ratedCount = UInt32(updated.takes.filter { $0.stars > 0 }.count)
            }
            return updated
        } catch {
            report(error)
            return nil
        }
    }

    // MARK: Comparing

    /// AVPlayer volumes for two takes at matched loudness (Rust decides).
    public func match(_ a: Take, _ b: Take) -> LoudnessMatch {
        loudnessMatch(aLufs: a.lufs, bLufs: b.lufs)
    }

    // MARK: Helpers

    public func report(_ error: Error) {
        log.warn("failed: \(error)")
        alert = .failure(error)
    }

    nonisolated static func isStop(_ error: Error) -> Bool {
        if case SoundscapeError.Stopped = error { return true }
        return error is CancellationError
    }

    nonisolated static func message(_ error: Error) -> String {
        if let error = error as? SoundscapeError { return SoundscapeModel.describe(error) }
        return error.localizedDescription
    }
}

public struct ListenFailure: Error, Equatable {
    public let message: String
    /// Cancelled because the screen closed: show nothing.
    public let stopped: Bool
}

extension SongSummary: Identifiable {
    public var id: String { slug }
}

extension Take: Identifiable {}

// MARK: Formatting (Foundation only, so it tests on Linux)

public enum TakeFormat {
    /// "92 BPM · D minor · xl-turbo", leaving out what the take doesn't say.
    public static func details(_ take: Take) -> String {
        var parts: [String] = []
        if let bpm = take.bpm { parts.append("\(Int(bpm.rounded())) BPM") }
        if let key = take.key { parts.append(key) }
        if let model = take.model, model != "turbo" { parts.append(model) }
        return parts.joined(separator: " · ")
    }

    /// "−14.2 LUFS", or nil before the server has measured it.
    public static func loudness(_ lufs: Double?) -> String? {
        guard let lufs else { return nil }
        let value = (lufs * 10).rounded() / 10
        return value < 0 ? "−\(String(format: "%.1f", -value)) LUFS" : "\(String(format: "%.1f", value)) LUFS"
    }

    /// What matching did: "B turned down 3.2 dB", "Same loudness", or that
    /// a take isn't measured yet.
    public static func match(_ match: LoudnessMatch, a: String = "A", b: String = "B") -> String {
        guard match.matched else { return "Not loudness-matched yet: the server is still measuring" }
        let db = abs(match.differenceDb)
        if db < 0.05 { return "Same loudness" }
        let louder = match.differenceDb > 0 ? a : b
        return "\(louder) turned down \(String(format: "%.1f", db)) dB to match"
    }

    /// "2:05".
    public static func time(_ seconds: Double) -> String {
        guard seconds.isFinite, seconds >= 0 else { return "0:00" }
        let s = Int(seconds.rounded(.down))
        return "\(s / 60):\(String(format: "%02d", s % 60))"
    }

    /// "3 takes · picked seed 7202".
    public static func summary(_ song: SongSummary) -> String {
        var text = "\(song.takeCount) \(song.takeCount == 1 ? "take" : "takes")"
        if let pick = song.pickLabel { text += " · picked \(pick)" } else if song.ratedCount > 0 { text += " · \(song.ratedCount) rated" }
        return text
    }
}
