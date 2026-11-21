#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import PhotosUI
import UniformTypeIdentifiers
import SwiftUI

// MARK: The library's first screen

/// Artists, albums and songs, and search across all three. A song plays
/// when tapped (from there on in the list); its details are one tap on the
/// ellipsis away.
struct LibraryHome: View {
    let model: ListenModel
    let player: TakePlayer

    enum Shelf: String, CaseIterable, Identifiable {
        case songs = "Songs", albums = "Albums", artists = "Artists"
        var id: String { rawValue }
    }

    @AppStorage("library.shelf") private var shelf: Shelf = .songs
    @State private var search = ""
    @State private var details: String?
    /// The catalogues' answer to `query`, once it arrives.
    @State private var catalogues: CatalogueSearch?
    @State private var cataloguesProblem: String?

    private var library: Library { model.library }
    private var query: String { search.trimmingCharacters(in: .whitespaces) }

    var body: some View {
        List {
            if let problem = model.refreshProblem {
                Section {
                    Label(problem, systemImage: "exclamationmark.icloud")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("library.refreshProblem")
                }
            }
            if query.isEmpty {
                YourMusicSection(model: model, player: player, showDetails: { details = $0 })
                Section {
                    Picker("Show", selection: $shelf) {
                        ForEach(Shelf.allCases) { Text($0.rawValue).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .accessibilityIdentifier("library.shelf")
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets(top: 4, leading: 16, bottom: 4, trailing: 16))
                }
                switch shelf {
                case .songs: songs(library.tracks)
                case .albums: albums(library.albums)
                case .artists: artists(library.artists)
                }
            } else {
                results(model.search(query))
                catalogueResults
            }
        }
        .listStyle(.insetGrouped)
        .searchable(text: $search, prompt: "Your music, Jamendo and the Archive")
        .task(id: query) {
            catalogues = nil
            cataloguesProblem = nil
            guard !query.isEmpty else { return }
            // Wait for typing to pause before asking the catalogues.
            try? await Task.sleep(for: .milliseconds(500))
            guard !Task.isCancelled else { return }
            let result = await model.searchCatalogues(query)
            // A newer search cancelled this one: its late answer is dropped.
            guard !Task.isCancelled else { return }
            switch result {
            case .success(let found): catalogues = found
            case .failure(let failure):
                if !failure.stopped { cataloguesProblem = failure.message }
            }
        }
        .refreshable { await model.loadLibrary(fresh: true) }
        .navigationDestination(item: $details) { id in
            TrackView(model: model, player: player, id: id)
        }
        .toolbar {
            ToolbarItem(placement: .secondaryAction) {
                ImportMusicButton(model: model)
            }
        }
        .overlay {
            if library.tracks.isEmpty && query.isEmpty {
                ForgeStateView.empty(
                    "No Music Yet", systemImage: "music.note.list",
                    description: "Find music in Discover, import your own files, or add a take server."
                )
            } else if !query.isEmpty && model.search(query).isEmpty && catalogues?.isEmpty == true {
                ForgeStateView.noResults(for: query)
            }
        }
    }

    @ViewBuilder private func songs(_ tracks: [LibraryTrack]) -> some View {
        Section {
            PlayShuffleButtons(disabled: tracks.isEmpty) { shuffle in
                player.play(model.queue(tracks), shuffle: shuffle)
            }
        }
        Section("\(tracks.count) \(tracks.count == 1 ? "Song" : "Songs")") {
            ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                TrackRow(model: model, player: player, track: track, showDetails: { details = track.id }) {
                    player.play(model.queue(tracks), from: index)
                }
            }
        }
    }

    @ViewBuilder private func albums(_ albums: [LibraryAlbum]) -> some View {
        Section {
            ForEach(albums) { album in
                NavigationLink(value: ListenRoute.album(album)) { AlbumRow(album: album, model: model) }
                    .accessibilityIdentifier("library.album")
            }
        }
    }

    @ViewBuilder private func artists(_ artists: [LibraryArtist]) -> some View {
        Section {
            ForEach(artists) { artist in
                NavigationLink(value: ListenRoute.artist(artist)) { ArtistRow(artist: artist) }
                    .accessibilityIdentifier("library.artist")
            }
        }
    }

    @ViewBuilder private func results(_ found: LibrarySearch) -> some View {
        if !found.artists.isEmpty {
            Section("Artists") {
                ForEach(found.artists) { artist in
                    NavigationLink(value: ListenRoute.artist(artist)) { ArtistRow(artist: artist) }
                        .accessibilityIdentifier("search.artist")
                }
            }
        }
        if !found.albums.isEmpty {
            Section("Albums") {
                ForEach(found.albums) { album in
                    NavigationLink(value: ListenRoute.album(album)) { AlbumRow(album: album, model: model) }
                        .accessibilityIdentifier("search.album")
                }
            }
        }
        if !found.tracks.isEmpty {
            Section("Songs") {
                ForEach(Array(found.tracks.enumerated()), id: \.element.id) { index, track in
                    TrackRow(model: model, player: player, track: track, showDetails: { details = track.id }) {
                        player.play(model.queue(found.tracks), from: index)
                    }
                }
            }
        }
    }
}

extension LibrarySearch {
    var isEmpty: Bool { artists.isEmpty && albums.isEmpty && tracks.isEmpty }
}

extension CatalogueSearch {
    /// Nothing found and nothing to explain.
    var isEmpty: Bool {
        jamendo.isEmpty && archive.isEmpty && jamendoProblem == nil && archiveProblem == nil
    }
}

extension LibraryHome {
    /// Under the library's own results: what Jamendo and the Internet
    /// Archive found, each in its own section, or why not.
    @ViewBuilder fileprivate var catalogueResults: some View {
        if let found = catalogues {
            // A track added from here moves up to the library's results.
            let jamendo = found.jamendo.filter { !model.inLibrary($0.id) }
            if !jamendo.isEmpty || found.jamendoProblem != nil {
                Section {
                    ForEach(Array(jamendo.enumerated()), id: \.element.id) { index, track in
                        CatalogueRow(model: model, player: player, track: track) {
                            player.play(model.queue(jamendo), from: index)
                        }
                    }
                } header: {
                    Text("From Jamendo")
                } footer: {
                    if let problem = found.jamendoProblem {
                        Text(SoundscapeErrors.sentence(problem))
                            .accessibilityIdentifier("search.jamendoProblem")
                    }
                }
            }
            if !found.archive.isEmpty || found.archiveProblem != nil {
                Section {
                    ForEach(found.archive, id: \.identifier) { item in
                        NavigationLink(value: ListenRoute.archiveItem(item)) {
                            ArchiveItemRow(item: item)
                        }
                        .accessibilityIdentifier("search.archiveItem")
                    }
                } header: {
                    Text("From the Internet Archive")
                } footer: {
                    if let problem = found.archiveProblem {
                        Text(SoundscapeErrors.sentence(problem))
                            .accessibilityIdentifier("search.archiveProblem")
                    }
                }
            }
            if !jamendo.isEmpty || !found.archive.isEmpty {
                Section {
                    NavigationLink(value: ListenRoute.discoverSearch(query)) {
                        Label("More in Discover", systemImage: "sparkle.magnifyingglass")
                    }
                    .accessibilityIdentifier("search.discover")
                }
            }
        } else if let cataloguesProblem {
            Section {
                Label(cataloguesProblem, systemImage: "exclamationmark.icloud")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("search.catalogueProblem")
            }
        } else {
            Section {
                HStack(spacing: 8) {
                    ProgressView()
                    Text("Searching Jamendo and the Internet Archive")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("search.catalogueSearching")
            }
        }
    }
}

// MARK: Rows

struct PlayShuffleButtons: View {
    let disabled: Bool
    let play: (_ shuffle: Bool) -> Void

    var body: some View {
        HStack(spacing: 12) {
            button("Play", systemImage: "play.fill", identifier: "library.play") { play(false) }
            button("Shuffle", systemImage: "shuffle", identifier: "library.shuffle") { play(true) }
        }
        .listRowBackground(Color.clear)
        .listRowInsets(EdgeInsets(top: 4, leading: 16, bottom: 4, trailing: 16))
    }

    private func button(
        _ title: String, systemImage: String, identifier: String, action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Label(title, systemImage: systemImage)
                .font(.headline)
                .frame(maxWidth: .infinity, minHeight: 44)
        }
        .buttonStyle(.bordered)
        .disabled(disabled)
        .accessibilityIdentifier(identifier)
    }
}

/// A song: tap to play, the ellipsis for its details, and Play Next / Add
/// to Queue on a long press or swipe.
struct TrackRow: View {
    let model: ListenModel
    let player: TakePlayer
    let track: LibraryTrack
    var number: UInt32? = nil
    /// In a playlist: Remove from Playlist on its menu and swipe.
    var remove: (() -> Void)? = nil
    let showDetails: () -> Void
    let play: () -> Void

    @State private var addingToPlaylist = false

    private var isCurrent: Bool { player.current?.id == track.preferred?.id && !player.isComparing }

    var body: some View {
        HStack(spacing: 12) {
            Button(action: play) {
                HStack(spacing: 12) {
                    if let number {
                        Text("\(number)")
                            .font(.subheadline.monospacedDigit())
                            .foregroundStyle(.secondary)
                            .frame(minWidth: 22)
                    } else {
                        Artwork(title: track.title, url: track.artworkUrl, size: 44)
                    }
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 4) {
                            Text(track.title)
                                .font(.body)
                                .foregroundStyle(isCurrent ? Color.accentColor : .primary)
                                .lineLimit(1)
                            if model.isFavourite(track.id) {
                                Image(systemName: "heart.fill")
                                    .font(.caption2)
                                    .foregroundStyle(.pink)
                                    .accessibilityLabel("Favourite")
                            }
                            DownloadBadge(model: model, track: track)
                            if isCurrent && player.isPlaying {
                                Image(systemName: "speaker.wave.2.fill")
                                    .font(.caption)
                                    .foregroundStyle(.tint)
                                    .accessibilityLabel("Playing")
                            }
                        }
                        Text(subtitle)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(track.preferred == nil)
            .accessibilityLabel("\(track.title), \(subtitle)")
            .accessibilityHint("Plays it")
            .accessibilityIdentifier("library.song")
            Button(action: showDetails) {
                Image(systemName: "ellipsis")
                    .frame(width: 32, height: 32)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .foregroundStyle(.secondary)
            .accessibilityLabel("Details for \(track.title)")
            .accessibilityIdentifier("library.songDetails")
        }
        .contextMenu {
            Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                player.add(model.queue([track]), next: true)
            }
            .accessibilityIdentifier("library.playNext")
            Button("Add to Queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                player.add(model.queue([track]), next: false)
            }
            .accessibilityIdentifier("library.addToQueue")
            Button("Add to Playlist…", systemImage: "text.badge.plus") { addingToPlaylist = true }
                .accessibilityIdentifier("library.addToPlaylist")
            let favourite = model.isFavourite(track.id)
            Button(favourite ? "Remove from Favourites" : "Add to Favourites",
                   systemImage: favourite ? "heart.slash" : "heart") {
                Task { await model.toggleFavourite(track.id) }
            }
            .accessibilityIdentifier("library.favouriteMenu")
            DownloadMenuItem(model: model, tracks: [track], identifier: "library.downloadMenu")
            Button("Details", systemImage: "info.circle", action: showDetails)
                .accessibilityIdentifier("library.detailsMenu")
            if let remove {
                Button("Remove from Playlist", systemImage: "minus.circle", role: .destructive, action: remove)
                    .accessibilityIdentifier("playlist.removeMenu")
            }
        }
        .sheet(isPresented: $addingToPlaylist) {
            AddToPlaylistSheet(model: model, trackIds: [track.id])
        }
        .swipeActions(edge: .leading) {
            Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                player.add(model.queue([track]), next: true)
            }
            .tint(.indigo)
            .accessibilityIdentifier("library.swipePlayNext")
        }
        .swipeActions(edge: .trailing) {
            // A row's own swipes replace the list's swipe to delete, so a
            // playlist offers Remove here.
            if let remove {
                Button("Remove", systemImage: "minus.circle", role: .destructive, action: remove)
                    .accessibilityIdentifier("playlist.swipeRemove")
            }
            Button("Add to Queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                player.add(model.queue([track]), next: false)
            }
            .tint(.orange)
            .accessibilityIdentifier("library.swipeAddToQueue")
        }
    }

    private var subtitle: String {
        var parts = [number == nil ? TakeFormat.subtitle(track) : track.durationSeconds.map(TakeFormat.time) ?? ""]
        if let n = TakeFormat.recordings(track.recordingCount) { parts.append(n) }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

struct AlbumRow: View {
    let album: LibraryAlbum
    /// Gives the row a long-press menu with Download.
    var model: ListenModel? = nil

    var body: some View {
        HStack(spacing: 12) {
            Artwork(title: album.title, url: album.artworkUrl, size: 56)
            VStack(alignment: .leading, spacing: 2) {
                Text(album.title).font(.headline).lineLimit(1)
                Text("\(album.artist) · \(album.trackCount) \(album.trackCount == 1 ? "song" : "songs")")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .accessibilityElement(children: .combine)
        .contextMenu {
            if let model {
                DownloadMenuItem(model: model, tracks: model.tracks(of: album), identifier: "library.albumDownload")
            }
        }
    }
}

struct ArtistRow: View {
    let artist: LibraryArtist

    var body: some View {
        HStack(spacing: 12) {
            Artwork(title: artist.name, url: artist.artworkUrl, size: 48, circle: true)
            VStack(alignment: .leading, spacing: 2) {
                Text(artist.name).font(.headline).lineLimit(1)
                Text(TakeFormat.artistSummary(artist))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

// MARK: An artist

struct ArtistView: View {
    let model: ListenModel
    let player: TakePlayer
    let artist: LibraryArtist

    @State private var editingArtwork = false
    @State private var details: String?

    /// The artist as the library has it now (artwork may have changed).
    private var current: LibraryArtist { model.library.artists.first { $0.id == artist.id } ?? artist }

    var body: some View {
        let albums = model.albums(of: current)
        let tracks = model.tracks(of: current)
        List {
            Section {
                VStack(spacing: 12) {
                    Artwork(title: current.name, url: current.artworkUrl, size: 180, circle: true)
                        .shadow(radius: 8, y: 4)
                    Text(current.name).font(.title.bold()).multilineTextAlignment(.center)
                    Text(TakeFormat.artistSummary(current)).font(.subheadline).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
            }
            Section {
                PlayShuffleButtons(disabled: tracks.isEmpty) { shuffle in
                    player.play(model.queue(tracks), shuffle: shuffle)
                }
            }
            if !albums.isEmpty {
                Section("Albums") {
                    ForEach(albums) { album in
                        NavigationLink(value: ListenRoute.album(album)) { AlbumRow(album: album, model: model) }
                            .accessibilityIdentifier("artist.album")
                    }
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
        .miniPlayerClearance()
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
}

// MARK: An album

struct AlbumView: View {
    let model: ListenModel
    let player: TakePlayer
    let album: LibraryAlbum

    @State private var editingArtwork = false
    @State private var details: String?
    @State private var addingToPlaylist = false

    private var current: LibraryAlbum { model.library.albums.first { $0.id == album.id } ?? album }
    private var artist: LibraryArtist? { model.library.artists.first { $0.id == current.artistId } }

    var body: some View {
        let tracks = model.tracks(of: current)
        List {
            Section {
                VStack(spacing: 10) {
                    Artwork(title: current.title, url: current.artworkUrl, size: 240)
                        .shadow(radius: 10, y: 5)
                    Text(current.title).font(.title2.bold()).multilineTextAlignment(.center)
                    if let artist {
                        NavigationLink(value: ListenRoute.artist(artist)) {
                            Text(artist.name).font(.title3).foregroundStyle(.tint)
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("album.artist")
                    } else {
                        Text(current.artist).font(.title3).foregroundStyle(.secondary)
                    }
                    Text(albumLength(tracks)).font(.caption).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
            }
            Section {
                PlayShuffleButtons(disabled: tracks.isEmpty) { shuffle in
                    player.play(model.queue(tracks), shuffle: shuffle)
                }
                AlbumDownloadButton(model: model, tracks: tracks)
            }
            Section {
                ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                    TrackRow(
                        model: model, player: player, track: track,
                        number: track.number ?? UInt32(index + 1),
                        showDetails: { details = track.id }
                    ) {
                        player.play(model.queue(tracks), from: index)
                    }
                }
            }
        }
        .miniPlayerClearance()
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
                    Button("Add to Playlist…", systemImage: "text.badge.plus") { addingToPlaylist = true }
                        .accessibilityIdentifier("album.addToPlaylist")
                    DownloadMenuItem(model: model, tracks: model.tracks(of: current), identifier: "album.download")
                    Button("Artwork", systemImage: "photo") { editingArtwork = true }
                        .accessibilityIdentifier("album.artwork")
                }
                .accessibilityIdentifier("album.more")
            }
        }
        .sheet(isPresented: $addingToPlaylist) {
            AddToPlaylistSheet(model: model, trackIds: model.tracks(of: current).map(\.id))
        }
        .forgeFeedback(model.feedback)
        .sheet(isPresented: $editingArtwork) {
            ArtworkSheet(model: model, title: current.title, url: current.artworkUrl,
                         target: .album(id: current.id), circle: false)
        }
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

// MARK: A track

/// One track: its details, Play, and every recording of it. The preferred
/// recording is what plays; Use This Recording makes another one preferred
/// (the take server's pick). Edit changes its details and artwork.
struct TrackView: View {
    let model: ListenModel
    let player: TakePlayer
    let id: String

    @State private var detail: TrackDetail?
    @State private var editing = false
    @State private var addingToPlaylist = false
    @State private var showingLyrics = false
    @State private var confirmingRemove = false
    @State private var identifying = false
    @State private var noting: Recording?
    @State private var noteText = ""
    @AppStorage("track.recordingOrder") private var orderName = "best"
    @Environment(\.dismiss) private var dismiss

    private var order: RecordingOrder {
        switch orderName {
        case "newest": .newest
        case "oldest": .oldest
        default: .best
        }
    }

    var body: some View {
        Group {
            if let detail {
                content(detail)
            } else {
                ForgeStateView.empty(
                    "Not in the Library", systemImage: "music.note",
                    description: "This song has gone from the take server. Pull down on the library to refresh it."
                )
            }
        }
        .miniPlayerClearance()
        .navigationTitle(detail?.track.title ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { detail = model.track(id, order: order) }
        .onChange(of: model.library) { detail = model.track(id, order: order) }
        .onChange(of: order) { detail = model.track(id, order: order) }
        .alert("Note", isPresented: Binding(get: { noting != nil }, set: { if !$0 { noting = nil } })) {
            TextField("What stands out", text: $noteText)
                .accessibilityIdentifier("recording.noteText")
            Button("Save") {
                if let recording = noting {
                    Task { if let d = await model.note(recording, of: id, noteText) { detail = d } }
                }
            }
            .accessibilityIdentifier("recording.noteSave")
            Button("Cancel", role: .cancel) {}
                .accessibilityIdentifier("recording.noteCancel")
        }
        .forgeFeedback(model.feedback)
        .toolbar {
            if detail != nil {
                ToolbarItem(placement: .primaryAction) {
                    FavouriteButton(model: model, id: id, identifier: "track.favourite")
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("Edit") { editing = true }
                        .accessibilityIdentifier("track.edit")
                }
            }
        }
        .sheet(isPresented: $addingToPlaylist) {
            AddToPlaylistSheet(model: model, trackIds: [id])
        }
        .sheet(isPresented: $showingLyrics) {
            if let detail, let lyrics = detail.track.preferred?.lyrics ?? detail.lyrics {
                LyricsView(title: detail.track.title, subtitle: detail.track.artist, lyrics: lyrics)
            }
        }
        .sheet(isPresented: $editing) {
            if let detail {
                EditTrackSheet(model: model, track: detail.track) { updated in self.detail = updated }
            }
        }
        .sheet(isPresented: $identifying) {
            if let detail {
                IdentifySongSheet(model: model, track: detail.track) { updated in self.detail = updated }
            }
        }
        .confirmationDialog("Remove this song from your library?", isPresented: $confirmingRemove,
                            titleVisibility: .visible) {
            Button("Remove from Library", role: .destructive) {
                Task {
                    await model.removeImported(id)
                    dismiss()
                }
            }
            .accessibilityIdentifier("track.confirmRemove")
        } message: {
            if detail?.track.preferred?.kind == .catalogue {
                Text("It stays in the catalogue, where you can find it again in Discover.")
            } else {
                Text("Soundscape's copy is deleted. The file you imported it from isn't touched.")
            }
        }
    }

    private func content(_ detail: TrackDetail) -> some View {
        let track = detail.track
        let artist = model.library.artists.first { $0.id == track.artistId }
        let album = model.library.albums.first { $0.id == track.albumId }
        return List {
            Section {
                VStack(spacing: 10) {
                    Artwork(title: track.title, url: track.artworkUrl, size: 220)
                        .shadow(radius: 10, y: 5)
                    Text(track.title).font(.title2.bold()).multilineTextAlignment(.center)
                    HStack(spacing: 6) {
                        if let artist {
                            NavigationLink(value: ListenRoute.artist(artist)) { Text(artist.name) }
                                .accessibilityIdentifier("track.artist")
                        } else {
                            Text(track.artist)
                        }
                        if let album {
                            Text("·").foregroundStyle(.secondary)
                            NavigationLink(value: ListenRoute.album(album)) { Text(album.title) }
                                .accessibilityIdentifier("track.album")
                        }
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.tint)
                    let facts = [track.genre, track.durationSeconds.map(TakeFormat.time)].compactMap { $0 }
                    if !facts.isEmpty {
                        Text(facts.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
            }
            Section {
                HStack(spacing: 12) {
                    Button {
                        player.play(model.queue([track]))
                    } label: {
                        Label("Play", systemImage: "play.fill")
                            .font(.headline)
                            .frame(maxWidth: .infinity, minHeight: 44)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(track.preferred == nil)
                    .accessibilityIdentifier("track.play")
                    Button {
                        player.add(model.queue([track]), next: true)
                    } label: {
                        Label("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward")
                            .font(.headline)
                            .frame(maxWidth: .infinity, minHeight: 44)
                    }
                    .buttonStyle(.bordered)
                    .disabled(track.preferred == nil)
                    .accessibilityIdentifier("track.playNext")
                }
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets(top: 4, leading: 16, bottom: 4, trailing: 16))
                Button {
                    addingToPlaylist = true
                } label: {
                    Label("Add to Playlist", systemImage: "text.badge.plus")
                }
                .accessibilityIdentifier("track.addToPlaylist")
                if !model.downloadable([track]).isEmpty {
                    downloadButton(track)
                }
                if track.preferred?.kind == .imported || track.preferred?.kind == .catalogue {
                    Button(role: .destructive) {
                        confirmingRemove = true
                    } label: {
                        Label("Remove from Library", systemImage: "trash")
                    }
                    .accessibilityIdentifier("track.remove")
                }
            }
            Section {
                ForEach(detail.recordings) { recording in
                    RecordingRow(
                        recording: recording,
                        playing: player.current?.id == recording.id && player.isPlaying && !player.isComparing,
                        play: {
                            if player.current?.id == recording.id && !player.isComparing {
                                player.toggle()
                            } else if let item = model.queue(recording, of: track) {
                                player.play([item])
                            }
                        },
                        choose: {
                            Task { if let d = await model.choose(recording, of: track) { self.detail = d } }
                        },
                        rate: rater(recording, of: track),
                        note: noter(recording)
                    )
                }
            } header: {
                HStack {
                    Text(detail.recordings.count == 1 ? "Recording" : "Recordings")
                    Spacer()
                    if detail.recordings.count > 1 {
                        Picker("Order", selection: $orderName) {
                            Text("Best").tag("best")
                            Text("Newest").tag("newest")
                            Text("Oldest").tag("oldest")
                        }
                        .pickerStyle(.menu)
                        .textCase(nil)
                        .accessibilityIdentifier("track.recordingOrder")
                    }
                }
            } footer: {
                if detail.recordings.count > 1 {
                    Text(track.picked
                         ? "The picked recording plays. Your pick is saved on the take server."
                         : "Nothing is picked yet, so the best rated (or newest) recording plays.")
                }
            }
            if let recording = track.preferred, let rights = recording.rights {
                LicenceSection(recording: recording, rights: rights)
            }
            musicBrainzSection(detail)
            if let slug = track.slug {
                Section {
                    NavigationLink(value: ListenRoute.takes(slug)) {
                        Label("Compare Takes", systemImage: "rectangle.split.2x1")
                    }
                    .accessibilityIdentifier("track.takes")
                } footer: {
                    Text("Hear two takes A/B at matched loudness, blind if you like, and pick one.")
                }
            }
            if detail.caption != nil || detail.lyrics != nil {
                Section("How it was made") {
                    if let caption = detail.caption {
                        DisclosureGroup("Caption") {
                            Text(caption).font(.callout).textSelection(.enabled)
                        }
                        .accessibilityIdentifier("track.caption")
                    }
                    if let lyrics = track.preferred?.lyrics ?? detail.lyrics,
                       !lyricLines(lyrics: lyrics).isEmpty {
                        Button {
                            showingLyrics = true
                        } label: {
                            Label("Lyrics", systemImage: "quote.bubble")
                        }
                        .accessibilityIdentifier("track.lyrics")
                    }
                }
            }
        }
    }
}

extension TrackView {
    /// The MusicBrainz recording the song is linked to, or Identify Song;
    /// and the same recording from other sources in the library.
    @ViewBuilder fileprivate func musicBrainzSection(_ detail: TrackDetail) -> some View {
        let track = detail.track
        Section {
            if let mbid = track.musicbrainzId {
                if let url = URL(string: "https://musicbrainz.org/recording/\(mbid)") {
                    Link(destination: url) {
                        Label("Open in MusicBrainz", systemImage: "safari")
                    }
                    .accessibilityIdentifier("track.musicbrainz")
                }
                Button {
                    identifying = true
                } label: {
                    Label("Identify Again", systemImage: "magnifyingglass")
                }
                .accessibilityIdentifier("track.identify")
                Button(role: .destructive) {
                    Task { if let d = await model.unlink(track) { self.detail = d } }
                } label: {
                    Label("Unlink from MusicBrainz", systemImage: "link.badge.minus")
                }
                .accessibilityIdentifier("track.unlink")
            } else {
                Button {
                    identifying = true
                } label: {
                    Label("Identify Song", systemImage: "magnifyingglass")
                }
                .accessibilityIdentifier("track.identify")
            }
        } header: {
            Text("MusicBrainz")
        } footer: {
            Text(track.musicbrainzId == nil
                 ? "Look the song up in MusicBrainz, the open music encyclopedia, to fix its details and cover and to recognise it from other sources."
                 : "Linked. Other copies of this recording in your library show below.")
        }
        if !detail.sameRecording.isEmpty {
            Section("Also in Your Library") {
                ForEach(detail.sameRecording, id: \.id) { other in
                    NavigationLink(value: ListenRoute.track(other.id)) {
                        LabeledContent(other.title, value: Self.sourceName(other))
                    }
                    .accessibilityIdentifier("track.sameRecording")
                }
            }
        }
    }

    /// Where a song plays from, in words.
    static func sourceName(_ track: LibraryTrack) -> String {
        switch track.preferred?.source {
        case .catalogue(let provider): provider
        case .localFile: "Your file"
        case .takeServer: "Take server"
        case nil: "Unavailable"
        }
    }

    /// Rating, for a take on the take server only.
    fileprivate func rater(_ recording: Recording, of track: LibraryTrack) -> ((Int) -> Void)? {
        guard recording.kind == .generated else { return nil }
        return { n in
            Task { if let d = await model.rate(recording, of: track.id, stars: n) { detail = d } }
        }
    }

    fileprivate func noter(_ recording: Recording) -> (() -> Void)? {
        guard recording.kind == .generated else { return nil }
        return {
            noteText = recording.note
            noting = recording
        }
    }

    /// Download, with how far along it is, or Remove Download.
    @ViewBuilder fileprivate func downloadButton(_ track: LibraryTrack) -> some View {
        let state = model.downloadState(track.id)
        Button {
            Task { await model.keepOnPhone([track], state == .notDownloaded) }
        } label: {
            switch state {
            case .notDownloaded:
                Label("Download", systemImage: "arrow.down.circle")
            case .waiting:
                Label("Waiting to Download", systemImage: "arrow.down.circle.dotted")
            case .downloading:
                Label("Downloading…", systemImage: "arrow.down.circle.dotted")
            case .downloaded:
                Label("Remove Download", systemImage: "minus.circle")
            case .failed(let reason):
                Label("Download failed: \(reason). Tap to stop trying.", systemImage: "exclamationmark.circle")
            }
        }
        .accessibilityIdentifier("track.download")
    }
}

/// A catalogue track's licence and credit, and what it allows, kept with
/// the track when it was added.
private struct LicenceSection: View {
    let recording: Recording
    let rights: Rights

    private var provider: String {
        if case .catalogue(let provider) = recording.source { return provider }
        return "the catalogue"
    }

    var body: some View {
        Section {
            if let url = rights.licenceUrl.flatMap(URL.init(string:)) {
                Link(destination: url) {
                    LabeledContent("Licence", value: rights.licence)
                }
                .accessibilityIdentifier("track.licence")
            } else {
                LabeledContent("Licence", value: rights.licence)
            }
            LabeledContent("Offline", value: rights.mayDownload ? "Allowed" : "Not allowed")
            LabeledContent("Changes and remixes", value: rights.mayAdapt ? "Allowed" : "Not allowed")
            if rights.nonCommercial {
                LabeledContent("Use", value: "Non-commercial only")
            }
            if let page = rights.pageUrl.flatMap(URL.init(string:)) {
                Link(destination: page) {
                    Label("Open on \(provider)", systemImage: "safari")
                }
                .accessibilityIdentifier("track.openSource")
            }
        } header: {
            Text("From \(provider)")
        } footer: {
            Text(rights.attribution)
                .textSelection(.enabled)
        }
    }
}

private struct RecordingRow: View {
    let recording: Recording
    let playing: Bool
    let play: () -> Void
    let choose: () -> Void
    /// Rating and notes, for a take on the take server.
    var rate: ((Int) -> Void)? = nil
    var note: (() -> Void)? = nil

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Button(action: play) {
                Image(systemName: playing ? "pause.circle.fill" : "play.circle.fill")
                    .font(.system(size: 30))
            }
            .buttonStyle(.borderless)
            .accessibilityLabel(playing ? "Pause \(recording.label)" : "Play \(recording.label)")
            .accessibilityIdentifier("recording.play")
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(recording.label).font(.headline)
                    if recording.preferred {
                        Label("Plays for this song", systemImage: "checkmark.circle.fill")
                            .labelStyle(.iconOnly)
                            .foregroundStyle(.tint)
                    }
                }
                let facts = [
                    TakeFormat.generation(recording),
                    recording.durationSeconds.map(TakeFormat.time),
                    rate == nil && recording.stars > 0 ? String(repeating: "★", count: Int(recording.stars)) : nil,
                ].compactMap { $0 }.filter { !$0.isEmpty }
                if !facts.isEmpty {
                    Text(facts.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
                }
                if !recording.note.isEmpty {
                    Text(recording.note).font(.callout).foregroundStyle(.secondary)
                }
                if let rate {
                    HStack(spacing: 12) {
                        Stars(value: Int(recording.stars), identifier: "recording", rate: rate)
                        if let note {
                            Button(recording.note.isEmpty ? "Add Note" : "Edit Note", action: note)
                                .font(.subheadline)
                                .buttonStyle(.borderless)
                                .accessibilityIdentifier("recording.note")
                        }
                    }
                }
                if !recording.preferred {
                    Button("Use This Recording", action: choose)
                        .font(.subheadline)
                        .buttonStyle(.borderless)
                        .accessibilityIdentifier("recording.use")
                }
            }
        }
        .padding(.vertical, 2)
    }
}

// MARK: Editing

/// A track's title, artist, album, number and genre, and its own artwork.
/// A field left empty goes back to what the take server says.
private struct EditTrackSheet: View {
    let model: ListenModel
    let track: LibraryTrack
    let saved: (TrackDetail) -> Void
    @Environment(\.dismiss) private var dismiss

    @State private var title = ""
    @State private var artist = ""
    @State private var album = ""
    @State private var number = ""
    @State private var genre = ""
    @State private var saving = false
    @State private var editingArtwork = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Title", text: $title)
                        .accessibilityIdentifier("editTrack.title")
                    TextField("Artist", text: $artist)
                        .textInputAutocapitalization(.words)
                        .accessibilityIdentifier("editTrack.artist")
                    TextField("Album", text: $album)
                        .textInputAutocapitalization(.words)
                        .accessibilityIdentifier("editTrack.album")
                    TextField("Track number", text: $number)
                        .keyboardType(.numberPad)
                        .accessibilityIdentifier("editTrack.number")
                    TextField("Genre", text: $genre)
                        .textInputAutocapitalization(.words)
                        .accessibilityIdentifier("editTrack.genre")
                } footer: {
                    Text("Changes stay on this phone and survive a refresh. Clear a field to use the take server's.")
                }
                Section {
                    Button {
                        editingArtwork = true
                    } label: {
                        HStack {
                            Artwork(title: track.title, url: track.artworkUrl, size: 44)
                            Text("Song Artwork")
                        }
                    }
                    .accessibilityIdentifier("editTrack.artwork")
                }
            }
            .navigationTitle("Edit Song")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("editTrack.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save", action: save)
                        .disabled(saving)
                        .accessibilityIdentifier("editTrack.save")
                }
            }
            .sheet(isPresented: $editingArtwork) {
                ArtworkSheet(model: model, title: track.title, url: track.artworkUrl,
                             target: .track(id: track.id), circle: false)
            }
        }
        .onAppear {
            title = track.title
            artist = track.artistId == nil ? "" : track.artist
            album = track.album ?? ""
            number = track.number.map(String.init) ?? ""
            genre = track.genre ?? ""
        }
    }

    private func save() {
        // Only what changed, so an untouched field keeps following the server.
        func changed(_ text: String, _ was: String) -> String? {
            let t = text.trimmingCharacters(in: .whitespaces)
            return t == was ? nil : t
        }
        let edit = TrackEdit(
            title: changed(title, track.title),
            artist: changed(artist, track.artistId == nil ? "" : track.artist),
            album: changed(album, track.album ?? ""),
            number: changed(number, track.number.map(String.init) ?? "").map { UInt32($0) ?? 0 },
            genre: changed(genre, track.genre ?? "")
        )
        saving = true
        Task {
            defer { saving = false }
            if let detail = await model.edit(track, edit) {
                saved(detail)
                dismiss()
            }
        }
    }
}

/// Choose a picture from Photos for an artist, album or song, or go back
/// to the take server's (or the stand-in).
private struct ArtworkSheet: View {
    let model: ListenModel
    let title: String
    let url: String?
    let target: ArtworkTarget
    let circle: Bool
    @Environment(\.dismiss) private var dismiss

    @State private var item: PhotosPickerItem?
    @State private var working = false
    @State private var problem: String?

    var body: some View {
        NavigationStack {
            VStack(spacing: 24) {
                Artwork(title: title, url: url, size: 220, circle: circle)
                    .shadow(radius: 8, y: 4)
                    .overlay { if working { ProgressView() } }
                PhotosPicker(selection: $item, matching: .images) {
                    Label("Choose a Picture", systemImage: "photo.on.rectangle")
                        .frame(maxWidth: .infinity, minHeight: 44)
                }
                .buttonStyle(.borderedProminent)
                .disabled(working)
                .accessibilityIdentifier("artwork.choose")
                Button("Use the Default", role: .destructive) {
                    Task {
                        await model.clearArtwork(target)
                        dismiss()
                    }
                }
                .disabled(working)
                .accessibilityIdentifier("artwork.clear")
                if let problem {
                    Text(problem).font(.callout).forgeForeground(.danger)
                }
                Spacer()
            }
            .padding()
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("artwork.done")
                }
            }
        }
        .presentationDetents([.medium, .large])
        .onChange(of: item) { _, picked in
            guard let picked else { return }
            Task { await use(picked) }
        }
    }

    /// Copies the picture to a temporary file Rust can take in.
    private func use(_ picked: PhotosPickerItem) async {
        working = true
        defer { working = false }
        do {
            guard let data = try await picked.loadTransferable(type: Data.self) else {
                problem = "That picture couldn't be read."
                return
            }
            let ext = picked.supportedContentTypes.first?.preferredFilenameExtension ?? "jpg"
            let file = FileManager.default.temporaryDirectory
                .appendingPathComponent("artwork-\(UUID().uuidString)")
                .appendingPathExtension(ext)
            try data.write(to: file)
            defer { try? FileManager.default.removeItem(at: file) }
            await model.setArtwork(target, path: file.path)
            dismiss()
        } catch {
            problem = error.localizedDescription
        }
    }
}

/// Looks a song up in MusicBrainz and links it to the recording the
/// person picks, with or without its details. Nothing changes until a
/// match is picked.
struct IdentifySongSheet: View {
    let model: ListenModel
    let track: LibraryTrack
    let done: (TrackDetail) -> Void

    @State private var found: [MusicBrainzMatch]?
    @State private var problem: String?
    @State private var attempt = 0
    @State private var choosing: MusicBrainzMatch?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    LabeledContent("Looking for", value: track.title)
                    LabeledContent("By", value: track.artist)
                    if let seconds = track.durationSeconds {
                        LabeledContent("Length", value: TakeFormat.time(seconds))
                    }
                }
                if let found, !found.isEmpty {
                    Section {
                        ForEach(found, id: \.recordingId) { match in
                            Button {
                                choosing = match
                            } label: {
                                MatchRow(match: match, linked: match.recordingId == track.musicbrainzId)
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("identify.match")
                        }
                    } footer: {
                        Text("From MusicBrainz, the open music encyclopedia. Likely matches have the same title, artist and length.")
                    }
                }
            }
            .navigationTitle("Identify Song")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("identify.cancel")
                }
            }
            .task(id: attempt) {
                switch await model.identify(track) {
                case .success(let matches):
                    found = matches
                    problem = nil
                case .failure(let failure):
                    if !failure.stopped { problem = failure.message }
                }
            }
            .overlay {
                if let problem, found == nil {
                    ForgeStateView.failed(
                        "Can't Ask MusicBrainz", message: problem,
                        retry: ForgeStateAction("Try Again", identifier: "identify.retry") { attempt += 1 }
                    )
                } else if found == nil {
                    ForgeStateView.loading("Asking MusicBrainz")
                } else if found?.isEmpty == true {
                    ForgeStateView.empty(
                        "No Match", systemImage: "questionmark.circle",
                        description: "MusicBrainz knows no recording by this title. Fix the title or artist with Edit, then try again."
                    )
                }
            }
            .confirmationDialog(
                choosing.map { "Link to \u{201C}\($0.title)\u{201D}?" } ?? "",
                isPresented: Binding(get: { choosing != nil }, set: { if !$0 { choosing = nil } }),
                titleVisibility: .visible
            ) {
                if let match = choosing {
                    Button("Link and Use Its Details") { link(match, useDetails: true) }
                        .accessibilityIdentifier("identify.linkAndUse")
                    Button("Link Only") { link(match, useDetails: false) }
                        .accessibilityIdentifier("identify.linkOnly")
                }
            } message: {
                Text("Its details are its title, artist, album and track number; Edit can change them back. Its cover shows when the song has no picture of its own.")
            }
        }
    }

    private func link(_ match: MusicBrainzMatch, useDetails: Bool) {
        Task {
            if let detail = await model.link(track, to: match, useDetails: useDetails) {
                done(detail)
                dismiss()
            }
        }
    }
}

/// One MusicBrainz recording: what it is called, where it is from, and
/// how likely it is to be the song.
private struct MatchRow: View {
    let match: MusicBrainzMatch
    let linked: Bool

    var body: some View {
        HStack(spacing: 12) {
            Artwork(title: match.title, url: match.artworkUrl, size: 48)
            VStack(alignment: .leading, spacing: 2) {
                Text(match.title).font(.body)
                if let artist = match.artist {
                    Text(artist).font(.subheadline).foregroundStyle(.secondary)
                }
                if let note = match.disambiguation {
                    Text(note).font(.caption).foregroundStyle(.secondary)
                } else if match.live {
                    Text("Live").font(.caption).foregroundStyle(.secondary)
                }
                let facts = [match.album, match.year, match.durationSeconds.map(TakeFormat.time)]
                    .compactMap { $0 }
                if !facts.isEmpty {
                    Text(facts.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer()
            if linked {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(.tint)
                    .accessibilityLabel("Linked")
            } else if match.likely {
                Text("Likely").font(.caption.bold()).foregroundStyle(.tint)
            }
        }
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}
#endif
