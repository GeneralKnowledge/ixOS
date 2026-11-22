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
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: SoundscapeMetrics.sectionGap) {
                Text(Self.greeting(Date()))
                    .font(.system(.title3, design: .rounded).weight(.semibold))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, SoundscapeMetrics.screenEdge)
                    .accessibilityIdentifier("listen.greeting")
                continueListening
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
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("listen.playlist")
                }
                Shelf(title: "Recently Added", items: recentAlbums, seeAll: .albums,
                      identifier: "listen.albums") { album in
                    NavigationLink(value: ListenRoute.album(album)) {
                        ArtworkTile(title: album.title, subtitle: album.artist, url: album.artworkUrl)
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
            if isEmpty {
                ForgeStateView.empty(
                    "Nothing to Play Yet", systemImage: "music.note.house",
                    description: "Find music in Discover or Search, import your own files in Library, or add a take server."
                )
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
            ArtworkTile(title: track.title, subtitle: track.artist, url: track.artworkUrl)
        }
        .buttonStyle(.plain)
        .accessibilityHint("Plays it")
        .accessibilityIdentifier(identifier)
        .contextMenu {
            Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                player.add(model.queue([track]), next: true)
            }
            .accessibilityIdentifier("\(identifier).playNext")
            Button("Add to Queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                player.add(model.queue([track]), next: false)
            }
            .accessibilityIdentifier("\(identifier).addToQueue")
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

    var body: some View {
        HStack(spacing: 14) {
            Button(action: open) {
                HStack(spacing: 14) {
                    Artwork(title: title, url: artworkUrl, size: 72)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(caption.uppercased())
                            .font(.caption2.weight(.semibold))
                            .tracking(0.8)
                            .foregroundStyle(.white.opacity(0.7))
                        Text(title)
                            .font(.headline)
                            .foregroundStyle(.white)
                            .lineLimit(1)
                        Text(subtitle)
                            .font(.subheadline)
                            .foregroundStyle(.white.opacity(0.7))
                            .lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
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
