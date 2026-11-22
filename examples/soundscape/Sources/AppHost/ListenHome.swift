#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// The Listen tab: a greeting, where you left off, what you played
/// lately, your playlists, albums added lately and what's popular on
/// Jamendo, as shelves of artwork.
struct ListenHome: View {
    let model: ListenModel
    let player: TakePlayer
    let radio: Radio
    let openNowPlaying: () -> Void

    @State private var popular: [CatalogueTrack] = []

    private var collection: MusicCollection { model.collection }

    /// Songs played lately, newest first, each once.
    private var recentTracks: [LibraryTrack] {
        var seen = Set<String>()
        return collection.recentlyPlayed.map(\.track).filter { seen.insert($0.id).inserted }
    }

    private var recentAlbums: [LibraryAlbum] {
        Array(model.library.albums.sorted { $0.updatedMs > $1.updatedMs }.prefix(12))
    }

    private var isEmpty: Bool {
        model.library.tracks.isEmpty && player.current == nil && popular.isEmpty
            && model.suggestedStations.isEmpty
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: SoundscapeMetrics.sectionGap) {
                Text(Self.greeting(Date()))
                    .font(.system(.title3, design: .rounded).weight(.semibold))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, SoundscapeMetrics.screenEdge)
                    .accessibilityIdentifier("listen.greeting")
                if radio.problem != nil {
                    RadioProblem(radio: radio, model: model, player: player)
                        .padding(12)
                        .background(.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                        .padding(.horizontal, SoundscapeMetrics.screenEdge)
                }
                continueListening
                Shelf(title: "Hear Nonstop Music", items: model.suggestedStations, identifier: "listen.radio") { station in
                    Button {
                        Task { await radio.start(station.seed, title: station.title, model: model, player: player) }
                    } label: {
                        StationTile(station: station)
                            .overlay {
                                if radio.isTuning && radio.seed == station.seed {
                                    ProgressView().tint(.white)
                                }
                            }
                    }
                    .buttonStyle(.plain)
                    .accessibilityHint("Starts nonstop music")
                    .accessibilityIdentifier("listen.station")
                    .contextMenu {
                        Button(station.saved ? "Remove Saved Station" : "Save Station",
                               systemImage: station.saved ? "pin.slash" : "pin") {
                            Task { await model.toggleStationSaved(station.seed, title: station.title) }
                        }
                        .accessibilityIdentifier("listen.saveStation")
                    }
                }
                Shelf(title: "Recently Played", items: Array(recentTracks.prefix(15)), seeAll: .recent,
                      identifier: "listen.recent") { track in
                    trackTile(track, in: recentTracks, identifier: "listen.recentSong")
                }
                Shelf(title: "Your Playlists", items: collection.playlists, seeAll: .playlists,
                      identifier: "listen.playlists") { list in
                    NavigationLink(value: ListenRoute.playlist(list.id)) {
                        VStack(alignment: .leading, spacing: 6) {
                            PlaylistArtwork(list: list, size: SoundscapeMetrics.shelfArtwork)
                            Text(list.name)
                                .font(.subheadline.weight(.medium))
                                .lineLimit(1)
                            Text(TakeFormat.playlistSummary(list))
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                        .frame(width: SoundscapeMetrics.shelfArtwork, alignment: .leading)
                        .accessibilityElement(children: .combine)
                        .surfaceSource(.playlist(list.id))
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("listen.playlist")
                }
                Shelf(title: "Recently Added", items: recentAlbums, seeAll: .albums,
                      identifier: "listen.albums") { album in
                    NavigationLink(value: ListenRoute.album(album)) {
                        ArtworkTile(title: album.title, subtitle: album.artist, url: album.artworkUrl,
                                    download: model.downloadState(of: model.tracks(of: album)))
                            .surfaceSource(.album(album))
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("listen.album")
                }
                Shelf(title: "Favourite Songs", items: collection.favouriteTracks, seeAll: .favourites,
                      identifier: "listen.favourites") { track in
                    trackTile(track, in: collection.favouriteTracks, identifier: "listen.favouriteSong")
                }
                Shelf(title: "Popular on Jamendo", items: popular, seeAll: .discover,
                      identifier: "listen.popular") { track in
                    Button {
                        let index = popular.firstIndex { $0.id == track.id } ?? 0
                        player.play(model.queue(popular), from: index)
                    } label: {
                        ArtworkTile(title: track.title, subtitle: track.artist, url: track.artworkUrl)
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("listen.popularSong")
                    .contextMenu {
                        SongMenu(model: model, player: player, song: SongRef(track, model: model),
                                 identifier: "listen.popularSong")
                    }
                }
            }
            .padding(.vertical, 8)
        }
        .navigationTitle("Listen")
        .refreshable {
            await model.loadLibrary(fresh: true)
            await loadPopular()
        }
        .task(id: model.jamendoClientId) { await loadPopular() }
        .overlay {
            // The library's phase comes first: loading and failed only
            // happen while the library is empty, and stations or Jamendo's
            // popular songs must not hide them. The empty state waits until
            // the library is ready.
            switch model.phase {
            case .loading:
                ForgeStateView.loading("Asking the take server")
            case .failed(let message):
                ForgeStateView.failed(
                    "Can't Load the Library", message: message,
                    retry: ForgeStateAction("Try Again", identifier: "listen.homeRetry") {
                        Task { await model.loadLibrary() }
                    }
                )
            case .ready:
                if isEmpty {
                    ForgeStateView.empty(
                        "Nothing to Play Yet", systemImage: "music.note.house",
                        description: "Find music in Discover or Search, import your own files in Library, or add a take server."
                    )
                }
            }
        }
    }

    /// Where you left off: the song in the player (kept across launches),
    /// in a card in its own colours, to carry on or open.
    @ViewBuilder private var continueListening: some View {
        if let current = player.current, !player.isComparing {
            ContinueCard(player: player, current: current, open: openNowPlaying)
                .padding(.horizontal, SoundscapeMetrics.screenEdge)
        } else if let last = recentTracks.first {
            ContinueCard(track: last) {
                player.play(model.queue(recentTracks), from: 0)
            }
            .padding(.horizontal, SoundscapeMetrics.screenEdge)
        }
    }

    private func trackTile(_ track: LibraryTrack, in tracks: [LibraryTrack], identifier: String) -> some View {
        Button {
            let index = tracks.firstIndex { $0.id == track.id } ?? 0
            player.play(model.queue(tracks), from: index)
        } label: {
            ArtworkTile(title: track.title, subtitle: track.artist, url: track.artworkUrl,
                        download: DownloadBadge(model: model, track: track).state)
        }
        .buttonStyle(.plain)
        .accessibilityHint("Plays it")
        .accessibilityIdentifier(identifier)
        .contextMenu {
            SongMenu(model: model, player: player, song: SongRef(track, model: model), identifier: identifier)
        }
    }

    private func loadPopular() async {
        guard model.jamendoClientId != nil else {
            popular = []
            return
        }
        // Quietly left out when Jamendo can't answer: Discover says why.
        if case .success(let found) = await model.searchJamendo("") {
            popular = Array(found.prefix(15))
        }
    }

    /// "Good morning" before noon, "Good afternoon" until six, then
    /// "Good evening".
    static func greeting(_ date: Date, calendar: Calendar = .current) -> String {
        switch calendar.component(.hour, from: date) {
        case 5..<12: return "Good morning"
        case 12..<18: return "Good afternoon"
        default: return "Good evening"
        }
    }
}

/// A wide card for the song to carry on with, drawn in its artwork's
/// colours.
private struct ContinueCard: View {
    let title: String
    let subtitle: String
    let artworkUrl: String?
    let playing: Bool
    let caption: String
    let toggle: () -> Void
    let open: () -> Void

    @State private var palette: ArtworkPalette

    /// The song in the player.
    init(player: TakePlayer, current: TakePlayer.Loaded, open: @escaping () -> Void) {
        title = current.song
        subtitle = current.artist ?? current.label
        artworkUrl = current.artworkUrl
        playing = player.isPlaying
        caption = player.isPlaying ? "Now Playing" : "Pick Up Where You Left Off"
        toggle = { player.toggle() }
        self.open = open
        _palette = State(initialValue: .standIn(title: current.song))
    }

    /// The last song played, to start again.
    init(track: LibraryTrack, play: @escaping () -> Void) {
        title = track.title
        subtitle = track.artist
        artworkUrl = track.artworkUrl
        playing = false
        caption = "Play Again"
        toggle = play
        open = play
        _palette = State(initialValue: .standIn(title: track.title))
    }

    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        // At the accessibility sizes the words go under the artwork, so
        // they keep their room.
        let layout = typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 10))
            : AnyLayout(HStackLayout(spacing: 14))
        HStack(spacing: 14) {
            Button(action: open) {
                layout {
                    Artwork(title: title, url: artworkUrl, size: 72)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(caption.uppercased())
                            .font(.caption2.weight(.semibold))
                            .tracking(0.8)
                            .foregroundStyle(.white.opacity(0.7))
                        Text(title)
                            .font(.headline)
                            .foregroundStyle(.white)
                            .lineLimit(typeSize.isAccessibilitySize ? 3 : 1)
                        Text(subtitle)
                            .font(.subheadline)
                            .foregroundStyle(.white.opacity(0.7))
                            .lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("listen.continue")
            Button(action: toggle) {
                Image(systemName: playing ? "pause.fill" : "play.fill")
                    .font(.title2)
                    .foregroundStyle(.white)
                    .contentTransition(.symbolEffect(.replace))
                    .frame(width: 48, height: 48)
                    .background(.white.opacity(0.18), in: Circle())
            }
            .accessibilityLabel(playing ? "Pause" : "Play")
            .accessibilityIdentifier("listen.continuePlay")
        }
        .padding(12)
        .background {
            LinearGradient(colors: [palette.top.color, palette.bottom.color],
                           startPoint: .topLeading, endPoint: .bottomTrailing)
        }
        .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
        .task(id: "\(title)|\(artworkUrl ?? "")") {
            let found = await ArtworkPaletteLoader.palette(title: title, url: artworkUrl)
            withAnimation(SoundscapeMotion.paletteFade) { palette = found }
        }
    }
}
#endif
