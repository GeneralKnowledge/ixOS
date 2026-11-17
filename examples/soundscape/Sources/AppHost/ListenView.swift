#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Where the Listen stack can go.
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
}

/// The Listen tab: your music library (artists, albums, songs, each song
/// with its recordings), from the take server and kept on the phone, with
/// the player under every screen.
struct ListenView: View {
    @State private var model: ListenModel
    @State private var player = TakePlayer()
    @State private var editingServer = false
    @State private var showingNowPlaying = false
    @State private var barHeight: CGFloat = 0

    init(core: SoundscapeCore) {
        _model = State(initialValue: ListenModel(core: core))
    }

    var body: some View {
        NavigationStack {
            content
                .miniPlayerClearance()
                .navigationTitle("Library")
                .navigationDestination(for: ListenRoute.self) { route in
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
                    }
                }
                .toolbar {
                    if model.server != nil {
                        ToolbarItem(placement: .primaryAction) {
                            Button("Take Server", systemImage: "server.rack") { editingServer = true }
                                .accessibilityIdentifier("listen.server")
                        }
                    }
                }
                .sheet(isPresented: $editingServer) {
                    ServerSheet(model: model)
                }
        }
        // The bar floats over every screen of the stack, and each screen
        // keeps room for it below its content (`miniPlayerClearance`):
        // an inset on the stack alone didn't reach the lists of pushed
        // screens, so a long album's last song hid under the bar.
        .environment(\.miniPlayerHeight, player.current == nil ? 0 : barHeight)
        .overlay(alignment: .bottom) {
            NowPlayingBar(player: player) { showingNowPlaying = true }
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { barHeight = $0 }
        }
        .sheet(isPresented: $showingNowPlaying) {
            NowPlayingView(model: model, player: player)
        }
        .onChange(of: player.current == nil) { _, stopped in
            if stopped { showingNowPlaying = false }
        }
        // On the stack, so an alert from a song's screen shows above it.
        .forgeAlert(Binding(
            get: { editingServer ? nil : model.alert },
            set: { model.alert = $0 }
        )) { action in Task { await model.handle(action) } }
        .task(id: model.server) { await model.loadLibrary() }
        .onAppear {
            player.onPlayed = { [model] trackId, recordingId in
                Task { await model.recordPlay(trackId: trackId, recordingId: recordingId) }
            }
        }
    }

    @ViewBuilder private var content: some View {
        switch model.phase {
        case .needsServer:
            ServerForm(model: model, done: nil)
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
                    Button("Change Server", systemImage: "server.rack") { editingServer = true }
                        .accessibilityIdentifier("listen.changeServer")
                }
            }
        case .ready:
            LibraryHome(model: model, player: player)
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
        .miniPlayerClearance()
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
                Stars(value: Int(take.stars), rate: rate)
                if !take.note.isEmpty {
                    Text(take.note).font(.callout).foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 2)
    }
}

private struct Stars: View {
    let value: Int
    let rate: (Int) -> Void

    var body: some View {
        HStack(spacing: 2) {
            ForEach(1...5, id: \.self) { n in
                Button { rate(n) } label: {
                    Image(systemName: n <= value ? "star.fill" : "star")
                        .foregroundStyle(n <= value ? Color.orange : Color.secondary)
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("\(n) \(n == 1 ? "star" : "stars")")
                .accessibilityIdentifier("take.star\(n)")
            }
        }
        .font(.subheadline)
        .accessibilityElement(children: .contain)
        .accessibilityValue("\(value) of 5")
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
            .background(on ? Color.accentColor.opacity(0.25) : Color.secondary.opacity(0.12),
                        in: RoundedRectangle(cornerRadius: 12))
            .overlay {
                RoundedRectangle(cornerRadius: 12).stroke(on ? Color.accentColor : .clear, lineWidth: 2)
            }
        }
        .accessibilityLabel(hidden ? "Hear \(label)" : "Hear \(label), \(take.label)")
        .accessibilityAddTraits(on ? .isSelected : [])
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

/// What's playing, under every Listen screen; tap it for Now Playing.
extension EnvironmentValues {
    /// How tall the mini-player is over the Library's screens; 0 when nothing plays.
    @Entry var miniPlayerHeight: CGFloat = 0
}

extension View {
    /// Room below a screen's content for the mini-player, so the last row
    /// can scroll clear of it.
    func miniPlayerClearance() -> some View { modifier(MiniPlayerClearance()) }
}

private struct MiniPlayerClearance: ViewModifier {
    @Environment(\.miniPlayerHeight) private var height

    func body(content: Content) -> some View {
        content.safeAreaInset(edge: .bottom, spacing: 0) {
            Color.clear.frame(height: height).allowsHitTesting(false)
        }
    }
}

private struct NowPlayingBar: View {
    let player: TakePlayer
    let open: () -> Void

    var body: some View {
        if let current = player.current {
            VStack(spacing: 0) {
                ProgressView(value: min(player.time, max(player.duration, 1)), total: max(player.duration, 1))
                    .progressViewStyle(.linear)
                    .tint(.accentColor)
                    .accessibilityHidden(true)
                HStack(spacing: 12) {
                    Button(action: open) {
                        HStack(spacing: 10) {
                            Artwork(title: current.song, url: current.artworkUrl, size: 40)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(player.isComparing ? "Comparing · \(player.active == .a ? "A" : "B")" : current.song)
                                    .font(.subheadline.bold())
                                    .lineLimit(1)
                                Text(player.isComparing ? current.song : [current.artist, current.label].compactMap { $0 }.joined(separator: " · "))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                            Spacer(minLength: 0)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Now Playing: \(current.song), \(current.label)")
                    .accessibilityHint("Opens the player")
                    .accessibilityIdentifier("nowPlaying.open")
                    if player.isBuffering { ProgressView() }
                    Button { player.toggle() } label: {
                        Image(systemName: player.isPlaying ? "pause.fill" : "play.fill").font(.title2)
                    }
                    .accessibilityLabel(player.isPlaying ? "Pause" : "Play")
                    .accessibilityIdentifier("nowPlaying.playPause")
                    if player.isComparing {
                        Button { player.stop() } label: {
                            Image(systemName: "xmark").font(.body.bold())
                        }
                        .accessibilityLabel("Stop")
                        .accessibilityIdentifier("nowPlaying.stop")
                    } else {
                        Button { player.next() } label: {
                            Image(systemName: "forward.fill").font(.title3)
                        }
                        .disabled(!player.hasNext)
                        .accessibilityLabel("Next")
                        .accessibilityIdentifier("nowPlaying.next")
                    }
                }
                .buttonStyle(.borderless)
                .padding(.horizontal)
                .padding(.vertical, 8)
                if let failure = player.failure {
                    Text(failure)
                        .font(.caption)
                        .forgeForeground(.danger)
                        .padding(.horizontal)
                        .padding(.bottom, 6)
                }
            }
            .background(.bar)
        }
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
        Group {
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
        .frame(width: size, height: size)
        .clipShape(circle ? AnyShape(Circle()) : AnyShape(RoundedRectangle(cornerRadius: size * 0.12)))
        .accessibilityHidden(true)
    }

    private var standIn: some View {
        let hue = Double(title.unicodeScalars.reduce(0) { ($0 &* 31 &+ Int($1.value)) & 0xFFFF } % 360) / 360
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

/// The full player: artwork, the position, previous/play/next, shuffle and
/// repeat, and Up Next. In a comparison, the A and B switch instead.
private struct NowPlayingView: View {
    let model: ListenModel
    let player: TakePlayer
    @Environment(\.dismiss) private var dismiss
    @State private var addingToPlaylist = false
    @State private var showingLyrics = false

    var body: some View {
        NavigationStack {
            List {
                if let current = player.current {
                    Section {
                        VStack(spacing: 16) {
                            Artwork(title: current.song, url: current.artworkUrl, size: 260)
                                .shadow(radius: 12, y: 6)
                                .padding(.top, 8)
                            VStack(spacing: 4) {
                                Text(current.song).font(.title2.bold()).multilineTextAlignment(.center)
                                Text([current.artist, current.album, current.label].compactMap { $0 }.joined(separator: " · "))
                                    .font(.subheadline)
                                    .foregroundStyle(.secondary)
                                    .multilineTextAlignment(.center)
                            }
                            .accessibilityElement(children: .combine)
                            Scrubber(player: player, showsButtons: false)
                            if player.isComparing { abSwitch } else { transport }
                            if let lyrics = current.lyrics, !lyricLines(lyrics: lyrics).isEmpty {
                                Button {
                                    showingLyrics = true
                                } label: {
                                    Label("Lyrics", systemImage: "quote.bubble")
                                        .font(.subheadline.weight(.semibold))
                                }
                                .buttonStyle(.bordered)
                                .buttonBorderShape(.capsule)
                                .accessibilityIdentifier("nowPlayingSheet.lyrics")
                                .sheet(isPresented: $showingLyrics) {
                                    LyricsView(title: current.song, subtitle: current.artist, lyrics: lyrics, player: player)
                                }
                            }
                            if let failure = player.failure {
                                Text(failure).font(.caption).forgeForeground(.danger)
                            }
                        }
                        .frame(maxWidth: .infinity)
                        .listRowBackground(Color.clear)
                    }
                    if !player.upcoming.isEmpty {
                        Section("Up Next") {
                            ForEach(player.upcoming) { entry in
                                Button {
                                    player.jump(to: entry.position)
                                } label: {
                                    HStack(spacing: 12) {
                                        Artwork(title: entry.track.title, url: entry.track.artworkUrl, size: 40)
                                        VStack(alignment: .leading, spacing: 2) {
                                            Text(entry.track.title).font(.body).foregroundStyle(.primary)
                                            Text(entry.track.artist).font(.caption).foregroundStyle(.secondary)
                                        }
                                    }
                                }
                                .accessibilityIdentifier("upNext.song")
                                .swipeActions {
                                    Button("Remove", systemImage: "minus.circle", role: .destructive) {
                                        player.remove(at: entry.position)
                                    }
                                    .accessibilityIdentifier("upNext.remove")
                                }
                            }
                        }
                    }
                }
            }
            .navigationTitle("Now Playing")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close", systemImage: "chevron.down") { dismiss() }
                        .accessibilityIdentifier("nowPlayingSheet.close")
                }
                if let trackId = player.current?.trackId, !player.isComparing {
                    ToolbarItem(placement: .primaryAction) {
                        FavouriteButton(model: model, id: trackId, identifier: "nowPlayingSheet.favourite")
                    }
                    ToolbarItem(placement: .primaryAction) {
                        Button("Add to Playlist", systemImage: "text.badge.plus") { addingToPlaylist = true }
                            .accessibilityIdentifier("nowPlayingSheet.addToPlaylist")
                    }
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("Stop", systemImage: "stop.fill") {
                        player.stop()
                        dismiss()
                    }
                    .accessibilityIdentifier("nowPlayingSheet.stop")
                }
            }
            .sheet(isPresented: $addingToPlaylist) {
                if let trackId = player.current?.trackId {
                    AddToPlaylistSheet(model: model, trackIds: [trackId])
                }
            }
        }
        .forgeFeedback(model.feedback)
        .presentationDragIndicator(.visible)
    }

    private var transport: some View {
        HStack {
            Button { player.toggleShuffle() } label: {
                Image(systemName: "shuffle")
                    .foregroundStyle(player.shuffle ? Color.accentColor : Color.secondary)
            }
            .accessibilityLabel("Shuffle")
            .accessibilityValue(player.shuffle ? "On" : "Off")
            .accessibilityIdentifier("nowPlayingSheet.shuffle")
            Spacer()
            Button { player.previous() } label: {
                Image(systemName: "backward.fill").font(.title)
            }
            .accessibilityLabel("Previous")
            .accessibilityIdentifier("nowPlayingSheet.previous")
            Spacer()
            Button { player.toggle() } label: {
                Image(systemName: player.isPlaying ? "pause.circle.fill" : "play.circle.fill")
                    .font(.system(size: 64))
            }
            .accessibilityLabel(player.isPlaying ? "Pause" : "Play")
            .accessibilityIdentifier("nowPlayingSheet.playPause")
            Spacer()
            Button { player.next() } label: {
                Image(systemName: "forward.fill").font(.title)
            }
            .disabled(!player.hasNext)
            .accessibilityLabel("Next")
            .accessibilityIdentifier("nowPlayingSheet.next")
            Spacer()
            Button { player.cycleRepeat() } label: {
                Image(systemName: player.repeatMode == .one ? "repeat.1" : "repeat")
                    .foregroundStyle(player.repeatMode == .off ? Color.secondary : Color.accentColor)
            }
            .accessibilityLabel("Repeat")
            .accessibilityValue(Self.repeatName(player.repeatMode))
            .accessibilityIdentifier("nowPlayingSheet.repeat")
        }
        .font(.title3)
        .buttonStyle(.borderless)
        .padding(.horizontal, 8)
    }

    private var abSwitch: some View {
        HStack(spacing: 24) {
            ForEach([TakePlayer.Side.a, .b], id: \.self) { side in
                Button { player.hear(side) } label: {
                    Text(side == .a ? "A" : "B")
                        .font(.largeTitle.bold())
                        .frame(width: 72, height: 72)
                        .background(
                            player.active == side ? Color.accentColor.opacity(0.25) : Color.secondary.opacity(0.12),
                            in: Circle()
                        )
                }
                .accessibilityLabel(side == .a ? "Hear A" : "Hear B")
                .accessibilityAddTraits(player.active == side ? .isSelected : [])
                .accessibilityIdentifier(side == .a ? "nowPlayingSheet.a" : "nowPlayingSheet.b")
            }
            Button { player.toggle() } label: {
                Image(systemName: player.isPlaying ? "pause.circle.fill" : "play.circle.fill")
                    .font(.system(size: 64))
            }
            .accessibilityLabel(player.isPlaying ? "Pause" : "Play")
            .accessibilityIdentifier("nowPlayingSheet.playPause")
        }
        .buttonStyle(.borderless)
    }

    static func repeatName(_ mode: RepeatMode) -> String {
        switch mode {
        case .off: return "Off"
        case .all: return "All"
        case .one: return "One"
        }
    }
}
#endif
