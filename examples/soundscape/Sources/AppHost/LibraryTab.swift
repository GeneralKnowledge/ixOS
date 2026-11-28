#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

// MARK: The Library tab

/// Your music by kind (playlists, artists, albums, songs, favourites,
/// downloads), then the albums added lately as artwork.
struct LibraryTab: View {
    let model: ListenModel
    let player: TakePlayer
    let editServer: () -> Void

    @Environment(LiveRadioModel.self) private var live

    private var collection: MusicCollection { model.collection }
    private var library: Library { model.library }

    var body: some View {
        List {
            if let problem = model.refreshProblem {
                Section {
                    ForgeNotice(problem, systemImage: "exclamationmark.icloud")
                        .listRowInsets(EdgeInsets())
                        .listRowBackground(Color.clear)
                        .accessibilityIdentifier("library.refreshProblem")
                }
            }
            Section {
                category("Playlists", systemImage: "music.note.list", route: .playlists,
                         count: collection.playlists.count, identifier: "library.playlists")
                category("Artists", systemImage: "music.mic", route: .artists,
                         count: library.artists.count, identifier: "library.artists")
                category("Albums", systemImage: "square.stack", route: .albums,
                         count: library.albums.count, identifier: "library.albums")
                category("Songs", systemImage: "music.note", route: .songs,
                         count: library.tracks.count, identifier: "library.songs")
                category("Favourites", systemImage: "heart", route: .favourites,
                         count: collection.favouriteTracks.count + collection.favouriteAlbums.count
                             + collection.favouriteArtists.count,
                         identifier: "library.favourites")
                category("Downloaded", systemImage: "arrow.down.circle", route: .downloads,
                         count: Int(model.offline.downloaded), identifier: "library.downloads")
                category("Recently Played", systemImage: "clock", route: .recent,
                         count: nil, identifier: "library.recent")
                category("Stations", systemImage: "dot.radiowaves.left.and.right", route: .liveSaved,
                         count: live.kept.saved.count, identifier: "library.stations")
                let toPick = model.toPick.count
                if toPick > 0 {
                    category("To Pick", systemImage: "checkmark.circle.badge.questionmark", route: .toPick,
                             count: toPick, identifier: "library.toPick")
                }
                if !model.duplicates.isEmpty {
                    category("Same Song, Several Sources", systemImage: "square.on.square", route: .duplicates,
                             count: model.duplicates.count, identifier: "library.duplicates")
                }
            }
            let recent = Array(library.albums.sorted { $0.updatedMs > $1.updatedMs }.prefix(12))
            if !recent.isEmpty {
                Section {
                    AlbumGrid(albums: recent, identifier: "library.recentAlbum", model: model)
                        .listRowInsets(EdgeInsets(top: 8, leading: 0, bottom: 8, trailing: 0))
                        .listRowBackground(Color.clear)
                } header: {
                    Text("Recently Added")
                        .font(.shelfTitle)
                        .foregroundStyle(.primary)
                        .textCase(nil)
                        .accessibilityAddTraits(.isHeader)
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Library")
        .refreshable { await model.loadLibrary(fresh: true) }
        .toolbar {
            ToolbarItem(placement: .secondaryAction) {
                ImportMusicButton(model: model)
            }
            ToolbarItem(placement: .secondaryAction) {
                Button(model.server == nil ? "Add Take Server" : "Take Server", systemImage: "server.rack",
                       action: editServer)
                    .accessibilityIdentifier("listen.server")
            }
        }
        .overlay {
            if library.tracks.isEmpty {
                ForgeStateView.empty(
                    "No Music Yet", systemImage: "music.note.list",
                    description: "Find music in Discover, import your own files, or add a take server."
                )
            }
        }
    }

    private func category(
        _ title: LocalizedStringKey, systemImage: String, route: ListenRoute, count: Int?, identifier: String
    ) -> some View {
        NavigationLink(value: route) {
            ForgeRow(title, systemImage: systemImage, accent: .purple) {
                if let count, count > 0 {
                    Text(count, format: .number)
                }
            }
        }
        .accessibilityIdentifier(identifier)
    }
}

/// Albums as artwork, two to a row (three or more on a wide screen).
struct AlbumGrid: View {
    let albums: [LibraryAlbum]
    let identifier: String
    /// Shows each album's download sign.
    var model: ListenModel? = nil

    private let columns = [GridItem(.adaptive(minimum: 150), spacing: 16, alignment: .top)]

    var body: some View {
        LazyVGrid(columns: columns, alignment: .leading, spacing: 20) {
            ForEach(albums) { album in
                ForgeSurfaceLink(ListenRoute.album(album)) {
                    ArtworkTile(title: album.title, subtitle: album.artist, url: album.artworkUrl, size: nil,
                                download: model.map { $0.downloadState(of: $0.tracks(of: album)) } ?? .notDownloaded)
                        .forgeSurfaceSource(ListenRoute.album(album))
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier(identifier)
            }
        }
        .padding(.horizontal, 4)
    }
}

// MARK: Songs, albums, artists

/// Every song, with Play and Shuffle at the top.
struct SongsView: View {
    let model: ListenModel
    let player: TakePlayer
    @Environment(\.forgeOpenSurface) private var openSurface

    var body: some View {
        let tracks = model.library.tracks
        List {
            Section {
                PlayShuffleButtons(disabled: tracks.isEmpty) { shuffle in
                    player.play(model.queue(tracks), shuffle: shuffle)
                }
            }
            Section("\(tracks.count) \(tracks.count == 1 ? "Song" : "Songs")") {
                ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                    TrackRow(model: model, player: player, track: track, showDetails: { openSurface(ListenRoute.track(track.id)) }) {
                        player.play(model.queue(tracks, from: index))
                    }
                }
            }
        }
        .navigationTitle("Songs")
        .overlay {
            if tracks.isEmpty {
                ForgeStateView.empty("No Songs", systemImage: "music.note",
                                     description: "Songs you add or import show here.")
            }
        }
    }
}

/// Every album as artwork.
struct AlbumsView: View {
    let model: ListenModel

    var body: some View {
        let albums = model.library.albums
        ScrollView {
            AlbumGrid(albums: albums, identifier: "library.album", model: model)
                .padding(.horizontal, SoundscapeMetrics.screenEdge - 4)
                .padding(.vertical, 12)
        }
        .navigationTitle("Albums")
        .overlay {
            if albums.isEmpty {
                ForgeStateView.empty("No Albums", systemImage: "square.stack",
                                     description: "Albums show here once they have songs in your library.")
            }
        }
    }
}

/// Every artist, each with their picture in a circle.
struct ArtistsView: View {
    let model: ListenModel

    var body: some View {
        let artists = model.library.artists
        List(artists) { artist in
            NavigationLink(value: ListenRoute.artist(artist)) { ArtistRow(artist: artist) }
                .accessibilityIdentifier("library.artist")
        }
        .listStyle(.plain)
        .navigationTitle("Artists")
        .overlay {
            if artists.isEmpty {
                ForgeStateView.empty("No Artists", systemImage: "music.mic",
                                     description: "Artists show here once they have songs in your library.")
            }
        }
    }
}
#endif
