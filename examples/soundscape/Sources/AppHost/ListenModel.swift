@preconcurrency import SoundscapeFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import Foundation
import Observation

private let log = ForgeLog("Listen")

/// What the Listen screens show: the music library (artists, albums,
/// tracks and their recordings), one song's takes to compare, and the
/// actions on them. The rules are Soundscape's Rust core
/// (`rust/src/library.rs`, `listen.rs`); playback is `TakePlayer`.
/// Foundation and Observation only, so it compiles and tests on Linux.
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
    /// The library as kept on the phone; refreshed from the take server.
    public private(set) var library: Library
    public private(set) var phase: Phase = .needsServer
    /// Why the last refresh failed while the kept library still shows.
    public var refreshProblem: String?
    public var alert: ForgeAlert<Action>?
    /// Success after each pick or rating.
    public private(set) var feedback = ForgeFeedback()

    public let core: SoundscapeCore

    public init(core: SoundscapeCore) {
        self.core = core
        server = core.listenServer()
        library = core.library()
        phase = server == nil ? .needsServer : library.tracks.isEmpty ? .loading : .ready
    }

    // MARK: Server

    /// Saves the address the person typed; throws Rust's reason when it
    /// can't be used, so the form can say what to change.
    public func setServer(_ address: String) async throws {
        server = try await core.setListenServer(address: address)
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

    // MARK: Library

    /// Asks the take server for its catalogue and keeps it. `fresh` makes
    /// it look for new takes now (pull to refresh). Without the server the
    /// kept library still shows, with the reason in `refreshProblem`.
    public func loadLibrary(fresh: Bool = false) async {
        guard server != nil else {
            phase = .needsServer
            return
        }
        if library.tracks.isEmpty { phase = .loading }
        let cancellation = Cancellation()
        do {
            library = try await withTaskCancellationHandler {
                try await core.refreshLibrary(fresh: fresh, cancellation: cancellation)
            } onCancel: {
                cancellation.cancel()
            }
            refreshProblem = nil
            phase = .ready
        } catch {
            if Self.isStop(error) { return }
            log.warn("library failed: \(error)")
            if library.tracks.isEmpty {
                phase = .failed(Self.message(error))
            } else {
                refreshProblem = Self.message(error)
                phase = .ready
            }
        }
    }

    public func search(_ query: String) -> LibrarySearch { core.searchLibrary(query: query) }
    public func albums(of artist: LibraryArtist) -> [LibraryAlbum] { core.artistAlbums(artistId: artist.id) }
    public func tracks(of artist: LibraryArtist) -> [LibraryTrack] { core.artistTracks(artistId: artist.id) }
    public func tracks(of album: LibraryAlbum) -> [LibraryTrack] { core.albumTracks(albumId: album.id) }

    /// A track with its recordings, or `nil` once it has left the library.
    public func track(_ id: String) -> TrackDetail? { try? core.libraryTrack(id: id) }

    /// What plays for `tracks`: each one's preferred recording, in order.
    public func queue(_ tracks: [LibraryTrack]) -> [QueueTrack] {
        core.queueTracks(trackIds: tracks.map(\.id))
    }

    /// One recording of a track, to play instead of the preferred one.
    public func queue(_ recording: Recording, of track: LibraryTrack) -> QueueTrack? {
        try? core.queueRecording(trackId: track.id, recordingId: recording.id)
    }

    /// A take from the takes screen, with the library's details for its song.
    public func queue(_ take: Take, of song: SongDetail) -> QueueTrack {
        core.takeQueueTrack(song: song, take: take)
    }

    /// Makes `recording` the one that plays for its track (the server's pick).
    public func choose(_ recording: Recording, of track: LibraryTrack) async -> TrackDetail? {
        do {
            let detail = try await core.chooseRecording(trackId: track.id, recordingId: recording.id)
            library = core.library()
            feedback.play(.success)
            return detail
        } catch {
            report(error)
            return nil
        }
    }

    /// Changes a track's details on the phone; empty fields go back to the
    /// server's.
    public func edit(_ track: LibraryTrack, _ edit: TrackEdit) async -> TrackDetail? {
        do {
            let detail = try await core.editTrack(id: track.id, edit: edit)
            library = core.library()
            return detail
        } catch {
            report(error)
            return nil
        }
    }

    /// Uses the picture file at `path` for an artist, album or track.
    public func setArtwork(_ target: ArtworkTarget, path: String) async {
        do {
            library = try await core.setArtwork(target: target, sourcePath: path)
        } catch {
            report(error)
        }
    }

    public func clearArtwork(_ target: ArtworkTarget) async {
        do {
            library = try await core.clearArtwork(target: target)
        } catch {
            report(error)
        }
    }

    // MARK: Songs and takes

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
            feedback.play(.success)
            // Rust brought the library's copy of the song up to date.
            library = core.library()
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

extension LibraryTrack: Identifiable {}
extension LibraryAlbum: Identifiable {}
extension LibraryArtist: Identifiable {}
extension Recording: Identifiable {}

extension Take: Identifiable {}

extension QueueEntry: Identifiable {
    public var id: UInt32 { position }
}


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

    /// "AFTER HOURS · 3:45".
    public static func subtitle(_ track: LibraryTrack) -> String {
        var parts = [track.artist]
        if let d = track.durationSeconds { parts.append(time(d)) }
        return parts.joined(separator: " · ")
    }

    /// "3 recordings", or nothing for one.
    public static func recordings(_ count: UInt32) -> String? {
        count > 1 ? "\(count) recordings" : nil
    }

    /// "Seed 7202 · 112 BPM · A major · xl-turbo".
    public static func generation(_ recording: Recording) -> String {
        guard let g = recording.generation else { return "" }
        var parts: [String] = []
        if let bpm = g.bpm { parts.append("\(Int(bpm.rounded())) BPM") }
        if let key = g.key { parts.append(key) }
        if let model = g.model, model != "turbo" { parts.append(model) }
        return parts.joined(separator: " · ")
    }

    /// "2 albums · 12 songs".
    public static func artistSummary(_ artist: LibraryArtist) -> String {
        let albums = "\(artist.albumCount) \(artist.albumCount == 1 ? "album" : "albums")"
        let songs = "\(artist.trackCount) \(artist.trackCount == 1 ? "song" : "songs")"
        return artist.albumCount > 0 ? "\(albums) · \(songs)" : songs
    }
}
