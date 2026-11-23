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
        case loading
        case ready
        case failed(String)
    }

    public enum Action: Equatable, Sendable {
        case forgetServer
    }

    /// What Undo on the banner takes back.
    public enum Undo: Equatable, Sendable {
        /// A song taken out of the queue: its index in the queue's tracks
        /// and where it was in the play order. The player puts it back.
        case queue(index: UInt32, position: UInt32)
        /// A playlist's songs before a removal.
        case playlistSongs(id: UInt64, trackIds: [String])
        case deletedPlaylist(UInt64)
        case removedSong(String)
    }

    /// The take server's address, as Rust saved it. Optional: the library
    /// also holds your own files and catalogue tracks.
    public private(set) var server: String?
    /// The person's Jamendo client ID, as Rust saved it.
    public private(set) var jamendoClientId: String?
    /// Seconds songs overlap as one ends and the next starts; 0 is gapless.
    public private(set) var crossfadeSeconds: UInt32
    /// The library as kept on the phone; refreshed from the take server.
    public private(set) var library: Library {
        didSet {
            collection = core.collection()
            duplicates = core.duplicateSongs()
        }
    }
    /// Songs that look like one song from several sources, offered for
    /// merging.
    public private(set) var duplicates: [DuplicateSongs]
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
    public private(set) var phase: Phase = .ready
    /// Why the last refresh failed while the kept library still shows.
    public var refreshProblem: String?
    public var alert: ForgeAlert<Action>?
    /// "Removed …" with Undo, at the bottom of the tab.
    public var banner: ForgeBanner<Undo>?
    /// Success after each pick or rating.
    public private(set) var feedback = ForgeFeedback()

    public let core: SoundscapeCore

    public init(core: SoundscapeCore) {
        self.core = core
        server = core.listenServer()
        jamendoClientId = core.jamendoClientId()
        crossfadeSeconds = core.crossfadeSeconds()
        library = core.library()
        collection = core.collection()
        duplicates = core.duplicateSongs()
        offline = core.offline()
        downloadStates = Dictionary(
            offline.tracks.map { ($0.track.id, $0.state) }, uniquingKeysWith: { a, _ in a }
        )
        phase = server != nil && library.tracks.isEmpty ? .loading : .ready
    }

    // MARK: Server

    /// Saves the address the person typed; throws Rust's reason when it
    /// can't be used, so the form can say what to change.
    public func setServer(_ address: String) async throws {
        server = try await core.setListenServer(address: address)
        // The Listen screen reloads when the server changes.
        phase = server == nil ? .ready : .loading
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

    // MARK: The background

    /// How many times the app has come back from the background, for
    /// screens that start again work the background stopped.
    public private(set) var resumes = 0

    /// Follows the app's phases through `host`, for as long as this model
    /// lives: each return from the background reloads the library, so picks
    /// and stars made on the computer while the phone was away show
    /// without a pull to refresh.
    public func follow(_ host: ForgeHost<SoundscapeCore>) {
        host.onResume(self) { [weak self] _ in
            guard let self else { return }
            self.resumes += 1
            Task { await self.loadLibrary() }
        }
    }

    // MARK: Library

    /// Asks the take server for its catalogue and keeps it. `fresh` makes
    /// it look for new takes now (pull to refresh). Without the server the
    /// kept library still shows, with the reason in `refreshProblem`.
    public func loadLibrary(fresh: Bool = false) async {
        guard server != nil else {
            library = core.library()
            refreshProblem = nil
            phase = .ready
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

    /// Remembers a search for Search's Recent list.
    public func noteSearch(_ query: String) async {
        guard !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        if let found = try? await core.noteSearch(query: query) { collection = found }
    }

    public func clearRecentSearches() async {
        do {
            collection = try await core.clearRecentSearches()
        } catch {
            report(error)
        }
    }

    // MARK: What was playing

    /// Keeps what is playing so the app opens where it was left; `nil`
    /// forgets it. A failure only costs the resume, so it is logged.
    public func savePlayback(_ saved: SavedPlayback?) async {
        do {
            try await core.savePlayback(saved: saved)
        } catch {
            log.warn("playback not kept: \(error)")
        }
    }

    /// What was playing when the app closed, brought up to date.
    public func savedPlayback() -> SavedPlayback? { core.savedPlayback() }

    /// Saves the crossfade (0 is gapless); a failure shows in an alert.
    public func setCrossfade(_ seconds: UInt32) async {
        do {
            crossfadeSeconds = try await core.setCrossfadeSeconds(seconds: seconds)
        } catch {
            report(error)
        }
    }

    // MARK: Catalogues

    /// Saves the Jamendo client ID; throws Rust's reason when it can't be
    /// used. Empty forgets it.
    public func setJamendoClientId(_ text: String) async throws {
        jamendoClientId = try await core.setJamendoClientId(clientId: text)
    }

    /// Jamendo tracks for `query` (this week's popular when empty).
    /// Cancelled with the screen that asked.
    public func searchJamendo(_ query: String) async -> Result<[CatalogueTrack], ListenFailure> {
        do {
            let found = try await withRustCancellation { cancellation in
                try await core.searchJamendo(query: query, cancellation: cancellation)
            }
            return .success(found)
        } catch {
            if !Self.isStop(error) { log.warn("jamendo search failed: \(error)") }
            return .failure(ListenFailure(message: Self.message(error), stopped: Self.isStop(error)))
        }
    }

    /// Internet Archive items (albums, concerts) for `query`, or this
    /// week's most downloaded net-label releases when empty.
    public func searchArchive(_ query: String) async -> Result<[ArchiveItem], ListenFailure> {
        do {
            let found = try await withRustCancellation { cancellation in
                try await core.searchArchive(query: query, cancellation: cancellation)
            }
            return .success(found)
        } catch {
            if !Self.isStop(error) { log.warn("archive search failed: \(error)") }
            return .failure(ListenFailure(message: Self.message(error), stopped: Self.isStop(error)))
        }
    }

    /// One search for everything, the catalogues' part: Jamendo and the
    /// Internet Archive together, each with its results or its problem.
    /// Cancelled with the screen, or when the search changes.
    public func searchCatalogues(_ query: String) async -> Result<CatalogueSearch, ListenFailure> {
        do {
            let found = try await withRustCancellation { cancellation in
                try await core.searchCatalogues(query: query, cancellation: cancellation)
            }
            return .success(found)
        } catch {
            if !Self.isStop(error) { log.warn("catalogue search failed: \(error)") }
            return .failure(ListenFailure(message: Self.message(error), stopped: Self.isStop(error)))
        }
    }

    /// The fixed test catalogue, its files made the first time.
    public func testCatalogue() async -> Result<[TestTrack], ListenFailure> {
        do {
            let found = try await withRustCancellation { cancellation in
                try await core.testCatalogue(cancellation: cancellation)
            }
            return .success(found)
        } catch {
            if !Self.isStop(error) { log.warn("test catalogue failed: \(error)") }
            return .failure(ListenFailure(message: Self.message(error), stopped: Self.isStop(error)))
        }
    }

    /// The genres to browse Jamendo by, in the order shown.
    public var jamendoGenres: [JamendoGenre] { core.jamendoGenres() }
    /// Jamendo genres for Discover's shelves, the person's own first.
    public func discoverGenres(limit: Int = 3) -> [JamendoGenre] {
        _ = collection
        return core.discoverGenres(limit: UInt32(limit))
    }

    /// Jamendo tracks in `genre`, this week's most popular first.
    public func jamendoTracks(in genre: JamendoGenre) async -> Result<[CatalogueTrack], ListenFailure> {
        do {
            let found = try await withRustCancellation { cancellation in
                try await core.jamendoGenre(tag: genre.tag, cancellation: cancellation)
            }
            return .success(found)
        } catch {
            if !Self.isStop(error) { log.warn("jamendo genre failed: \(error)") }
            return .failure(ListenFailure(message: Self.message(error), stopped: Self.isStop(error)))
        }
    }

    /// More from Jamendo for the Jamendo track `trackId`: by its artist, or
    /// in its genre.
    public func moreFromJamendo(_ trackId: String, by: JamendoMore) async -> Result<JamendoMoreTracks, ListenFailure> {
        do {
            let found = try await withRustCancellation { cancellation in
                try await core.moreFromJamendo(trackId: trackId, by: by, cancellation: cancellation)
            }
            return .success(found)
        } catch {
            if !Self.isStop(error) { log.warn("jamendo more failed: \(error)") }
            return .failure(ListenFailure(message: Self.message(error), stopped: Self.isStop(error)))
        }
    }

    /// An Internet Archive item's playable tracks, in order.
    public func archiveTracks(_ item: ArchiveItem) async -> Result<[CatalogueTrack], ListenFailure> {
        do {
            let tracks = try await withRustCancellation { cancellation in
                try await core.archiveItemTracks(identifier: item.identifier, cancellation: cancellation)
            }
            return .success(tracks)
        } catch {
            if !Self.isStop(error) { log.warn("archive item failed: \(error)") }
            return .failure(ListenFailure(message: Self.message(error), stopped: Self.isStop(error)))
        }
    }

    /// True when a catalogue track is in the library.
    public func inLibrary(_ id: String) -> Bool {
        library.tracks.contains { $0.id == id }
    }

    /// Adds catalogue tracks to the library, with their licences.
    public func addToLibrary(_ tracks: [CatalogueTrack]) async {
        do {
            library = try await core.addCatalogueTracks(tracks: tracks)
            feedback.play(.success)
        } catch {
            report(error)
        }
    }

    /// Catalogue tracks to play: the library's copy where there is one
    /// (downloaded, edited), else the stream.
    public func queue(_ tracks: [CatalogueTrack]) -> [QueueTrack] {
        core.catalogueQueueTracks(tracks: tracks)
    }
    public func albums(of artist: LibraryArtist) -> [LibraryAlbum] { core.artistAlbums(artistId: artist.id) }
    public func tracks(of artist: LibraryArtist) -> [LibraryTrack] { core.artistTracks(artistId: artist.id) }
    /// An artist's top songs: most played, then favourites and likes.
    public func topTracks(of artist: LibraryArtist, limit: Int = 5) -> [LibraryTrack] {
        _ = collection
        return core.artistTopTracks(artistId: artist.id, limit: UInt32(limit))
    }
    public func tracks(of album: LibraryAlbum) -> [LibraryTrack] { core.albumTracks(albumId: album.id) }

    /// A track with its recordings, or `nil` once it has left the library.
    public func track(_ id: String) -> TrackDetail? { try? core.libraryTrack(id: id) }

    /// What plays for `tracks`: each one's preferred recording, in order.
    public func queue(_ tracks: [LibraryTrack]) -> [QueueTrack] {
        core.queueTracks(trackIds: tracks.map(\.id))
    }

    /// Where the song at `index` lands in `queue(tracks)`: songs that can't
    /// be played are left out of the queue, so the ones before it count
    /// only when they are in it.
    public func queueIndex(_ index: Int, in tracks: [LibraryTrack]) -> Int {
        queue(Array(tracks.prefix(index))).count
    }

    public func queueIndex(_ index: Int, in tracks: [CatalogueTrack]) -> Int {
        queue(Array(tracks.prefix(index))).count
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

    // MARK: Takes and versions

    /// A track with its recordings in `order`.
    public func track(_ id: String, order: RecordingOrder) -> TrackDetail? {
        try? core.libraryTrackSorted(id: id, order: order)
    }

    /// Songs with several takes and no pick yet.
    public var toPick: [LibraryTrack] { core.tracksToPick() }

    /// Rates one of a track's recordings on the take server; tapping its
    /// current rating clears it.
    public func rate(_ recording: Recording, of trackId: String, stars: Int) async -> TrackDetail? {
        let value = stars == Int(recording.stars) ? 0 : max(0, min(5, stars))
        return await markRecording(recording, of: trackId, TakeMark(picked: nil, stars: UInt8(value), note: nil))
    }

    public func note(_ recording: Recording, of trackId: String, _ text: String) async -> TrackDetail? {
        await markRecording(recording, of: trackId, TakeMark(picked: nil, stars: nil, note: text))
    }

    private func markRecording(_ recording: Recording, of trackId: String, _ mark: TakeMark) async -> TrackDetail? {
        do {
            let detail = try await core.markRecording(trackId: trackId, recordingId: recording.id, mark: mark)
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

    // MARK: MusicBrainz

    /// Recordings MusicBrainz knows that might be this song, best first.
    /// Changes nothing. Cancelled with the screen that asked.
    public func identify(_ track: LibraryTrack) async -> Result<[MusicBrainzMatch], ListenFailure> {
        do {
            let found = try await withRustCancellation { cancellation in
                try await core.identifyTrack(trackId: track.id, cancellation: cancellation)
            }
            return .success(found)
        } catch {
            if !Self.isStop(error) { log.warn("musicbrainz lookup failed: \(error)") }
            return .failure(ListenFailure(message: Self.message(error), stopped: Self.isStop(error)))
        }
    }

    /// Links a song to the MusicBrainz recording picked, and with
    /// `useDetails` gives it that recording's title, artist and album.
    public func link(_ track: LibraryTrack, to found: MusicBrainzMatch, useDetails: Bool) async -> TrackDetail? {
        do {
            let detail = try await core.linkMusicbrainz(trackId: track.id, found: found, useDetails: useDetails)
            library = core.library()
            feedback.play(.success)
            return detail
        } catch {
            report(error)
            return nil
        }
    }

    /// Forgets a song's MusicBrainz link; details it took stay.
    public func unlink(_ track: LibraryTrack) async -> TrackDetail? {
        do {
            let detail = try await core.unlinkMusicbrainz(trackId: track.id)
            library = core.library()
            return detail
        } catch {
            report(error)
            return nil
        }
    }

    // MARK: One song, several sources

    /// Merges `others` into `track`: one song with every source's
    /// recordings. Returns the merged song.
    public func merge(_ others: [String], into track: String) async -> TrackDetail? {
        do {
            let detail = try await core.mergeSongs(into: track, ids: others)
            library = core.library()
            // Favourites and songs chosen for the phone moved to the kept song.
            collection = core.collection()
            offline = core.offline()
            feedback.play(.success)
            return detail
        } catch {
            report(error)
            return nil
        }
    }

    /// Brings a merged song back as a song of its own.
    public func separate(_ id: String) async {
        do {
            library = try await core.separateSong(id: id)
            // What plays may have changed, and with it what is downloaded.
            offline = core.offline()
        } catch {
            report(error)
        }
    }

    /// Stops offering these songs for merging.
    public func keepApart(_ ids: [String]) async {
        do {
            try await core.keepSongsApart(ids: ids)
            duplicates = core.duplicateSongs()
        } catch {
            report(error)
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

    /// The song playing becomes a favourite, or stops being one; a
    /// catalogue song (a radio find) joins the library first.
    public func toggleFavourite(queued track: QueueTrack) async {
        if !isFavourite(track.trackId), !inLibrary(track.trackId) {
            do {
                library = try await core.addQueuedToLibrary(tracks: [track])
            } catch {
                report(error)
                return
            }
        }
        await toggleFavourite(track.trackId)
    }

    /// A song, album or artist (by library id) becomes a favourite, or stops being one.
    public func toggleFavourite(_ id: String) async {
        do {
            collection = try await core.setFavourite(id: id, on: !isFavourite(id))
            feedback.play(.success)
        } catch {
            report(error)
        }
    }

    // MARK: Radio

    /// Stations to offer: the saved ones, then your genres and artists,
    /// then Jamendo's genres.
    public var suggestedStations: [Station] {
        _ = stationRevision
        return core.suggestedStations()
    }

    /// Changes when a station is saved or forgotten.
    public private(set) var stationRevision = 0

    public func isStationSaved(_ seed: StationSeed) -> Bool {
        _ = stationRevision
        return core.isStationSaved(seed: seed)
    }

    /// Saves a station at the front of Hear Nonstop Music, or forgets it.
    public func toggleStationSaved(_ seed: StationSeed, title: String) async {
        do {
            try await core.setStationSaved(seed: seed, title: title, saved: !core.isStationSaved(seed: seed))
            stationRevision += 1
            feedback.play(.success)
        } catch {
            report(error)
        }
    }

    /// How the person feels about a song for the radio, if they said.
    public func feeling(_ trackId: String) -> Feeling? { core.feeling(trackId: trackId) }

    /// Likes or dislikes a song for the radio; the same again takes it back.
    public func toggleFeeling(_ trackId: String, _ feeling: Feeling) async {
        let new: Feeling? = core.feeling(trackId: trackId) == feeling ? nil : feeling
        do {
            try await core.setFeeling(trackId: trackId, feeling: new)
            feelingRevision += 1
            feedback.play(.success)
        } catch {
            report(error)
        }
    }

    /// Changes when a song is liked or disliked, so screens read it again.
    public private(set) var feelingRevision = 0

    /// The next songs for a station (Rust mixes them).
    public func stationBatch(
        _ seed: StationSeed, played: [String], recentArtists: [String], size: Int = 10
    ) async -> Result<[QueueTrack], StationFailure> {
        do {
            let tracks = try await withRustCancellation { cancellation in
                try await core.stationBatch(
                    seed: seed, played: played, recentArtists: recentArtists,
                    size: UInt32(size), seedNumber: UInt64.random(in: 0...UInt64.max),
                    cancellation: cancellation
                )
            }
            return .success(tracks)
        } catch {
            if !Self.isStop(error) { log.warn("station failed: \(error)") }
            var empty = false
            if case SoundscapeError.NotInLibrary = error { empty = true }
            return .failure(StationFailure(
                message: Self.message(error), stopped: Self.isStop(error), nothingToPlay: empty
            ))
        }
    }

    public func playlist(_ id: UInt64) -> PlaylistDetail? { try? core.playlist(id: id) }

    /// A new playlist, empty or with `tracks`. Throws when Rust refuses the
    /// name, so the name sheet can say why.
    @discardableResult
    public func createPlaylist(_ name: String, with tracks: [String] = []) async throws -> PlaylistDetail {
        try await savePlaylists { try await self.core.createPlaylist(name: name, trackIds: tracks) }
    }

    /// Throws when Rust refuses the name, so the name sheet can say why.
    @discardableResult
    public func renamePlaylist(_ id: UInt64, to name: String) async throws -> PlaylistDetail {
        try await savePlaylists { try await self.core.renamePlaylist(id: id, name: name) }
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

    /// Deletes a playlist, with Undo on the banner.
    public func deletePlaylist(_ id: UInt64) async {
        let name = collection.playlists.first { $0.id == id }?.name
        do {
            collection = try await core.deletePlaylist(id: id)
            banner = .undo("Deleted \(name.map { "“\($0)”" } ?? "Playlist")",
                           action: .deletedPlaylist(id), identifier: "undo.playlist")
        } catch {
            report(error)
        }
    }

    /// Removes songs from a playlist, keeping the order left (by place),
    /// with Undo on the banner.
    public func removeFromPlaylist(_ detail: PlaylistDetail, keeping places: [UInt32]) async -> PlaylistDetail? {
        // Every song it keeps, also those the library no longer has.
        let before = (try? core.playlistTrackIds(id: detail.playlist.id)) ?? detail.entries.map(\.track.id)
        let gone = detail.entries.count - places.count
        guard let changed = await arrangePlaylist(detail.playlist.id, places: places) else { return nil }
        if gone > 0 {
            banner = .undo(gone == 1 ? "Removed from “\(detail.playlist.name)”" : "Removed \(gone) songs",
                           action: .playlistSongs(id: detail.playlist.id, trackIds: before),
                           identifier: "undo.playlistSongs")
        }
        return changed
    }

    /// Makes a playlist of the queue (catalogue songs join the library).
    /// Throws when Rust refuses the name, so the name sheet can say why.
    @discardableResult
    public func saveQueue(_ tracks: [QueueTrack], as name: String) async throws -> PlaylistDetail {
        let detail = try await savePlaylists { try await self.core.saveQueueAsPlaylist(name: name, tracks: tracks) }
        library = core.library()
        return detail
    }

    /// Takes back what the banner offered (all but the queue, which the
    /// player puts back).
    public func undo(_ undo: Undo) async {
        do {
            switch undo {
            case .queue:
                return
            case .playlistSongs(let id, let trackIds):
                _ = try await core.setPlaylistTracks(id: id, trackIds: trackIds)
                collection = core.collection()
            case .deletedPlaylist(let id):
                collection = try await core.restorePlaylist(id: id)
            case .removedSong(let id):
                library = try await core.restoreRemoved(trackId: id)
                offline = core.offline()
            }
            feedback.play(.success)
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
            return try await savePlaylists(change)
        } catch {
            report(error)
            return nil
        }
    }

    /// A playlist change whose failure the caller shows itself.
    private func savePlaylists(_ change: () async throws -> PlaylistDetail) async throws -> PlaylistDetail {
        let detail = try await change()
        collection = core.collection()
        feedback.play(.success)
        return detail
    }

    // MARK: On the phone

    /// Several songs (an album) as one sign: failed if any failed, on its
    /// way while any is, downloaded once every downloadable one is here.
    public func downloadState(of tracks: [LibraryTrack]) -> DownloadState {
        let states = downloadable(tracks).map { downloadState($0.id) }
        if states.isEmpty { return .notDownloaded }
        for state in states { if case .failed = state { return state } }
        if states.contains(where: { $0 == .downloading || $0 == .waiting }) { return .downloading }
        return states.allSatisfy { $0 == .downloaded } ? .downloaded : .notDownloaded
    }

    public func downloadState(_ trackId: String) -> DownloadState {
        downloadStates[trackId] ?? .notDownloaded
    }

    /// Songs that can be downloaded: your own files are on the phone already.
    public func downloadable(_ tracks: [LibraryTrack]) -> [LibraryTrack] {
        tracks.filter { track in
            guard let recording = track.preferred, recording.kind != .imported else { return false }
            return recording.rights?.mayDownload ?? true
        }
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

    /// Takes a song the person added out of the library (Soundscape's copy
    /// of a file goes after the next removal), with Undo on the banner.
    public func removeImported(_ trackId: String) async {
        let title = library.tracks.first { $0.id == trackId }?.title
        do {
            library = try await core.removeImported(trackId: trackId)
            offline = core.offline()
            banner = .undo("Removed \(title.map { "“\($0)”" } ?? "Song") from Library",
                           action: .removedSong(trackId), identifier: "undo.song")
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
        if let error = error as? SoundscapeError { return SoundscapeErrors.describe(error) }
        return error.localizedDescription
    }
}

/// Why a station couldn't give more songs.
public struct StationFailure: Error, Equatable {
    public let message: String
    /// Cancelled because the screen closed: show nothing.
    public let stopped: Bool
    /// Nothing fits the station at all: trying again won't help. Otherwise
    /// the failure is worth retrying (a dropped connection, Jamendo busy).
    public let nothingToPlay: Bool
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

extension CatalogueTrack: Identifiable {}
