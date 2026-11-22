#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Where each tab's stack can go.
enum ListenRoute: Hashable {
    case artist(LibraryArtist)
    case album(LibraryAlbum)
    case track(String)
    /// A song's takes on the take server, to compare and pick.
    case takes(String)
    case favourites
    case playlists
    case playlist(UInt64)
    case recent
    /// What is kept on the phone.
    case downloads
    /// Songs with several takes and no pick yet.
    case toPick
    /// Free catalogues to search and play (Jamendo, Internet Archive).
    case discover
    /// Discover, opened on a search (More in Discover).
    case discoverSearch(String)
    /// An Internet Archive album or concert, with its tracks.
    case archiveItem(ArchiveItem)
    /// Jamendo's genres, then one genre's tracks.
    case jamendoGenres
    case jamendoGenre(JamendoGenre)
    /// More from Jamendo like a song: by its artist or in its genre.
    case jamendoMore(trackId: String, by: JamendoMore)
    /// All of the library's songs, albums (a grid) or artists.
    case songs
    case albums
    case artists
}

/// Soundscape's four tabs (docs/design.md).
enum SoundscapeTab: String, Hashable {
    case listen, library, discover, search
}

/// The app: Listen, Library, Discover and Search, each its own stack, with
/// the mini-player above the tab bar and Now Playing opening out of it. One
/// model and one player serve every tab.
struct ListenView: View {
    @State private var model: ListenModel
    @State private var player = TakePlayer()
    @State private var radio = Radio()
    @State private var editingServer = false
    @State private var showingNowPlaying = false
    /// Songs on their way to a playlist, from a song's menu anywhere.
    @State private var addingToPlaylist: PlaylistAdd?
    @AppStorage("soundscape.tab") private var tab: SoundscapeTab = .listen
    @State private var listenPath: [ListenRoute] = []
    @State private var libraryPath: [ListenRoute] = []
    @State private var discoverPath: [ListenRoute] = []
    @State private var searchPath: [ListenRoute] = []
    @Namespace private var playerZoom

    init(core: SoundscapeCore, host: ForgeHost<SoundscapeCore>) {
        let model = ListenModel(core: core)
        model.follow(host)
        _model = State(initialValue: model)
    }

    var body: some View {
        tabs
            .modifier(MiniPlayerAccessory {
                MiniPlayer(model: model, player: player, zoom: playerZoom, open: openNowPlaying)
            })
            .fullScreenCover(isPresented: $showingNowPlaying) {
                NowPlayingView(model: model, player: player, radio: radio, go: go)
                    .modifier(ZoomedFrom(id: MiniPlayer.zoomID, namespace: playerZoom))
            }
            .onChange(of: player.current == nil) { _, stopped in
                if stopped { showingNowPlaying = false }
            }
            .sheet(isPresented: $editingServer) {
                ServerSheet(model: model)
            }
            .sheet(item: $addingToPlaylist) { adding in
                AddToPlaylistSheet(model: model, trackIds: adding.trackIds)
            }
            .environment(\.songActions, SongActions(
                startRadio: { [model, player, radio] seed, title in
                    Task { await radio.start(seed, title: title, model: model, player: player) }
                },
                go: go,
                addToPlaylist: { addingToPlaylist = PlaylistAdd(trackIds: $0) }
            ))
            // Over every tab, so an alert from any screen shows above it.
            .forgeAlert(Binding(
                get: { editingServer ? nil : model.alert },
                set: { model.alert = $0 }
            )) { action in Task { await model.handle(action) } }
            .task(id: model.server) { await model.loadLibrary() }
            // VoiceOver's two-finger double tap plays or pauses anywhere.
            .accessibilityAction(.magicTap) { player.toggle() }
            // The radio keeps a few songs queued ahead, and ends when
            // something else plays.
            .task(id: "\(player.current?.trackId ?? "")|\(player.upcoming.count)") {
                await radio.follow(model: model, player: player)
            }
            .onAppear {
                player.onPlayed = { [model] trackId, recordingId in
                    Task { await model.recordPlay(trackId: trackId, recordingId: recordingId) }
                }
                player.onSave = { [model] saved in
                    Task { await model.savePlayback(saved) }
                }
                player.crossfade = Double(model.crossfadeSeconds)
                // Where the app was left: the same queue and song, paused.
                if let saved = model.savedPlayback() { player.restore(saved) }
            }
    }

    @ViewBuilder private var tabs: some View {
        if #available(iOS 18.0, *) {
            TabView(selection: $tab) {
                Tab("Listen", systemImage: "play.circle.fill", value: SoundscapeTab.listen) { listen }
                Tab("Library", systemImage: "square.stack.fill", value: SoundscapeTab.library) { library }
                Tab("Discover", systemImage: "sparkles", value: SoundscapeTab.discover) { discover }
                Tab("Search", systemImage: "magnifyingglass", value: SoundscapeTab.search, role: .search) { search }
            }
        } else {
            TabView(selection: $tab) {
                listen.tabItem { Label("Listen", systemImage: "play.circle.fill") }.tag(SoundscapeTab.listen)
                library.tabItem { Label("Library", systemImage: "square.stack.fill") }.tag(SoundscapeTab.library)
                discover.tabItem { Label("Discover", systemImage: "sparkles") }.tag(SoundscapeTab.discover)
                search.tabItem { Label("Search", systemImage: "magnifyingglass") }.tag(SoundscapeTab.search)
            }
        }
    }

    private var listen: some View {
        stack($listenPath) {
            ListenHome(model: model, player: player, radio: radio, openNowPlaying: openNowPlaying)
        }
    }

    private var library: some View {
        stack($libraryPath) {
            LibraryGate(model: model, editServer: { editingServer = true }) {
                LibraryTab(model: model, player: player, editServer: { editingServer = true })
            }
        }
    }

    private var discover: some View {
        stack($discoverPath) { DiscoverView(model: model, player: player) }
    }

    private var search: some View {
        stack($searchPath) { SearchTab(model: model, player: player) }
    }

    /// One tab's stack: its first screen, every screen it can push, and
    /// (before iOS 26) the mini-player floating above the tab bar.
    private func stack<Root: View>(_ path: Binding<[ListenRoute]>, @ViewBuilder root: () -> Root) -> some View {
        NavigationStack(path: path) {
            root().navigationDestination(for: ListenRoute.self) { route in
                ListenDestination(model: model, player: player, route: route)
            }
        }
        // Undo for what was just removed, above the tab bar.
        .soundscapeBanner(model: model, player: player)
        .modifier(MiniPlayerBar {
            MiniPlayer(model: model, player: player, zoom: playerZoom, open: openNowPlaying)
        })
    }

    private func openNowPlaying() {
        if player.current != nil { showingNowPlaying = true }
    }

    /// From Now Playing to a screen (an album, an artist, more like this
    /// song): close the player and push it on the tab being looked at.
    private func go(_ route: ListenRoute) {
        showingNowPlaying = false
        switch tab {
        case .listen: listenPath.append(route)
        case .library: libraryPath.append(route)
        case .discover: discoverPath.append(route)
        case .search: searchPath.append(route)
        }
    }
}

/// Songs for the Add to Playlist sheet.
private struct PlaylistAdd: Identifiable {
    let id = UUID()
    let trackIds: [String]
}

/// The screen for a route, the same in every tab.
struct ListenDestination: View {
    let model: ListenModel
    let player: TakePlayer
    let route: ListenRoute

    var body: some View {
        switch route {
        case .artist(let artist): ArtistView(model: model, player: player, artist: artist)
        case .album(let album): AlbumView(model: model, player: player, album: album)
        case .track(let id): TrackView(model: model, player: player, id: id)
        case .takes(let slug): SongView(model: model, player: player, slug: slug)
        case .favourites: FavouritesView(model: model, player: player)
        case .playlists: PlaylistsView(model: model, player: player)
        case .playlist(let id): PlaylistView(model: model, player: player, id: id)
        case .recent: RecentlyPlayedView(model: model, player: player)
        case .downloads: DownloadsView(model: model, player: player)
        case .toPick: ToPickView(model: model, player: player)
        case .discover: DiscoverView(model: model, player: player)
        case .discoverSearch(let query): DiscoverView(model: model, player: player, search: query)
        case .archiveItem(let item): ArchiveItemView(model: model, player: player, item: item)
        case .jamendoGenres: JamendoGenresView(model: model)
        case .jamendoGenre(let genre): JamendoTracksView(model: model, player: player, ask: .genre(genre))
        case .jamendoMore(let trackId, let by): JamendoTracksView(model: model, player: player, ask: .more(trackId: trackId, by: by))
        case .songs: SongsView(model: model, player: player)
        case .albums: AlbumsView(model: model)
        case .artists: ArtistsView(model: model)
        }
    }
}

/// The library's screens wait for the first load from the take server, or
/// say why it failed.
struct LibraryGate<Content: View>: View {
    let model: ListenModel
    let editServer: () -> Void
    @ViewBuilder let content: () -> Content

    var body: some View {
        switch model.phase {
        case .loading:
            ForgeStateView.loading("Asking the take server")
        case .failed(let message):
            ForgeStateView.failed(
                "Can't Load the Library", message: message,
                retry: ForgeStateAction("Try Again", identifier: "listen.retry") {
                    Task { await model.loadLibrary() }
                }
            )
            .toolbar {
                ToolbarItem(placement: .secondaryAction) {
                    Button("Change Server", systemImage: "server.rack", action: editServer)
                        .accessibilityIdentifier("listen.changeServer")
                }
            }
        case .ready:
            content()
        }
    }
}

/// The mini-player as the tab bar's bottom accessory on iOS 26, where it
/// shrinks into the tab bar as a list scrolls (the system's own music
/// pattern). Before iOS 26 `MiniPlayerBar` floats it in each tab instead.
private struct MiniPlayerAccessory<Bar: View>: ViewModifier {
    @ViewBuilder let bar: () -> Bar

    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 26.0, *) {
            content
                .tabViewBottomAccessory { AccessoryPlacement { bar() } }
                .tabBarMinimizeBehavior(.onScrollDown)
        } else {
            content
        }
    }
}

/// Tells the mini-player whether the accessory is full width or tucked
/// into the minimised tab bar.
@available(iOS 26.0, *)
private struct AccessoryPlacement<Bar: View>: View {
    @Environment(\.tabViewBottomAccessoryPlacement) private var placement
    @ViewBuilder let bar: () -> Bar

    var body: some View {
        bar().environment(\.miniPlayerInline, placement == .inline)
    }
}

/// Before iOS 26: the mini-player floats over each tab's screens, above the
/// tab bar, through ForgeUI's forgeBar (which gives lists room below).
private struct MiniPlayerBar<Bar: View>: ViewModifier {
    @ViewBuilder let bar: () -> Bar

    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 26.0, *) {
            content
        } else {
            content.forgeBar { bar().miniPlayerCard() }
        }
    }
}

/// Now Playing grows out of the mini-player (iOS 18 and later).
private struct ZoomedFrom: ViewModifier {
    let id: String
    let namespace: Namespace.ID

    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 18.0, *) {
            content.navigationTransition(.zoom(sourceID: id, in: namespace))
        } else {
            content
        }
    }
}

// MARK: Server address

private struct ServerSheet: View {
    let model: ListenModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ServerForm(model: model) { dismiss() }
                .navigationTitle("Take Server")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Cancel") { dismiss() }
                            .accessibilityIdentifier("server.cancel")
                    }
                }
        }
        .presentationDetents([.medium, .large])
    }
}

/// The address field, with how to start the server. Shown on its own
/// before there is a server, and in a sheet to change it.
private struct ServerForm: View {
    let model: ListenModel
    let done: (() -> Void)?
    @State private var address = ""
    @State private var problem: String?
    @State private var saving = false

    var body: some View {
        Form {
            Section {
                TextField("100.101.102.103", text: $address)
                    .keyboardType(.numbersAndPunctuation)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.go)
                    .onSubmit(save)
                    .accessibilityLabel("Server address")
                    .accessibilityIdentifier("server.address")
                Button(saving ? "Connecting…" : "Connect", action: save)
                    .disabled(saving || address.trimmingCharacters(in: .whitespaces).isEmpty)
                    .accessibilityIdentifier("server.save")
            } header: {
                Text("Take server address")
            } footer: {
                Text("The computer's IP address, with :port if it isn't 8642. Over Tailscale it starts with 100.")
            }
            if let problem {
                Section { Text(problem).forgeForeground(.danger) }
            }
            Section("Starting the server") {
                Text("On the computer with the songs, run:")
                Text("python3 tools/player/serve.py")
                    .font(.system(.body, design: .monospaced))
                    .textSelection(.enabled)
                Text("It finds the takes under ~/band, streams them here, and saves your picks to ~/band/picks.json.")
                    .foregroundStyle(.secondary)
            }
            if model.server != nil {
                Section {
                    Button("Forget This Server", role: .destructive) {
                        done?()
                        model.askToForgetServer()
                    }
                    .accessibilityIdentifier("server.forget")
                }
            }
        }
        .onAppear { if address.isEmpty { address = model.server ?? "" } }
    }

    private func save() {
        saving = true
        problem = nil
        let typed = address
        Task {
            defer { saving = false }
            do {
                try await model.setServer(typed)
                done?()
            } catch {
                problem = ListenModel.message(error)
            }
        }
    }
}

// MARK: One song

private struct SongView: View {
    let model: ListenModel
    let player: TakePlayer
    let slug: String

    @State private var song: SongDetail?
    @State private var failure: String?
    @State private var compareA: String?
    @State private var compareB: String?
    @State private var noting: Take?
    @State private var noteText = ""

    var body: some View {
        Group {
            if let song {
                takes(song)
            } else if let failure {
                ForgeStateView.failed(
                    "Can't Load the Takes", message: failure,
                    retry: ForgeStateAction("Try Again", identifier: "song.retry") { Task { await load() } }
                )
            } else {
                ForgeStateView.loading()
            }
        }
        .navigationTitle(song?.title ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .task { await load() }
        .refreshable { await load() }
        .forgeFeedback(model.feedback)
        .alert("Note", isPresented: Binding(get: { noting != nil }, set: { if !$0 { noting = nil } })) {
            TextField("What stands out", text: $noteText)
                .accessibilityIdentifier("take.noteText")
            Button("Save") { saveNote() }
                .accessibilityIdentifier("take.noteSave")
            Button("Cancel", role: .cancel) {}
                .accessibilityIdentifier("take.noteCancel")
        }
    }

    private func takes(_ song: SongDetail) -> some View {
        List {
            if song.takes.count >= 2 {
                Section("Compare") {
                    ComparePanel(model: model, player: player, song: song, a: $compareA, b: $compareB) { take in
                        Task { if let s = await model.pick(take, in: song) { self.song = s } }
                    }
                }
            }
            Section("Takes") {
                ForEach(song.takes) { take in
                    TakeRow(
                        take: take,
                        playing: player.current?.id == take.id && player.isPlaying && !player.isComparing,
                        play: {
                            if player.current?.id == take.id && !player.isComparing {
                                player.toggle()
                            } else {
                                player.play([model.queue(take, of: song)])
                            }
                        },
                        rate: { stars in Task { if let s = await model.rate(take, in: song, stars: stars) { self.song = s } } }
                    )
                    .swipeActions(edge: .leading) {
                        Button(take.picked ? "Unpick" : "Pick", systemImage: "checkmark.seal") {
                            Task { if let s = await model.pick(take, in: song, !take.picked) { self.song = s } }
                        }
                        .tint(.accentColor)
                        .accessibilityIdentifier("take.pick")
                    }
                    .swipeActions(edge: .trailing) {
                        Button("Note", systemImage: "square.and.pencil") {
                            noteText = take.note
                            noting = take
                        }
                        .accessibilityIdentifier("take.note")
                    }
                    .contextMenu {
                        Button(take.picked ? "Unpick" : "Pick This Take", systemImage: "checkmark.seal") {
                            Task { if let s = await model.pick(take, in: song, !take.picked) { self.song = s } }
                        }
                        .accessibilityIdentifier("take.pickMenu")
                        Button("Note", systemImage: "square.and.pencil") {
                            noteText = take.note
                            noting = take
                        }
                        .accessibilityIdentifier("take.noteMenu")
                        Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                            player.add([model.queue(take, of: song)], next: true)
                        }
                        .accessibilityIdentifier("take.playNext")
                        Button("Add to Queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                            player.add([model.queue(take, of: song)], next: false)
                        }
                        .accessibilityIdentifier("take.addToQueue")
                    }
                }
            }
            if song.caption != nil || song.lyrics != nil {
                Section("How it was made") {
                    if let caption = song.caption {
                        DisclosureGroup("Caption") {
                            Text(caption).font(.callout).textSelection(.enabled)
                        }
                        .accessibilityIdentifier("song.caption")
                    }
                    if let lyrics = song.lyrics {
                        DisclosureGroup("Lyrics") {
                            Text(lyrics).font(.callout).textSelection(.enabled)
                        }
                        .accessibilityIdentifier("song.lyrics")
                    }
                }
            }
        }
    }

    private func load() async {
        switch await model.song(slug) {
        case .success(let loaded):
            song = loaded
            failure = nil
            let ids = loaded.takes.map(\.id)
            if compareA == nil || !ids.contains(compareA!) { compareA = ids.first }
            if compareB == nil || !ids.contains(compareB!) { compareB = ids.dropFirst().first }
        case .failure(let f):
            if !f.stopped && song == nil { failure = f.message }
        }
    }

    private func saveNote() {
        guard let take = noting, let song else { return }
        let text = noteText
        Task { if let s = await model.note(take, in: song, text) { self.song = s } }
    }
}

private struct TakeRow: View {
    let take: Take
    let playing: Bool
    let play: () -> Void
    let rate: (Int) -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Button(action: play) {
                Image(systemName: playing ? "pause.circle.fill" : "play.circle.fill")
                    .font(.system(size: 34))
            }
            .buttonStyle(.borderless)
            .accessibilityLabel(playing ? "Pause \(take.label)" : "Play \(take.label)")
            .accessibilityIdentifier("take.play")
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(take.label).font(.headline)
                    if take.picked {
                        Label("Picked", systemImage: "checkmark.seal.fill")
                            .labelStyle(.iconOnly)
                            .foregroundStyle(.tint)
                    }
                }
                let details = [TakeFormat.details(take), TakeFormat.loudness(take.lufs)]
                    .compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
                if !details.isEmpty {
                    Text(details).font(.caption).foregroundStyle(.secondary)
                }
                ForgeRating(value: Int(take.stars), identifier: "take.rating") { rate($0 ?? 0) }
                if !take.note.isEmpty {
                    Text(take.note).font(.callout).foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 2)
    }
}

// MARK: Comparing

/// Two takes in step: switch between A and B at the same moment of the
/// song, at matched loudness. Blind hides which take is which until you
/// reveal it.
private struct ComparePanel: View {
    let model: ListenModel
    let player: TakePlayer
    let song: SongDetail
    @Binding var a: String?
    @Binding var b: String?
    let pick: (Take) -> Void

    @State private var blind = false
    @State private var swapped = false
    @State private var revealed = false

    private var takeA: Take? { song.takes.first { $0.id == a } }
    private var takeB: Take? { song.takes.first { $0.id == b } }
    /// What plays on side A and B (swapped at random when blind).
    private var sides: (Take, Take)? {
        guard let takeA, let takeB else { return nil }
        return blind && swapped ? (takeB, takeA) : (takeA, takeB)
    }
    private var loaded: Bool {
        guard player.isComparing, let sides else { return false }
        return player.a?.id == sides.0.id && player.b?.id == sides.1.id
    }
    private var hidden: Bool { blind && !revealed }

    var body: some View {
        if !hidden {
            Picker("A", selection: $a) {
                ForEach(song.takes) { Text($0.label).tag(Optional($0.id)) }
            }
            .accessibilityIdentifier("compare.pickA")
            Picker("B", selection: $b) {
                ForEach(song.takes) { Text($0.label).tag(Optional($0.id)) }
            }
            .accessibilityIdentifier("compare.pickB")
        }
        Toggle("Blind", isOn: $blind)
            .onChange(of: blind) {
                swapped = Bool.random()
                revealed = false
                if player.isComparing { player.stop() }
            }
            .accessibilityIdentifier("compare.blind")
        if let sides {
            HStack(spacing: 12) {
                sideButton(.a, label: "A", take: sides.0)
                sideButton(.b, label: "B", take: sides.1)
            }
            .buttonStyle(.borderless)
            if loaded {
                Scrubber(player: player)
                Text(TakeFormat.match(player.match ?? model.match(sides.0, sides.1)))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                HStack {
                    Button("Pick A") { reveal(); pick(sides.0) }
                        .accessibilityIdentifier("compare.pickTakeA")
                    Spacer()
                    if blind && !revealed {
                        Button("Reveal") { revealed = true }
                            .accessibilityIdentifier("compare.reveal")
                        Spacer()
                    }
                    Button("Pick B") { reveal(); pick(sides.1) }
                        .accessibilityIdentifier("compare.pickTakeB")
                }
                .buttonStyle(.borderless)
                if blind && revealed {
                    Text("A was \(sides.0.label), B was \(sides.1.label)")
                        .font(.callout)
                }
            }
        }
    }

    private func reveal() { revealed = true }

    private func sideButton(_ side: TakePlayer.Side, label: String, take: Take) -> some View {
        let on = loaded && player.active == side
        return Button {
            if !loaded {
                guard let sides else { return }
                player.compare(sides.0, sides.1, of: song.title, match: model.match(sides.0, sides.1))
            }
            player.hear(side)
        } label: {
            VStack(spacing: 4) {
                Text(label).font(.largeTitle.bold())
                Text(hidden ? "Hidden" : take.label)
                    .font(.caption)
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity, minHeight: 72)
        }
        .buttonStyle(.forgeChoice(isSelected: on, shape: .tile))
        .accessibilityLabel(hidden ? "Hear \(label)" : "Hear \(label), \(take.label)")
        .accessibilityIdentifier(side == .a ? "compare.a" : "compare.b")
    }
}

/// Play/pause, the position and a slider to move it.
private struct Scrubber: View {
    let player: TakePlayer
    /// Back 10, play/pause and forward 10 between the times.
    var showsButtons = true
    @State private var dragging: Double?

    var body: some View {
        VStack(spacing: 4) {
            Slider(
                value: Binding(get: { dragging ?? player.time }, set: { dragging = $0 }),
                in: 0...max(player.duration, 1)
            ) { editing in
                if !editing, let d = dragging {
                    player.seek(to: d)
                    dragging = nil
                }
            }
            .accessibilityLabel("Position")
            .accessibilityIdentifier("player.position")
            HStack {
                Text(TakeFormat.time(dragging ?? player.time))
                Spacer()
                if showsButtons { buttons }
                Spacer()
                Text(TakeFormat.time(player.duration))
            }
            .font(.caption.monospacedDigit())
            .buttonStyle(.borderless)
        }
    }

    @ViewBuilder private var buttons: some View {
        Button { player.skip(-10) } label: { Image(systemName: "gobackward.10") }
            .accessibilityLabel("Back 10 seconds")
            .accessibilityIdentifier("player.back")
        Button { player.toggle() } label: {
            Image(systemName: player.isPlaying ? "pause.fill" : "play.fill").font(.title2)
        }
        .accessibilityLabel(player.isPlaying ? "Pause" : "Play")
        .accessibilityIdentifier("player.playPause")
        Button { player.skip(10) } label: { Image(systemName: "goforward.10") }
            .accessibilityLabel("Forward 10 seconds")
            .accessibilityIdentifier("player.forward")
    }
}

/// A picture for an artist, album or track: its artwork when it has some
/// (from the server or chosen on the phone), else a stand-in, the title's
/// initial on a colour taken from the title, so each keeps its colour.
struct Artwork: View {
    let title: String
    var url: String? = nil
    let size: CGFloat
    var circle = false

    @Environment(\.displayScale) private var displayScale

    var body: some View {
        // The coloured initial is Soundscape's own stand-in (its look), in
        // ForgeUI's thumbnail frame.
        ForgeThumbnail(width: size, shape: circle ? .circle : .rounded) {
            if let url, let link = URL(string: url) {
                ForgeRemoteImage(url: link, maxPixels: ForgeImages.pixels(size, scale: displayScale)) { image in
                    image.resizable().scaledToFill()
                } placeholder: { _ in
                    standIn
                }
            } else {
                standIn
            }
        }
    }

    private var standIn: some View {
        let hue = ArtworkPalette.standInHue(title)
        return LinearGradient(
            colors: [Color(hue: hue, saturation: 0.55, brightness: 0.85),
                     Color(hue: hue, saturation: 0.75, brightness: 0.45)],
            startPoint: .topLeading, endPoint: .bottomTrailing
        )
        .overlay {
            Text(title.first.map { String($0).uppercased() } ?? "♪")
                .font(.system(size: size * 0.42, weight: .bold, design: .rounded))
                .foregroundStyle(.white.opacity(0.9))
        }
    }
}

#endif

#if os(iOS)
/// The sleep timer: pause after a while, or at the end of this song.
struct SleepTimerMenu: View {
    let player: TakePlayer

    private var on: Bool { player.sleepAt != nil || player.sleepAtEndOfSong }

    var body: some View {
        Menu {
            ForEach([15, 30, 45, 60, 90], id: \.self) { minutes in
                Button("\(minutes) Minutes") { player.sleep(after: minutes) }
                    .accessibilityIdentifier("sleep.\(minutes)")
            }
            Button("End of This Song") { player.sleepAtEndOfThisSong() }
                .accessibilityIdentifier("sleep.endOfSong")
            if on {
                Button("Turn Off Sleep Timer", role: .destructive) { player.cancelSleep() }
                    .accessibilityIdentifier("sleep.off")
            }
        } label: {
            Label("Sleep Timer", systemImage: on ? "moon.zzz.fill" : "moon.zzz")
        }
        .accessibilityValue(on ? "On" : "Off")
        .accessibilityIdentifier("nowPlayingSheet.sleep")
    }
}

/// How songs run into each other: back to back with no gap, or
/// overlapping by a few seconds.
struct CrossfadeMenu: View {
    let model: ListenModel
    let player: TakePlayer

    var body: some View {
        Menu {
            Picker("Between Songs", selection: Binding(
                get: { model.crossfadeSeconds },
                set: { seconds in
                    player.crossfade = Double(seconds)
                    Task { await model.setCrossfade(seconds) }
                }
            )) {
                Text("No Gap").tag(UInt32(0))
                    .accessibilityIdentifier("crossfade.0")
                ForEach([UInt32(2), 4, 6, 8, 12], id: \.self) { seconds in
                    Text("Crossfade \(seconds) Seconds").tag(seconds)
                        .accessibilityIdentifier("crossfade.\(seconds)")
                }
            }
        } label: {
            Label("Between Songs", systemImage: "arrow.left.and.right.righttriangle.left.righttriangle.right")
        }
        .accessibilityValue(model.crossfadeSeconds == 0 ? "No gap" : "Crossfade \(model.crossfadeSeconds) seconds")
        .accessibilityIdentifier("nowPlayingSheet.crossfade")
    }
}
#endif
