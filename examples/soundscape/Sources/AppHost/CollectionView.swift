#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

// MARK: On the library's first screen

/// Favourites, Playlists and Recently Played, above the library's shelves,
/// with the last few songs played one tap away.
struct YourMusicSection: View {
    let model: ListenModel
    let player: TakePlayer
    let showDetails: (String) -> Void

    private var collection: MusicCollection { model.collection }

    var body: some View {
        Section("Your Music") {
            NavigationLink(value: ListenRoute.favourites) {
                row("Favourites", systemImage: "heart.fill", tint: .pink,
                    count: collection.favouriteTracks.count + collection.favouriteAlbums.count
                        + collection.favouriteArtists.count)
            }
            .accessibilityIdentifier("library.favourites")
            NavigationLink(value: ListenRoute.playlists) {
                row("Playlists", systemImage: "music.note.list", tint: .indigo, count: collection.playlists.count)
            }
            .accessibilityIdentifier("library.playlists")
            NavigationLink(value: ListenRoute.recent) {
                row("Recently Played", systemImage: "clock.fill", tint: .orange, count: nil)
            }
            .accessibilityIdentifier("library.recent")
        }
        let recent = Array(collection.recentlyPlayed.prefix(3))
        if !recent.isEmpty {
            Section("Played Lately") {
                ForEach(Array(recent.enumerated()), id: \.element.track.id) { index, play in
                    TrackRow(model: model, player: player, track: play.track,
                             showDetails: { showDetails(play.track.id) }) {
                        player.play(model.queue(recent.map(\.track)), from: index)
                    }
                }
            }
        }
    }

    private func row(_ title: String, systemImage: String, tint: Color, count: Int?) -> some View {
        HStack(spacing: 12) {
            Image(systemName: systemImage)
                .font(.body)
                .foregroundStyle(.white)
                .frame(width: 32, height: 32)
                .background(tint.gradient, in: RoundedRectangle(cornerRadius: 8))
            Text(title)
            Spacer()
            if let count, count > 0 {
                Text("\(count)").foregroundStyle(.secondary)
            }
        }
    }
}

/// A heart that adds a song, album or artist to the favourites, or takes it out.
struct FavouriteButton: View {
    let model: ListenModel
    let id: String
    let identifier: String

    var body: some View {
        let on = model.isFavourite(id)
        Button(on ? "Remove from Favourites" : "Add to Favourites",
               systemImage: on ? "heart.fill" : "heart") {
            Task { await model.toggleFavourite(id) }
        }
        .tint(on ? .pink : nil)
        .accessibilityIdentifier(identifier)
    }
}

// MARK: Favourites

struct FavouritesView: View {
    let model: ListenModel
    let player: TakePlayer
    @State private var details: String?

    private var collection: MusicCollection { model.collection }

    var body: some View {
        let tracks = collection.favouriteTracks
        List {
            if !tracks.isEmpty {
                Section {
                    PlayShuffleButtons(disabled: false) { shuffle in
                        player.play(model.queue(tracks), shuffle: shuffle)
                    }
                }
                Section("Songs") {
                    ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                        TrackRow(model: model, player: player, track: track, showDetails: { details = track.id }) {
                            player.play(model.queue(tracks), from: index)
                        }
                    }
                }
            }
            if !collection.favouriteAlbums.isEmpty {
                Section("Albums") {
                    ForEach(collection.favouriteAlbums) { album in
                        NavigationLink(value: ListenRoute.album(album)) { AlbumRow(album: album) }
                            .accessibilityIdentifier("favourites.album")
                    }
                }
            }
            if !collection.favouriteArtists.isEmpty {
                Section("Artists") {
                    ForEach(collection.favouriteArtists) { artist in
                        NavigationLink(value: ListenRoute.artist(artist)) { ArtistRow(artist: artist) }
                            .accessibilityIdentifier("favourites.artist")
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Favourites")
        .navigationDestination(item: $details) { id in
            TrackView(model: model, player: player, id: id)
        }
        .overlay {
            if tracks.isEmpty && collection.favouriteAlbums.isEmpty && collection.favouriteArtists.isEmpty {
                ForgeStateView.empty(
                    "No Favourites Yet", systemImage: "heart",
                    description: "Tap the heart on a song, album or artist to keep it here."
                )
            }
        }
        .forgeFeedback(model.feedback)
    }
}

// MARK: Playlists

struct PlaylistsView: View {
    let model: ListenModel
    let player: TakePlayer
    @State private var naming = false
    @State private var deleting: Playlist?

    var body: some View {
        List {
            ForEach(model.collection.playlists) { list in
                NavigationLink(value: ListenRoute.playlist(list.id)) { PlaylistRow(list: list) }
                    .accessibilityIdentifier("playlists.playlist")
                    .swipeActions {
                        Button("Delete", systemImage: "trash", role: .destructive) { deleting = list }
                            .accessibilityIdentifier("playlists.delete")
                    }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Playlists")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("New Playlist", systemImage: "plus") { naming = true }
                    .accessibilityIdentifier("playlists.new")
            }
        }
        .overlay {
            if model.collection.playlists.isEmpty {
                ForgeStateView.empty(
                    "No Playlists Yet", systemImage: "music.note.list",
                    description: "Make one with +, or add a song from its menu."
                )
            }
        }
        .sheet(isPresented: $naming) {
            PlaylistNameSheet(title: "New Playlist", name: "") { name in
                await model.createPlaylist(name) != nil
            }
        }
        .confirmationDialog(
            "Delete \(deleting?.name ?? "")?",
            isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
            titleVisibility: .visible
        ) {
            Button("Delete Playlist", role: .destructive) {
                if let list = deleting { Task { await model.deletePlaylist(list.id) } }
            }
            .accessibilityIdentifier("playlists.confirmDelete")
        } message: {
            Text("The songs stay in your library.")
        }
    }
}

struct PlaylistRow: View {
    let list: Playlist

    var body: some View {
        HStack(spacing: 12) {
            PlaylistArtwork(list: list, size: 56)
            VStack(alignment: .leading, spacing: 2) {
                Text(list.name).font(.headline).lineLimit(1)
                Text(TakeFormat.playlistSummary(list))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// Four of its songs' pictures in a square, or one, or the stand-in.
struct PlaylistArtwork: View {
    let list: Playlist
    let size: CGFloat

    var body: some View {
        let urls = list.artworkUrls
        Group {
            if urls.count >= 4 {
                let half = size / 2
                VStack(spacing: 0) {
                    HStack(spacing: 0) {
                        Artwork(title: list.name, url: urls[0], size: half)
                        Artwork(title: list.name, url: urls[1], size: half)
                    }
                    HStack(spacing: 0) {
                        Artwork(title: list.name, url: urls[2], size: half)
                        Artwork(title: list.name, url: urls[3], size: half)
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: size * 0.08))
            } else {
                Artwork(title: list.name, url: urls.first, size: size)
            }
        }
        .accessibilityHidden(true)
    }
}

struct PlaylistView: View {
    let model: ListenModel
    let player: TakePlayer
    let id: UInt64

    @State private var detail: PlaylistDetail?
    @State private var renaming = false
    @State private var details: String?

    var body: some View {
        Group {
            if let detail {
                content(detail)
            } else {
                ForgeStateView.empty(
                    "Playlist Deleted", systemImage: "music.note.list",
                    description: "It isn't in your playlists any more."
                )
            }
        }
        .navigationTitle(detail?.playlist.name ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { detail = model.playlist(id) }
        .onChange(of: model.collection) { detail = model.playlist(id) }
        .navigationDestination(item: $details) { id in
            TrackView(model: model, player: player, id: id)
        }
        .toolbar {
            if detail != nil {
                ToolbarItem(placement: .primaryAction) {
                    Menu("More", systemImage: "ellipsis.circle") {
                        Button("Rename", systemImage: "pencil") { renaming = true }
                            .accessibilityIdentifier("playlist.rename")
                    }
                    .accessibilityIdentifier("playlist.more")
                }
                ToolbarItem(placement: .primaryAction) {
                    EditButton().accessibilityIdentifier("playlist.edit")
                }
            }
        }
        .sheet(isPresented: $renaming) {
            PlaylistNameSheet(title: "Rename Playlist", name: detail?.playlist.name ?? "") { name in
                await model.renamePlaylist(id, to: name) != nil
            }
        }
    }

    private func content(_ detail: PlaylistDetail) -> some View {
        let tracks = detail.entries.map(\.track)
        return List {
            Section {
                VStack(spacing: 10) {
                    PlaylistArtwork(list: detail.playlist, size: 200)
                        .shadow(radius: 10, y: 5)
                    Text(detail.playlist.name).font(.title2.bold()).multilineTextAlignment(.center)
                    Text(TakeFormat.playlistSummary(detail.playlist))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
            }
            Section {
                PlayShuffleButtons(disabled: tracks.isEmpty) { shuffle in
                    player.play(model.queue(tracks), shuffle: shuffle)
                }
            }
            Section {
                ForEach(Array(detail.entries.enumerated()), id: \.element.id) { index, entry in
                    TrackRow(model: model, player: player, track: entry.track,
                             showDetails: { details = entry.track.id }) {
                        player.play(model.queue(tracks), from: index)
                    }
                }
                .onMove { from, to in
                    var places = detail.entries.map(\.place)
                    places.move(fromOffsets: from, toOffset: to)
                    arrange(places)
                }
                .onDelete { offsets in
                    var places = detail.entries.map(\.place)
                    places.remove(atOffsets: offsets)
                    arrange(places)
                }
            }
        }
        .overlay {
            if detail.entries.isEmpty {
                ForgeStateView.empty(
                    "No Songs Yet", systemImage: "music.note",
                    description: "Add songs from their menu, an album, or Now Playing."
                )
            }
        }
    }

    private func arrange(_ places: [UInt32]) {
        Task { if let d = await model.arrangePlaylist(id, places: places) { detail = d } }
    }
}

/// Pick a playlist to add songs to, or make a new one with them.
struct AddToPlaylistSheet: View {
    let model: ListenModel
    let trackIds: [String]
    @Environment(\.dismiss) private var dismiss
    @State private var naming = false

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Button {
                        naming = true
                    } label: {
                        Label("New Playlist", systemImage: "plus")
                    }
                    .accessibilityIdentifier("addToPlaylist.new")
                }
                Section {
                    ForEach(model.collection.playlists) { list in
                        Button {
                            Task {
                                if await model.add(trackIds, toPlaylist: list.id) != nil { dismiss() }
                            }
                        } label: {
                            PlaylistRow(list: list)
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("addToPlaylist.playlist")
                    }
                }
            }
            .navigationTitle(trackIds.count == 1 ? "Add Song to" : "Add \(trackIds.count) Songs to")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("addToPlaylist.cancel")
                }
            }
            .sheet(isPresented: $naming) {
                PlaylistNameSheet(title: "New Playlist", name: "") { name in
                    let made = await model.createPlaylist(name, with: trackIds) != nil
                    if made { dismiss() }
                    return made
                }
            }
        }
        .presentationDetents([.medium, .large])
        .forgeFeedback(model.feedback)
    }
}

/// A playlist's name, for a new one or a rename. `save` says whether it worked.
struct PlaylistNameSheet: View {
    let title: String
    @State var name: String
    let save: (String) async -> Bool
    @Environment(\.dismiss) private var dismiss
    @State private var saving = false
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            Form {
                TextField("Name", text: $name)
                    .focused($focused)
                    .submitLabel(.done)
                    .onSubmit(commit)
                    .accessibilityIdentifier("playlistName.field")
            }
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("playlistName.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save", action: commit)
                        .disabled(saving || name.trimmingCharacters(in: .whitespaces).isEmpty)
                        .accessibilityIdentifier("playlistName.save")
                }
            }
            .onAppear { focused = true }
        }
        .presentationDetents([.height(220)])
    }

    private func commit() {
        guard !saving else { return }
        saving = true
        Task {
            defer { saving = false }
            if await save(name) { dismiss() }
        }
    }
}

// MARK: Recently played

struct RecentlyPlayedView: View {
    let model: ListenModel
    let player: TakePlayer
    @State private var details: String?
    @State private var confirmingClear = false

    var body: some View {
        let recent = model.collection.recentlyPlayed
        let tracks = recent.map(\.track)
        List {
            ForEach(Array(recent.enumerated()), id: \.element.track.id) { index, play in
                VStack(alignment: .leading, spacing: 0) {
                    TrackRow(model: model, player: player, track: play.track,
                             showDetails: { details = play.track.id }) {
                        player.play(model.queue(tracks), from: index)
                    }
                    Text(Self.when(play.playedMs))
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .padding(.leading, 56)
                }
            }
        }
        .listStyle(.plain)
        .navigationTitle("Recently Played")
        .navigationDestination(item: $details) { id in
            TrackView(model: model, player: player, id: id)
        }
        .toolbar {
            if !recent.isEmpty {
                ToolbarItem(placement: .primaryAction) {
                    Button("Clear History", systemImage: "trash") { confirmingClear = true }
                        .accessibilityIdentifier("recent.clear")
                }
            }
        }
        .confirmationDialog("Clear your listening history?", isPresented: $confirmingClear,
                            titleVisibility: .visible) {
            Button("Clear History", role: .destructive) { Task { await model.clearHistory() } }
                .accessibilityIdentifier("recent.confirmClear")
        } message: {
            Text("Favourites and playlists stay.")
        }
        .overlay {
            if recent.isEmpty {
                ForgeStateView.empty(
                    "Nothing Played Yet", systemImage: "clock",
                    description: "Songs you listen to for 30 seconds show up here."
                )
            }
        }
    }

    /// "5 minutes ago".
    static func when(_ ms: Int64) -> String {
        let date = Date(timeIntervalSince1970: Double(ms) / 1000)
        return date.formatted(.relative(presentation: .named))
    }
}
#endif
