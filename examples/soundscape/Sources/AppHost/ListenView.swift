#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// The Listen tab: songs from the take server (generated takes on another
/// computer), each song's takes to play, compare, rate and pick.
struct ListenView: View {
    @State private var model: ListenModel
    @State private var player = TakePlayer()
    @State private var editingServer = false
    @State private var search = ""
    @AppStorage("listen.unpickedOnly") private var unpickedOnly = false

    init(core: SoundscapeCore) {
        _model = State(initialValue: ListenModel(core: core))
    }

    var body: some View {
        NavigationStack {
            content
                .navigationTitle("Listen")
                .navigationDestination(for: String.self) { slug in
                    SongView(model: model, player: player, slug: slug)
                }
                .toolbar {
                    if model.server != nil, model.phase == .ready {
                        ToolbarItem(placement: .topBarLeading) {
                            Toggle(isOn: $unpickedOnly) {
                                Label("Unpicked Only", systemImage: "line.3.horizontal.decrease.circle")
                            }
                            .toggleStyle(.button)
                            .accessibilityIdentifier("listen.unpickedOnly")
                        }
                    }
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
                .safeAreaInset(edge: .bottom) { NowPlayingBar(player: player) }
        }
        // On the stack, so an alert from a song's screen shows above it.
        .forgeAlert(Binding(
            get: { editingServer ? nil : model.alert },
            set: { model.alert = $0 }
        )) { action in Task { await model.handle(action) } }
        .task(id: model.server) { await model.loadSongs() }
    }

    @ViewBuilder private var content: some View {
        switch model.phase {
        case .needsServer:
            ServerForm(model: model, done: nil)
        case .loading:
            ForgeStateView.loading("Asking the take server")
        case .failed(let message):
            ForgeStateView.failed(
                "Can't Load the Songs", message: message,
                retry: ForgeStateAction("Try Again", identifier: "listen.retry") {
                    Task { await model.loadSongs() }
                }
            )
            .toolbar {
                ToolbarItem(placement: .secondaryAction) {
                    Button("Change Server", systemImage: "server.rack") { editingServer = true }
                        .accessibilityIdentifier("listen.changeServer")
                }
            }
        case .ready:
            songList
        }
    }

    private var songList: some View {
        List {
            ForEach(groups) { group in
                Section {
                    ForEach(group.songs) { song in songRow(song) }
                } header: {
                    Text(group.act ?? "Other")
                }
            }
        }
        .overlay {
            if model.songs.isEmpty {
                ForgeStateView.empty(
                    "No Takes Yet", systemImage: "music.note.list",
                    description: "The server found no audio in its folders. New renders show up here when you pull down."
                )
            } else if groups.isEmpty && !search.trimmingCharacters(in: .whitespaces).isEmpty {
                ForgeStateView.noResults(for: search)
            } else if groups.isEmpty {
                ForgeStateView.empty(
                    "Every Song Is Picked", systemImage: "checkmark.seal",
                    description: "Turn off Unpicked Only to see them all."
                )
            }
        }
        .searchable(text: $search, prompt: "Songs or acts")
        .refreshable { await model.loadSongs(fresh: true) }
    }

    private func songRow(_ song: SongSummary) -> some View {
        NavigationLink(value: song.slug) {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(song.title).font(.headline)
                    if song.pickLabel != nil {
                        Image(systemName: "checkmark.seal.fill")
                            .foregroundStyle(.tint)
                            .accessibilityLabel("Picked")
                    }
                }
                Text(TakeFormat.summary(song))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                Text(SoundFormat.date(song.updatedMs), format: .relative(presentation: .named))
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            .accessibilityElement(children: .combine)
        }
        .accessibilityIdentifier("listen.song")
    }

    private var groups: [SongGroup] {
        model.groups(search: search, unpickedOnly: unpickedOnly)
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
                Section { Text(problem).foregroundStyle(.red) }
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
        .sensoryFeedback(.success, trigger: model.markCount)
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
                        playing: player.current?.take.id == take.id && player.isPlaying && !player.isComparing,
                        play: {
                            if player.current?.take.id == take.id && !player.isComparing {
                                player.toggle()
                            } else {
                                player.play(take, of: song.title)
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
        return player.a?.take.id == sides.0.id && player.b?.take.id == sides.1.id
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
                Spacer()
                Text(TakeFormat.time(player.duration))
            }
            .font(.caption.monospacedDigit())
            .buttonStyle(.borderless)
        }
    }
}

/// What's playing, under every Listen screen.
private struct NowPlayingBar: View {
    let player: TakePlayer

    var body: some View {
        if let current = player.current {
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(player.isComparing ? "Comparing · \(player.active == .a ? "A" : "B")" : current.take.label)
                            .font(.subheadline.bold())
                            .lineLimit(1)
                        Text(current.song).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer()
                    if player.isBuffering { ProgressView() }
                    Button { player.toggle() } label: {
                        Image(systemName: player.isPlaying ? "pause.fill" : "play.fill").font(.title2)
                    }
                    .accessibilityLabel(player.isPlaying ? "Pause" : "Play")
                    .accessibilityIdentifier("nowPlaying.playPause")
                    Button { player.stop() } label: {
                        Image(systemName: "xmark").font(.body.bold())
                    }
                    .accessibilityLabel("Stop")
                    .accessibilityIdentifier("nowPlaying.stop")
                }
                .buttonStyle(.borderless)
                if !player.isComparing {
                    Scrubber(player: player)
                }
                if let failure = player.failure {
                    Text(failure).font(.caption).foregroundStyle(.red)
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 10)
            .background(.bar)
        }
    }
}
#endif
