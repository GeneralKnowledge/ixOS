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
    public private(set) var library: Library {
        didSet { collection = core.collection() }
    }
    /// Favourites, playlists and recently played, against the library now.
    public private(set) var collection: MusicCollection
    /// The songs chosen for the phone and how far along they are.
    public private(set) var offline: OfflineSummary {
        didSet {
            downloadStates = Dictionary(
                offline.tracks.map { ($0.track.id, $0.state) }, uniquingKeysWith: { a, _ in a }
            )
        }
    }
    private var downloadStates: [String: DownloadState] = [:]
    private var syncTask: Task<Void, Never>?
    private var syncAgain = false
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
        collection = core.collection()
        offline = core.offline()
        downloadStates = Dictionary(
            offline.tracks.map { ($0.track.id, $0.state) }, uniquingKeysWith: { a, _ in a }
        )
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
        do {
            library = try await withRustCancellation { cancellation in
                try await core.refreshLibrary(fresh: fresh, cancellation: cancellation)
            }
            refreshProblem = nil
            phase = .ready
            // A new pick may mean a new take to keep on the phone.
            syncDownloads()
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

    // MARK: Your collection

    public func isFavourite(_ id: String) -> Bool { collection.favouriteIds.contains(id) }

    /// A song, album or artist (by library id) becomes a favourite, or stops being one.
    public func toggleFavourite(_ id: String) async {
        do {
            collection = try await core.setFavourite(id: id, on: !isFavourite(id))
            feedback.play(.success)
        } catch {
            report(error)
        }
    }

    public func playlist(_ id: UInt64) -> PlaylistDetail? { try? core.playlist(id: id) }

    /// A new playlist, empty or with `tracks`.
    @discardableResult
    public func createPlaylist(_ name: String, with tracks: [String] = []) async -> PlaylistDetail? {
        await changePlaylists { try await self.core.createPlaylist(name: name, trackIds: tracks) }
    }

    public func renamePlaylist(_ id: UInt64, to name: String) async -> PlaylistDetail? {
        await changePlaylists { try await self.core.renamePlaylist(id: id, name: name) }
    }

    /// Adds songs (by track id) at the end of a playlist.
    @discardableResult
    public func add(_ tracks: [String], toPlaylist id: UInt64) async -> PlaylistDetail? {
        await changePlaylists { try await self.core.addToPlaylist(id: id, trackIds: tracks) }
    }

    /// Keeps the entries at `places`, in that order (what's left out goes).
    public func arrangePlaylist(_ id: UInt64, places: [UInt32]) async -> PlaylistDetail? {
        await changePlaylists { try await self.core.arrangePlaylist(id: id, places: places) }
    }

    public func deletePlaylist(_ id: UInt64) async {
        do {
            collection = try await core.deletePlaylist(id: id)
        } catch {
            report(error)
        }
    }

    /// Notes a song that played long enough to count (`TakePlayer.onPlayed`).
    public func recordPlay(trackId: String, recordingId: String) async {
        do {
            try await core.recordPlay(trackId: trackId, recordingId: recordingId)
            collection = core.collection()
        } catch {
            // History is a convenience: a play that couldn't be noted isn't worth an alert.
            log.warn("play not noted: \(error)")
        }
    }

    public func clearHistory() async {
        do {
            collection = try await core.clearHistory()
        } catch {
            report(error)
        }
    }

    private func changePlaylists(_ change: () async throws -> PlaylistDetail) async -> PlaylistDetail? {
        do {
            let detail = try await change()
            collection = core.collection()
            feedback.play(.success)
            return detail
        } catch {
            report(error)
            return nil
        }
    }

    // MARK: On the phone

    public func downloadState(_ trackId: String) -> DownloadState {
        downloadStates[trackId] ?? .notDownloaded
    }

    /// Songs that can be downloaded: your own files are on the phone already.
    public func downloadable(_ tracks: [LibraryTrack]) -> [LibraryTrack] {
        tracks.filter { $0.preferred != nil && $0.preferred?.kind != .imported }
    }

    /// True when every downloadable one of `tracks` is chosen for the phone.
    public func allChosenForPhone(_ tracks: [LibraryTrack]) -> Bool {
        let some = downloadable(tracks)
        return !some.isEmpty && some.allSatisfy { downloadStates[$0.id] != nil }
    }

    /// Chooses songs for the phone (or takes them off), then downloads or
    /// removes their files.
    public func keepOnPhone(_ tracks: [LibraryTrack], _ on: Bool) async {
        let ids = on ? downloadable(tracks).map(\.id) : tracks.map(\.id)
        guard !ids.isEmpty else { return }
        do {
            offline = try await core.setOffline(trackIds: ids, on: on)
            feedback.play(.success)
            syncDownloads()
        } catch {
            report(error)
        }
    }

    public func removeAllDownloads() async {
        do {
            offline = try await core.removeAllDownloads()
            library = core.library()
        } catch {
            report(error)
        }
    }

    /// Downloads what is chosen and removes what isn't, following along
    /// until it's done. A pass already running picks up new choices; a call
    /// that comes as it ends starts another.
    public func syncDownloads() {
        guard syncTask == nil else {
            syncAgain = true
            return
        }
        syncTask = Task { [weak self] in
            guard let self else { return }
            repeat {
                syncAgain = false
                let follow = Task { @MainActor [weak self] in
                    while !Task.isCancelled {
                        try? await Task.sleep(for: .seconds(1))
                        guard let self, !Task.isCancelled else { return }
                        let before = offline.downloaded
                        offline = core.offline()
                        if offline.downloaded != before { library = core.library() }
                    }
                }
                do {
                    offline = try await core.syncDownloads(cancellation: nil)
                } catch {
                    if !Self.isStop(error) { log.warn("downloads stopped: \(error)") }
                    offline = core.offline()
                }
                follow.cancel()
                library = core.library()
            } while syncAgain
            syncTask = nil
        }
    }

    // MARK: Your own files

    /// Copies picked music files into Soundscape (`MusicFile`s carry the
    /// tags the phone read). Returns what happened, for the summary.
    public func importMusic(_ files: [MusicFile]) async -> ImportResult? {
        do {
            let result = try await core.importMusic(files: files)
            library = core.library()
            if !result.added.isEmpty { feedback.play(.success) }
            return result
        } catch {
            report(error)
            return nil
        }
    }

    /// Takes an imported file out of the library and deletes Soundscape's copy.
    public func removeImported(_ trackId: String) async {
        do {
            library = try await core.removeImported(trackId: trackId)
            offline = core.offline()
        } catch {
            report(error)
        }
    }

    // MARK: Songs and takes

    /// One song's takes, or `nil` with the reason in `failure`.
    public func song(_ slug: String) async -> Result<SongDetail, ListenFailure> {
        do {
            let song = try await withRustCancellation { cancellation in
                try await core.listenSong(slug: slug, cancellation: cancellation)
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
extension Playlist: Identifiable {}
extension PlaylistEntry: Identifiable {
    public var id: UInt32 { place }
}

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

    /// "12 songs, 340 MB", with what is still coming or failed.
    public static func offlineSummary(_ summary: OfflineSummary) -> String {
        var text = "\(summary.downloaded) \(summary.downloaded == 1 ? "song" : "songs"), \(bytes(summary.bytes))"
        if summary.pending > 0 { text += " · \(summary.pending) to come" }
        if summary.failed > 0 { text += " · \(summary.failed) failed" }
        return text
    }

    /// "340 MB".
    public static func bytes(_ count: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: count), countStyle: .file)
    }

    /// "Added 3 songs. 1 was already in your library. 1 couldn't be imported: notes.txt: not a music file."
    public static func importSummary(_ result: ImportResult) -> String {
        var parts: [String] = []
        let added = result.added.count
        parts.append(added == 0 ? "No new songs." : "Added \(added) \(added == 1 ? "song" : "songs").")
        if result.already > 0 {
            parts.append("\(result.already) \(result.already == 1 ? "was" : "were") already in your library.")
        }
        if !result.refused.isEmpty {
            let count = result.refused.count
            parts.append("\(count) couldn't be imported: \(result.refused.joined(separator: "; ")).")
        }
        return parts.joined(separator: " ")
    }

    /// "12 songs, 48 minutes", "2 songs not in your library" added when some are gone.
    public static func playlistSummary(_ list: Playlist) -> String {
        var text = "\(list.trackCount) \(list.trackCount == 1 ? "song" : "songs")"
        let minutes = Int((list.durationSeconds / 60).rounded())
        if minutes > 0 { text += ", \(minutes) \(minutes == 1 ? "minute" : "minutes")" }
        if list.missingCount > 0 {
            text += " · \(list.missingCount) not in your library"
        }
        return text
    }

    /// "2 albums · 12 songs".
    public static func artistSummary(_ artist: LibraryArtist) -> String {
        let albums = "\(artist.albumCount) \(artist.albumCount == 1 ? "album" : "albums")"
        let songs = "\(artist.trackCount) \(artist.trackCount == 1 ? "song" : "songs")"
        return artist.albumCount > 0 ? "\(albums) · \(songs)" : songs
    }
}
