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
            NavigationLink(value: ListenRoute.discover) {
                row("Discover", systemImage: "globe", tint: .blue, count: nil)
            }
            .accessibilityIdentifier("library.discover")
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
            let toPick = model.toPick.count
            if toPick > 0 {
                NavigationLink(value: ListenRoute.toPick) {
                    row("To Pick", systemImage: "checkmark.circle.badge.questionmark", tint: .teal, count: toPick)
                }
                .accessibilityIdentifier("library.toPick")
            }
            if !model.duplicates.isEmpty {
                NavigationLink(value: ListenRoute.duplicates) {
                    row("Same Song, Several Sources", systemImage: "square.on.square", tint: .purple,
                        count: model.duplicates.count)
                }
                .accessibilityIdentifier("library.duplicates")
            }
            NavigationLink(value: ListenRoute.downloads) {
                row("Downloaded", systemImage: "arrow.down.circle.fill", tint: .green,
                    count: Int(model.offline.downloaded))
            }
            .accessibilityIdentifier("library.downloads")
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

    private func row(_ title: LocalizedStringKey, systemImage: String, tint: ForgeAccent, count: Int?) -> some View {
        ForgeRow(title, systemImage: systemImage, accent: tint) {
            if let count, count > 0 {
                Text(count, format: .number)
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
                        NavigationLink(value: ListenRoute.album(album)) { AlbumRow(album: album, model: model) }
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
                    .forgeRowActions {
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
        .forgeRename("New Playlist", isPresented: $naming, name: "", id: "playlistName", confirmTitle: "Create") { name in
            _ = try await model.createPlaylist(name)
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
    @State private var confirmingDelete = false
    @Environment(\.dismiss) private var dismiss

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
                        if let detail {
                            DownloadMenuItem(model: model, tracks: detail.entries.map(\.track),
                                             identifier: "playlist.download")
                        }
                        Divider()
                        Button("Delete Playlist", systemImage: "trash", role: .destructive) {
                            confirmingDelete = true
                        }
                        .accessibilityIdentifier("playlist.delete")
                    }
                    .accessibilityIdentifier("playlist.more")
                }
                ToolbarItem(placement: .primaryAction) {
                    EditButton().accessibilityIdentifier("playlist.edit")
                }
            }
        }
        .forgeRename("Rename Playlist", isPresented: $renaming, name: detail?.playlist.name ?? "", id: "playlistName") { name in
            _ = try await model.renamePlaylist(id, to: name)
        }
        .confirmationDialog("Delete \(detail?.playlist.name ?? "this playlist")?", isPresented: $confirmingDelete,
                            titleVisibility: .visible) {
            Button("Delete Playlist", role: .destructive) {
                Task {
                    await model.deletePlaylist(id)
                    dismiss()
                }
            }
            .accessibilityIdentifier("playlist.confirmDelete")
        } message: {
            Text("The songs stay in your library.")
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
                ForEach(Array(Self.keyed(detail.entries).enumerated()), id: \.element.key) { index, row in
                    let entry = row.entry
                    TrackRow(model: model, player: player, track: entry.track,
                             remove: { removeEntry(at: index, of: detail) },
                             showDetails: { details = entry.track.id }) {
                        player.play(model.queue(tracks), from: index)
                    }
                }
                .onMove { from, to in
                    var entries = detail.entries
                    entries.move(fromOffsets: from, toOffset: to)
                    show(entries, of: detail)
                }
                .onDelete { offsets in
                    var entries = detail.entries
                    entries.remove(atOffsets: offsets)
                    show(entries, of: detail)
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

    private func removeEntry(at index: Int, of detail: PlaylistDetail) {
        var entries = detail.entries
        guard entries.indices.contains(index) else { return }
        entries.remove(at: index)
        show(entries, of: detail)
    }

    /// Shows the new order at once, so a moved row lands where it was
    /// dropped instead of leaving a gap while Rust saves, then saves it.
    private func show(_ entries: [PlaylistEntry], of detail: PlaylistDetail) {
        self.detail = PlaylistDetail(playlist: detail.playlist, entries: entries)
        arrange(entries.map(\.place))
    }

    /// Rows keyed by song and how many times it came before, not by place:
    /// places are renumbered after every change, which made SwiftUI treat
    /// each row as new and blank the moved one for a moment.
    static func keyed(_ entries: [PlaylistEntry]) -> [(key: String, entry: PlaylistEntry)] {
        var seen: [String: Int] = [:]
        return entries.map { entry in
            let n = seen[entry.track.id, default: 0]
            seen[entry.track.id] = n + 1
            return (key: "\(entry.track.id)#\(n)", entry: entry)
        }
    }

    private func arrange(_ places: [UInt32]) {
        Task { if let d = await model.arrangePlaylist(id, places: places) { detail = d } }
    }
}

/// Songs with several takes and nothing picked yet: each opens on its
/// recordings, to rate, compare and choose one.
struct ToPickView: View {
    let model: ListenModel
    let player: TakePlayer

    @State private var details: String?

    var body: some View {
        let tracks = model.toPick
        List {
            Section {
                ForEach(tracks) { track in
                    TrackRow(model: model, player: player, track: track,
                             showDetails: { details = track.id }) {
                        details = track.id
                    }
                }
            } footer: {
                if !tracks.isEmpty {
                    Text("Until you pick, the best rated (or newest) take plays. Your pick is saved on the take server.")
                }
            }
        }
        .navigationTitle("To Pick")
        .navigationDestination(item: $details) { id in
            TrackView(model: model, player: player, id: id)
        }
        .overlay {
            if tracks.isEmpty {
                ForgeStateView.empty(
                    "All Picked", systemImage: "checkmark.circle",
                    description: "Every song with more than one take has a pick."
                )
            }
        }
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
            .forgeRename("New Playlist", isPresented: $naming, name: "", id: "playlistName", confirmTitle: "Create") { name in
                _ = try await model.createPlaylist(name, with: trackIds)
                dismiss()
            }
        }
        .presentationDetents([.medium, .large])
        .forgeFeedback(model.feedback)
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

/// Songs that look like one song reaching the library from several sources
/// (the take server, your files, Jamendo, the Internet Archive). Merging
/// makes one song with every source's recordings; Keep Apart stops the
/// offer. Nothing merges by itself.
struct DuplicatesView: View {
    let model: ListenModel
    let player: TakePlayer

    @State private var details: String?

    var body: some View {
        let groups = model.duplicates
        List {
            ForEach(groups, id: \.self) { group in
                Section {
                    ForEach(Array(group.tracks.enumerated()), id: \.element.id) { index, track in
                        TrackRow(model: model, player: player, track: track,
                                 showDetails: { details = track.id }) {
                            player.play(model.queue(group.tracks), from: index)
                        }
                    }
                    if let keep = group.tracks.first {
                        Button {
                            Task { _ = await model.merge(group.tracks.dropFirst().map(\.id), into: keep.id) }
                        } label: {
                            Label("Merge into One Song", systemImage: "square.on.square")
                        }
                        .accessibilityIdentifier("duplicates.merge")
                    }
                    Button {
                        Task { await model.keepApart(group.tracks.map(\.id)) }
                    } label: {
                        Label("Keep Apart", systemImage: "square.split.2x1")
                    }
                    .accessibilityIdentifier("duplicates.keepApart")
                } header: {
                    Text(group.tracks.first?.title ?? "")
                } footer: {
                    Text(Self.why(group))
                }
            }
        }
        .navigationTitle("Same Song")
        .navigationDestination(item: $details) { id in
            TrackView(model: model, player: player, id: id)
        }
        .overlay {
            if groups.isEmpty {
                ForgeStateView.empty(
                    "Nothing to Merge", systemImage: "checkmark.circle",
                    description: "No songs in your library look like the same song from different places."
                )
            }
        }
        .forgeFeedback(model.feedback)
    }

    static func why(_ group: DuplicateSongs) -> String {
        let keep = group.tracks.first.map { "\(TrackView.sourceName($0))\u{2019}s details are kept" } ?? ""
        switch group.reason {
        case .sameRecording:
            return "Linked to the same MusicBrainz recording. \(keep); every recording stays, and your own file plays when there is one."
        case .sameDetails:
            return "Same title, artist and length. \(keep); every recording stays, and your own file plays when there is one."
        }
    }
}
#endif

#if os(iOS)
// MARK: Lyrics

/// A song's lyrics to read along: section headings ("Verse", "Chorus") in
/// small capitals, stanzas apart. With timed lyrics and the song playing
/// (`player`), the line being sung is bright and stays in view, and with
/// word timings each word lights up as it is sung; tap a line to go to it.
struct LyricsView: View {
    let title: String
    let subtitle: String?
    let lyrics: String
    /// Playing these lyrics' recording now, to follow along.
    var player: TakePlayer? = nil
    @Environment(\.dismiss) private var dismiss

    @State private var lines: [LyricLine] = []
    @State private var currentLine: Int?
    @State private var currentWord: Int?

    private var timed: Bool { player != nil && lines.contains { $0.atSeconds != nil } }

    var body: some View {
        NavigationStack {
            ScrollViewReader { scroller in
                ScrollView {
                    VStack(alignment: .leading, spacing: timed ? 10 : 6) {
                        ForEach(Array(lines.enumerated()), id: \.offset) { index, line in
                            row(line, index: index).id(index)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 24)
                    .padding(.vertical, 16)
                    .padding(.bottom, timed ? 240 : 0)
                }
                .onChange(of: currentLine) { _, line in
                    guard let line else { return }
                    withAnimation(.easeInOut(duration: 0.35)) { scroller.scrollTo(line, anchor: .center) }
                }
            }
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .principal) {
                    VStack(spacing: 0) {
                        Text(title).font(.headline).lineLimit(1)
                        if let subtitle {
                            Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("lyrics.done")
                }
            }
        }
        .presentationDragIndicator(.visible)
        .onAppear { lines = lyricLines(lyrics: lyrics) }
        .task(id: lines.count) { await follow() }
    }

    @ViewBuilder private func row(_ line: LyricLine, index: Int) -> some View {
        switch line.kind {
        case .section:
            Text(line.text.uppercased())
                .font(.caption.weight(.semibold))
                .tracking(1.2)
                .foregroundStyle(.secondary)
                .padding(.top, 4)
                .accessibilityAddTraits(.isHeader)
        case .line:
            let sung = timed && index == currentLine
            words(line, sung: sung)
                .font(timed ? .title2.weight(.bold) : .title3.weight(.semibold))
                .fixedSize(horizontal: false, vertical: true)
                .scaleEffect(sung ? 1.0 : 0.97, anchor: .leading)
                .animation(.easeOut(duration: 0.2), value: sung)
                .contentShape(Rectangle())
                .onTapGesture {
                    if timed, let at = line.atSeconds { player?.seek(to: at) }
                }
                .accessibilityAddTraits(timed ? .isButton : [])
                .accessibilityHint(timed ? "Plays from this line" : "")
        case .gap:
            Spacer().frame(height: 18)
        }
    }

    /// The line's text: all bright when it's sung without word timings,
    /// the words sung so far bright with them, dim otherwise (when timed).
    private func words(_ line: LyricLine, sung: Bool) -> Text {
        guard timed else { return Text(line.text) }
        guard sung else { return Text(line.text).foregroundColor(.secondary) }
        guard !line.words.isEmpty else { return Text(line.text).foregroundColor(.primary) }
        let upTo = currentWord ?? -1
        return line.words.enumerated().reduce(Text("")) { text, item in
            let (i, word) = item
            return text + Text(word.text).foregroundColor(i <= upTo ? .accentColor : .secondary)
        }
    }

    /// Follows the song while the sheet is open: which line and word are sung.
    private func follow() async {
        guard let player, lines.contains(where: { $0.atSeconds != nil }) else { return }
        while !Task.isCancelled {
            let now = player.exactTime
            let line = lyricLineAt(lines: lines, seconds: now).map(Int.init)
            if line != currentLine { currentLine = line }
            let word = line.flatMap { lyricWordAt(line: lines[$0], seconds: now) }.map(Int.init)
            if word != currentWord { currentWord = word }
            try? await Task.sleep(for: .milliseconds(60))
        }
    }
}
#endif
