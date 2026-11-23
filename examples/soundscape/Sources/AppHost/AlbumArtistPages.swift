#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI
import UIKit

// S4 (docs/design.md): an album and an artist as pages, in the colours of
// their artwork, with Play and Shuffle up top and more to go on to below.

// MARK: The artwork's colours behind a page

extension View {
    /// The artwork's colours washing down from the top of a list, fading
    /// into the system background, like Now Playing's but quieter.
    func artworkBackdrop(title: String, url: String?) -> some View {
        modifier(ArtworkBackdrop(title: title, url: url))
    }
}

private struct ArtworkBackdrop: ViewModifier {
    let title: String
    let url: String?
    @State private var palette: ArtworkPalette?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.colorScheme) private var colorScheme

    func body(content: Content) -> some View {
        content
            .scrollContentBackground(.hidden)
            .background(alignment: .top) {
                ZStack(alignment: .top) {
                    Color(uiColor: .systemBackground)
                    if let palette {
                        LinearGradient(
                            colors: [palette.top.color.opacity(colorScheme == .dark ? 0.7 : 0.45),
                                     palette.bottom.color.opacity(colorScheme == .dark ? 0.35 : 0.15),
                                     .clear],
                            startPoint: .top, endPoint: .bottom
                        )
                        .frame(height: 520)
                        .transition(.opacity)
                    }
                }
                .ignoresSafeArea()
                .accessibilityHidden(true)
            }
            .task(id: "\(title)|\(url ?? "")") {
                let found = await ArtworkPaletteLoader.palette(title: title, url: url)
                withAnimation(SoundscapeMotion.palette(reduced: reduceMotion)) { palette = found }
            }
    }
}

/// Play, Shuffle and (when there is one) a station, side by side.
private struct PageActions: View {
    let disabled: Bool
    let play: (_ shuffle: Bool) -> Void
    var station: (() -> Void)? = nil
    let identifier: String

    var body: some View {
        HStack(spacing: 12) {
            button("Play", systemImage: "play.fill", id: "\(identifier).play") { play(false) }
                .disabled(disabled)
            button("Shuffle", systemImage: "shuffle", id: "\(identifier).shuffle") { play(true) }
                .disabled(disabled)
            if let station {
                Button(action: station) {
                    Image(systemName: "dot.radiowaves.left.and.right")
                        .font(.headline)
                        .frame(minWidth: 44, minHeight: 44)
                }
                .buttonStyle(.bordered)
                .accessibilityLabel("Start Station")
                .accessibilityIdentifier("\(identifier).station")
            }
        }
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
        .listRowInsets(EdgeInsets(top: 4, leading: SoundscapeMetrics.screenEdge,
                                  bottom: 12, trailing: SoundscapeMetrics.screenEdge))
    }

    private func button(_ title: String, systemImage: String, id: String,
                        action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Label(title, systemImage: systemImage)
                .font(.headline)
                .frame(maxWidth: .infinity, minHeight: 44)
        }
        .buttonStyle(.bordered)
        .accessibilityIdentifier(id)
    }
}

/// A shelf inside a list: the row reaches both edges so it scrolls under
/// them.
private extension View {
    func shelfRow() -> some View {
        listRowInsets(EdgeInsets(top: 12, leading: 0, bottom: 12, trailing: 0))
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
    }
}

// MARK: An album

/// An album: its artwork large over its colours, the artist to go to, Play
/// and Shuffle, the songs by number, how long it is, and more by the artist.
struct AlbumView: View {
    let model: ListenModel
    let player: TakePlayer
    let album: LibraryAlbum

    @State private var editingArtwork = false
    @State private var details: String?
    @Environment(\.songActions) private var actions

    private var current: LibraryAlbum { model.library.albums.first { $0.id == album.id } ?? album }
    private var artist: LibraryArtist? { model.library.artists.first { $0.id == current.artistId } }

    var body: some View {
        let tracks = model.tracks(of: current)
        let others = artist.map { model.albums(of: $0).filter { $0.id != current.id } } ?? []
        List {
            header(tracks)
            PageActions(disabled: tracks.isEmpty, play: { shuffle in
                player.play(model.queue(tracks), shuffle: shuffle)
            }, station: artist.map { a -> () -> Void in
                { actions.startRadio(.artist(id: a.id), "\(a.name) Station") }
            },
               identifier: "album")
            Section {
                ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                    TrackRow(
                        model: model, player: player, track: track,
                        number: track.number ?? UInt32(index + 1),
                        showDetails: { details = track.id }
                    ) {
                        player.play(model.queue(tracks, from: index))
                    }
                }
            } footer: {
                Text(albumLength(tracks))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("album.length")
            }
            Section {
                AlbumDownloadButton(model: model, tracks: tracks)
            }
            .listRowBackground(Color.clear)
            if let artist, !others.isEmpty {
                Shelf(title: "More by \(artist.name)", items: others, identifier: "album.moreByArtist") { other in
                    NavigationLink(value: ListenRoute.album(other)) {
                        ArtworkTile(title: other.title, subtitle: nil, url: other.artworkUrl,
                                    download: model.downloadState(of: model.tracks(of: other)))
                            .surfaceSource(.album(other))
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("album.otherAlbum")
                }
                .shelfRow()
            }
        }
        .listStyle(.plain)
        .artworkBackdrop(title: current.title, url: current.artworkUrl)
        .navigationTitle(current.title)
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(item: $details) { id in
            TrackView(model: model, player: player, id: id)
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                FavouriteButton(model: model, id: current.id, identifier: "album.favourite")
            }
            ToolbarItem(placement: .primaryAction) {
                Menu("More", systemImage: "ellipsis.circle") {
                    Button("Add to Playlist…", systemImage: "text.badge.plus") {
                        actions.addToPlaylist(tracks.map(\.id))
                    }
                    .accessibilityIdentifier("album.addToPlaylist")
                    Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                        player.add(model.queue(tracks), next: true)
                    }
                    .accessibilityIdentifier("album.playNext")
                    Button("Add to Queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                        player.add(model.queue(tracks), next: false)
                    }
                    .accessibilityIdentifier("album.addToQueue")
                    DownloadMenuItem(model: model, tracks: tracks, identifier: "album.download")
                    Button("Artwork", systemImage: "photo") { editingArtwork = true }
                        .accessibilityIdentifier("album.artwork")
                }
                .accessibilityIdentifier("album.more")
            }
        }
        .forgeFeedback(model.feedback)
        .sheet(isPresented: $editingArtwork) {
            ArtworkSheet(model: model, title: current.title, url: current.artworkUrl,
                         target: .album(id: current.id), circle: false)
        }
    }

    private func header(_ tracks: [LibraryTrack]) -> some View {
        VStack(spacing: 8) {
            Artwork(title: current.title, url: current.artworkUrl, size: 260)
                .shadow(color: .black.opacity(0.25), radius: 18, y: 10)
                .padding(.bottom, 8)
            Text(current.title)
                .font(.system(.title2, design: .rounded).weight(.bold))
                .multilineTextAlignment(.center)
                .accessibilityAddTraits(.isHeader)
            if let artist {
                NavigationLink(value: ListenRoute.artist(artist)) {
                    Text(artist.name).font(.title3).foregroundStyle(.tint)
                }
                .buttonStyle(.plain)
                .accessibilityHint("Opens the artist")
                .accessibilityIdentifier("album.artist")
            } else {
                Text(current.artist).font(.title3).foregroundStyle(.secondary)
            }
            if let genre = Self.genre(tracks) {
                Text(genre.uppercased())
                    .font(.caption.weight(.semibold))
                    .tracking(0.6)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 8)
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    /// The genre most of its songs carry, if any.
    static func genre(_ tracks: [LibraryTrack]) -> String? {
        let named = tracks.compactMap(\.genre).filter { !$0.isEmpty }
        let counts = Dictionary(named.map { ($0, 1) }, uniquingKeysWith: +)
        return counts.max { $0.value < $1.value || ($0.value == $1.value && $0.key > $1.key) }?.key
    }

    /// "5 songs, 18 minutes".
    private func albumLength(_ tracks: [LibraryTrack]) -> String {
        let seconds = tracks.compactMap(\.durationSeconds).reduce(0, +)
        let songs = "\(tracks.count) \(tracks.count == 1 ? "song" : "songs")"
        guard seconds > 0 else { return songs }
        let minutes = Int((seconds / 60).rounded())
        return "\(songs), \(minutes) \(minutes == 1 ? "minute" : "minutes")"
    }
}

// MARK: An artist

/// An artist: their picture over its colours, Play, Shuffle and their
/// station, Top Songs (all of them a tap away), albums as a shelf, and more
/// on Jamendo when their songs came from there.
struct ArtistView: View {
    let model: ListenModel
    let player: TakePlayer
    let artist: LibraryArtist

    @State private var editingArtwork = false
    @State private var details: String?
    @State private var showingAll = false
    @Environment(\.songActions) private var actions

    /// The artist as the library has it now (artwork may have changed).
    private var current: LibraryArtist { model.library.artists.first { $0.id == artist.id } ?? artist }

    var body: some View {
        let albums = model.albums(of: current)
        let tracks = model.tracks(of: current)
        let top = model.topTracks(of: current)
        let shown = showingAll ? tracks : top
        List {
            header
            PageActions(disabled: tracks.isEmpty, play: { shuffle in
                player.play(model.queue(tracks), shuffle: shuffle)
            }, station: { actions.startRadio(.artist(id: current.id), "\(current.name) Station") },
               identifier: "artist")
            Section {
                ForEach(Array(shown.enumerated()), id: \.element.id) { index, track in
                    TrackRow(model: model, player: player, track: track, showDetails: { details = track.id }) {
                        player.play(model.queue(shown, from: index))
                    }
                }
                if tracks.count > top.count {
                    Button(showingAll ? "Show Top Songs" : "Show All \(tracks.count) Songs") {
                        withAnimation { showingAll.toggle() }
                    }
                    .accessibilityIdentifier("artist.allSongs")
                }
            } header: {
                Text(showingAll ? "Songs" : "Top Songs")
                    .font(.shelfTitle)
                    .foregroundStyle(.primary)
                    .textCase(nil)
                    .accessibilityAddTraits(.isHeader)
            }
            if !albums.isEmpty {
                Shelf(title: "Albums", items: albums, identifier: "artist.albums") { album in
                    NavigationLink(value: ListenRoute.album(album)) {
                        ArtworkTile(title: album.title, subtitle: "\(album.trackCount) \(album.trackCount == 1 ? "song" : "songs")",
                                    url: album.artworkUrl,
                                    download: model.downloadState(of: model.tracks(of: album)))
                            .surfaceSource(.album(album))
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("artist.album")
                }
                .shelfRow()
            }
            if let jamendo = tracks.first(where: { $0.id.hasPrefix("jamendo:") }) {
                Section {
                    NavigationLink(value: ListenRoute.jamendoMore(trackId: jamendo.id, by: .artist)) {
                        Label("More by \(current.name) on Jamendo", systemImage: "sparkles")
                    }
                    .accessibilityIdentifier("artist.moreOnJamendo")
                }
            }
        }
        .listStyle(.plain)
        .artworkBackdrop(title: current.name, url: current.artworkUrl)
        .navigationTitle(current.name)
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(item: $details) { id in
            TrackView(model: model, player: player, id: id)
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                FavouriteButton(model: model, id: current.id, identifier: "artist.favourite")
            }
            ToolbarItem(placement: .primaryAction) {
                Button("Artwork", systemImage: "photo") { editingArtwork = true }
                    .accessibilityIdentifier("artist.artwork")
            }
        }
        .sheet(isPresented: $editingArtwork) {
            ArtworkSheet(model: model, title: current.name, url: current.artworkUrl,
                         target: .artist(id: current.id), circle: true)
        }
    }

    private var header: some View {
        VStack(spacing: 10) {
            Artwork(title: current.name, url: current.artworkUrl, size: 200, circle: true)
                .shadow(color: .black.opacity(0.25), radius: 16, y: 8)
            Text(current.name)
                .font(.system(.largeTitle, design: .rounded).weight(.heavy))
                .multilineTextAlignment(.center)
                .minimumScaleFactor(0.6)
                .lineLimit(2)
                .accessibilityAddTraits(.isHeader)
            Text(TakeFormat.artistSummary(current))
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 8)
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }
}
#endif
