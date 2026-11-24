#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

// S5 (docs/design.md): Discover before you search, as an editorial page:
// the week's top song, what is popular, stations to start, new music in
// your genres, what you found lately, and every genre.

extension JamendoGenre: Identifiable {
    public var id: String { tag }
}

struct DiscoverHome: View {
    let model: ListenModel
    let player: TakePlayer
    /// Jamendo's popular this week, already loaded by Discover.
    let popular: [CatalogueTrack]

    @State private var byGenre: [String: [CatalogueTrack]] = [:]
    @State private var byShelf: [JamendoShelf: [CatalogueTrack]] = [:]
    @State private var artists: [JamendoArtist] = []
    @State private var newArtists: [JamendoArtist] = []
    @State private var because: JamendoBecause?
    @Environment(\.songActions) private var actions

    private var genres: [JamendoGenre] { model.discoverGenres(limit: 3) }

    /// Catalogue songs added to the library lately, newest first.
    private var discovered: [LibraryTrack] {
        Array(model.library.tracks
            .filter { $0.preferred?.kind == .catalogue }
            .sorted { $0.updatedMs > $1.updatedMs }
            .prefix(12))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: SoundscapeMetrics.sectionGap) {
                if let top = popular.first {
                    FeaturedSong(track: top) {
                        player.play(model.queue(popular), from: 0)
                    }
                    .padding(.horizontal, SoundscapeMetrics.screenEdge)
                    .contextMenu {
                        SongMenu(model: model, player: player, song: SongRef(top, model: model),
                                 identifier: "discover.featured")
                    }
                }
                Shelf(title: "Popular This Week", items: Array(popular.dropFirst().prefix(15)),
                      identifier: "discover.popular") { track in
                    catalogueTile(track, in: popular, identifier: "discover.popularSong")
                }
                if let because {
                    Shelf(title: LocalizedStringKey(because.title), items: because.tracks,
                          identifier: "discover.because") { track in
                        catalogueTile(track, in: because.tracks, identifier: "discover.becauseSong")
                    }
                }
                shelf(.picks)
                Shelf(title: "Artists to Discover", items: artists, seeAll: .jamendoArtists(.popular),
                      identifier: "discover.artists") { artist in
                    JamendoArtistTile(artist: artist)
                }
                shelf(.newReleases)
                shelf(.rising)
                Shelf(title: "Start a Station", items: model.jamendoGenres, identifier: "discover.stations") { genre in
                    Button {
                        actions.startRadio(.genre(name: genre.name), "\(genre.name) Station")
                    } label: {
                        GenreTile(name: genre.name)
                            .frame(width: 160)
                            .overlay(alignment: .topTrailing) {
                                Image(systemName: "dot.radiowaves.left.and.right")
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(.white.opacity(0.85))
                                    .padding(10)
                            }
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("\(genre.name) Station")
                    .accessibilityHint("Starts nonstop music")
                    .accessibilityIdentifier("discover.station")
                }
                ForEach(genres) { genre in
                    let tracks = byGenre[genre.tag] ?? []
                    Shelf(title: "New in \(genre.name)", items: tracks, seeAll: .jamendoGenre(genre),
                          identifier: "discover.genreShelf") { track in
                        catalogueTile(track, in: tracks, identifier: "discover.genreSong")
                    }
                }
                ForEach(model.jamendoShelves.filter { ![JamendoShelf.picks, .newReleases, .rising].contains($0) }, id: \.self) { kind in
                    shelf(kind)
                }
                Shelf(title: "New on Jamendo", items: newArtists, seeAll: .jamendoArtists(.new),
                      identifier: "discover.newArtists") { artist in
                    JamendoArtistTile(artist: artist)
                }
                Shelf(title: "Recently Discovered", items: discovered, seeAll: .songs,
                      identifier: "discover.recent") { track in
                    Button {
                        let index = discovered.firstIndex { $0.id == track.id } ?? 0
                        player.play(model.queue(discovered, from: index))
                    } label: {
                        ArtworkTile(title: track.title, subtitle: track.artist, url: track.artworkUrl,
                                    download: DownloadBadge(model: model, track: track).state)
                    }
                    .buttonStyle(.plain)
                    .accessibilityHint("Plays it")
                    .accessibilityIdentifier("discover.recentSong")
                    .contextMenu {
                        SongMenu(model: model, player: player, song: SongRef(track, model: model),
                                 identifier: "discover.recentSong")
                    }
                }
                VStack(alignment: .leading, spacing: 10) {
                    HStack(alignment: .firstTextBaseline) {
                        Text("Browse by Genre")
                            .font(.shelfTitle)
                            .accessibilityAddTraits(.isHeader)
                        Spacer()
                        NavigationLink(value: ListenRoute.jamendoGenres) {
                            Text("See All").font(.subheadline)
                        }
                        .accessibilityIdentifier("discover.genres")
                    }
                    GenreGrid(genres: Array(model.jamendoGenres.prefix(8)))
                }
                .padding(.horizontal, SoundscapeMetrics.screenEdge)
                Text("Music from Jamendo, each track under its artist's Creative Commons licence.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, SoundscapeMetrics.screenEdge)
            }
            .padding(.vertical, 8)
        }
        .task(id: genres.map(\.tag).joined(separator: ",")) { await loadGenres() }
        .task { await loadExploring() }
    }

    /// One of Jamendo's shelves, with See All.
    private func shelf(_ kind: JamendoShelf) -> some View {
        let tracks = byShelf[kind] ?? []
        return Shelf(title: LocalizedStringKey(jamendoShelfTitle(shelf: kind)), items: tracks,
                     seeAll: .jamendoShelf(kind), identifier: "discover.shelf") { track in
            catalogueTile(track, in: tracks, identifier: "discover.shelfSong")
        }
    }

    /// Because You Like, artists and Jamendo's shelves, top of the page
    /// first; each that can't load is left out quietly, as genres are.
    private func loadExploring() async {
        if because == nil, case .success(let found) = await model.jamendoBecause() {
            because = found.map { JamendoBecause(title: $0.title, fromTrackId: $0.fromTrackId,
                                                 tracks: Array($0.tracks.prefix(15))) }
        }
        for kind in model.jamendoShelves.prefix(1) where byShelf[kind] == nil {
            if case .success(let found) = await model.jamendoTracks(on: kind) {
                byShelf[kind] = Array(found.prefix(15))
            }
        }
        if artists.isEmpty, case .success(let found) = await model.jamendoArtists(.popular) {
            artists = Array(found.prefix(15))
        }
        for kind in model.jamendoShelves.dropFirst() where byShelf[kind] == nil {
            if Task.isCancelled { return }
            if case .success(let found) = await model.jamendoTracks(on: kind) {
                byShelf[kind] = Array(found.prefix(15))
            }
        }
        if newArtists.isEmpty, case .success(let found) = await model.jamendoArtists(.new) {
            newArtists = Array(found.prefix(15))
        }
    }

    private func catalogueTile(_ track: CatalogueTrack, in tracks: [CatalogueTrack], identifier: String) -> some View {
        Button {
            let index = tracks.firstIndex { $0.id == track.id } ?? 0
            player.play(model.queue(tracks, from: index))
        } label: {
            ArtworkTile(title: track.title, subtitle: track.artist, url: track.artworkUrl)
        }
        .buttonStyle(.plain)
        .accessibilityHint("Plays it")
        .accessibilityIdentifier(identifier)
        .contextMenu {
            SongMenu(model: model, player: player, song: SongRef(track, model: model), identifier: identifier)
        }
    }

    /// Each genre shelf's songs; one that can't load is left out quietly
    /// (Popular above already says when Jamendo can't answer).
    private func loadGenres() async {
        for genre in genres where byGenre[genre.tag] == nil {
            if case .success(let found) = await model.jamendoTracks(in: genre) {
                byGenre[genre.tag] = Array(found.prefix(12))
            }
            if Task.isCancelled { return }
        }
    }
}

/// The week's top song, wide, in its artwork's colours.
private struct FeaturedSong: View {
    let track: CatalogueTrack
    let play: () -> Void

    @State private var palette: ArtworkPalette
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.dynamicTypeSize) private var typeSize

    init(track: CatalogueTrack, play: @escaping () -> Void) {
        self.track = track
        self.play = play
        _palette = State(initialValue: .standIn(title: track.title))
    }

    var body: some View {
        let layout = typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 12))
            : AnyLayout(HStackLayout(spacing: 16))
        Button(action: play) {
            layout {
                Artwork(title: track.title, url: track.artworkUrl, size: 112)
                    .shadow(color: .black.opacity(0.3), radius: 10, y: 6)
                VStack(alignment: .leading, spacing: 4) {
                    Text("TOP THIS WEEK")
                        .font(.caption2.weight(.semibold))
                        .tracking(0.8)
                        .foregroundStyle(.white.opacity(0.75))
                    Text(track.title)
                        .font(.system(.title3, design: .rounded).weight(.bold))
                        .foregroundStyle(.white)
                        .lineLimit(2)
                    Text(track.artist ?? "Unknown Artist")
                        .font(.subheadline)
                        .foregroundStyle(.white.opacity(0.8))
                        .lineLimit(1)
                    Label("Play", systemImage: "play.fill")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 6)
                        .background(.white.opacity(0.2), in: Capsule())
                        .padding(.top, 4)
                }
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(14)
            .background {
                LinearGradient(colors: [palette.top.color, palette.bottom.color],
                               startPoint: .topLeading, endPoint: .bottomTrailing)
            }
            .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Top this week: \(track.title), \(track.artist ?? "Unknown Artist")")
        .accessibilityHint("Plays this week's popular songs")
        .accessibilityIdentifier("discover.featured")
        .task(id: track.artworkUrl ?? track.title) {
            let found = await ArtworkPaletteLoader.palette(title: track.title, url: track.artworkUrl)
            withAnimation(SoundscapeMotion.palette(reduced: reduceMotion)) { palette = found }
        }
    }
}
#endif
