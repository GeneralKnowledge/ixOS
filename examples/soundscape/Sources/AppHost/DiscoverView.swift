#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

// MARK: Discover

/// Free music to find and play, from Jamendo (tracks, this week's popular
/// until you type) or the Internet Archive (albums and concerts), each
/// track with its licence. Plays through the same player; Add to Library
/// keeps a track with your music, where it can be put in playlists and,
/// where its source allows, downloaded.
struct DiscoverView: View {
    let model: ListenModel
    let player: TakePlayer

    enum Source: String, CaseIterable, Identifiable {
        case jamendo = "Jamendo", archive = "Internet Archive"
        var id: String { rawValue }
    }

    @AppStorage("discover.source") private var source: Source = .jamendo
    @State private var search: String
    @State private var results: [CatalogueTrack] = []
    @State private var items: [ArchiveItem] = []
    @State private var problem: String?
    @State private var searching = false
    @State private var editingClientId = false
    @State private var attempt = 0
    @State private var opened = false
    @State private var showingTestCatalogue = false

    /// Opens on `search` when given (More in Discover, from the library's
    /// search), else on the source's popular list.
    init(model: ListenModel, player: TakePlayer, search: String = "") {
        self.model = model
        self.player = player
        _search = State(initialValue: search)
    }

    private var query: String { search.trimmingCharacters(in: .whitespaces) }
    private var needsClientId: Bool { source == .jamendo && model.jamendoClientId == nil }
    private var empty: Bool { source == .jamendo ? results.isEmpty : items.isEmpty }

    var body: some View {
        Group {
            if needsClientId {
                JamendoClientIdForm(model: model, done: nil)
            } else {
                list
            }
        }
        .onAppear {
            // A search carried over from the library opens where it can
            // run: without a Jamendo client ID, that is the Archive. Only on
            // opening, so coming back from an album keeps the person's pick.
            if !opened && !query.isEmpty && needsClientId { source = .archive }
            opened = true
        }
        .safeAreaInset(edge: .top) {
            Picker("Source", selection: $source) {
                ForEach(Source.allCases) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)
            .padding(.horizontal)
            .padding(.bottom, 6)
            .background(.bar)
            .accessibilityIdentifier("discover.source")
        }
        .navigationTitle("Discover")
        .toolbar {
            ToolbarItem(placement: .secondaryAction) {
                Button("Test Catalogue", systemImage: "testtube.2") { showingTestCatalogue = true }
                    .accessibilityIdentifier("discover.testCatalogue")
            }
            if model.jamendoClientId != nil {
                ToolbarItem(placement: .secondaryAction) {
                    Button("Jamendo Client ID", systemImage: "key") { editingClientId = true }
                        .accessibilityIdentifier("discover.clientId")
                }
            }
        }
        .navigationDestination(isPresented: $showingTestCatalogue) {
            TestCatalogueView(model: model, player: player)
        }
        .sheet(isPresented: $editingClientId) {
            NavigationStack {
                JamendoClientIdForm(model: model) { editingClientId = false }
                    .navigationTitle("Jamendo")
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Cancel") { editingClientId = false }
                                .accessibilityIdentifier("discover.clientIdCancel")
                        }
                    }
            }
            .presentationDetents([.medium, .large])
        }
        .forgeFeedback(model.feedback)
    }

    private var list: some View {
        Group {
            // Before a search: Jamendo as an editorial page of shelves
            // (S5); a search, or the Archive, is a list.
            if source == .jamendo && query.isEmpty && !results.isEmpty {
                DiscoverHome(model: model, player: player, popular: results)
            } else {
                List {
                    switch source {
                    case .jamendo: jamendo
                    case .archive: archive
                    }
                }
            }
        }
        .searchable(text: $search, prompt: source == .jamendo ? "Search Jamendo" : "Search the Internet Archive")
        .refreshable { model.refreshJamendo(); attempt += 1 }
        .task(id: "\(source.rawValue)|\(model.jamendoClientId ?? "")|\(query)|\(attempt)") { await find() }
        .overlay {
            if let problem, empty {
                ForgeStateView.failed(
                    "Can't Search \(source.rawValue)", message: problem,
                    retry: ForgeStateAction("Try Again", identifier: "discover.retry") { attempt += 1 }
                )
            } else if searching && empty {
                ForgeStateView.loading("Searching \(source.rawValue)")
            } else if !searching && empty && !query.isEmpty {
                ForgeStateView.noResults(for: query)
            }
        }
    }

    @ViewBuilder private var jamendo: some View {
        if query.isEmpty {
            Section {
                NavigationLink(value: ListenRoute.jamendoGenres) {
                    Label("Browse by Genre", systemImage: "square.grid.2x2")
                }
                .accessibilityIdentifier("discover.genres")
            }
        }
        Section {
            PlayShuffleButtons(disabled: results.isEmpty) { shuffle in
                player.play(model.queue(results), shuffle: shuffle)
            }
        }
        Section {
            ForEach(Array(results.enumerated()), id: \.element.id) { index, track in
                CatalogueRow(model: model, player: player, track: track) {
                    player.play(model.queue(results, from: index))
                }
            }
        } header: {
            if !results.isEmpty {
                Text(query.isEmpty ? "Popular This Week" : "Results")
            }
        } footer: {
            if !results.isEmpty {
                Text("Music from Jamendo, each track under its artist's Creative Commons licence.")
            }
        }
    }

    @ViewBuilder private var archive: some View {
        Section {
            ForEach(items, id: \.identifier) { item in
                NavigationLink(value: ListenRoute.archiveItem(item)) {
                    ArchiveItemRow(item: item)
                }
                .accessibilityIdentifier("discover.archiveItem")
            }
        } header: {
            if !items.isEmpty {
                Text(query.isEmpty ? "Net Labels, Popular This Week" : "Albums and Recordings")
            }
        } footer: {
            if !items.isEmpty {
                Text("From the Internet Archive. Rights vary item by item; each shows its licence.")
            }
        }
    }

    /// Searches after a short pause in typing; a newer search cancels this.
    private func find() async {
        if !query.isEmpty {
            try? await Task.sleep(for: .milliseconds(400))
            if Task.isCancelled { return }
        }
        if needsClientId { return }
        searching = true
        defer { searching = false }
        problem = nil
        switch source {
        case .jamendo:
            switch await model.searchJamendo(query) {
            case .success(let found): results = found
            case .failure(let failure): if !failure.stopped { problem = failure.message }
            }
        case .archive:
            switch await model.searchArchive(query) {
            case .success(let found): items = found
            case .failure(let failure): if !failure.stopped { problem = failure.message }
            }
        }
    }
}

/// An Internet Archive item in a list: its picture, title, who and when,
/// and its licence.
struct ArchiveItemRow: View {
    let item: ArchiveItem

    var body: some View {
        HStack(spacing: 12) {
            Artwork(title: item.title, url: item.artworkUrl, size: 44)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title).lineLimit(1)
                Text([item.creator, item.year].compactMap { $0 }.joined(separator: " · "))
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                Text(item.licence).font(.caption2).foregroundStyle(.tertiary).lineLimit(1)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// An Internet Archive album or concert: its tracks, to play or add.
struct ArchiveItemView: View {
    let model: ListenModel
    let player: TakePlayer
    let item: ArchiveItem

    @State private var tracks: [CatalogueTrack]?
    @State private var problem: String?
    @State private var attempt = 0

    var body: some View {
        List {
            Section {
                VStack(spacing: 8) {
                    Artwork(title: item.title, url: item.artworkUrl, size: 180)
                        .shadow(radius: 8, y: 4)
                    Text(item.title).font(.title3.bold()).multilineTextAlignment(.center)
                    let who = [item.creator, item.year].compactMap { $0 }.joined(separator: " · ")
                    if !who.isEmpty {
                        Text(who).font(.subheadline).foregroundStyle(.secondary)
                    }
                    Text(item.licence).font(.caption).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
            }
            if let tracks, !tracks.isEmpty {
                Section {
                    PlayShuffleButtons(disabled: false) { shuffle in
                        player.play(model.queue(tracks), shuffle: shuffle)
                    }
                    let missing = tracks.filter { !model.inLibrary($0.id) }
                    Button {
                        Task { await model.addToLibrary(missing) }
                    } label: {
                        Label(missing.isEmpty ? "All in Your Library" : "Add All to Library",
                              systemImage: missing.isEmpty ? "checkmark.circle" : "plus.circle")
                    }
                    .disabled(missing.isEmpty)
                    .accessibilityIdentifier("archive.addAll")
                }
                Section {
                    ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                        CatalogueRow(model: model, player: player, track: track) {
                            player.play(model.queue(tracks, from: index))
                        }
                    }
                } footer: {
                    if let credit = tracks.first?.rights.attribution {
                        Text(credit)
                    }
                }
            }
        }
        .navigationTitle(item.title)
        .navigationBarTitleDisplayMode(.inline)
        .task(id: attempt) {
            switch await model.archiveTracks(item) {
            case .success(let found):
                tracks = found
                problem = nil
            case .failure(let failure):
                if !failure.stopped { problem = failure.message }
            }
        }
        .overlay {
            if let problem, tracks == nil {
                ForgeStateView.failed(
                    "Can't Open This", message: problem,
                    retry: ForgeStateAction("Try Again", identifier: "archive.retry") { attempt += 1 }
                )
            } else if tracks == nil {
                ForgeStateView.loading("Asking the Internet Archive")
            } else if tracks?.isEmpty == true {
                ForgeStateView.empty(
                    "Nothing to Play", systemImage: "music.note",
                    description: "This item has no audio Soundscape can stream."
                )
            }
        }
        .forgeFeedback(model.feedback)
    }
}

/// One catalogue track: plays on tap, with its licence, and adds to the
/// library with the plus.
struct CatalogueRow: View {
    let model: ListenModel
    let player: TakePlayer
    let track: CatalogueTrack
    let play: () -> Void

    private var isCurrent: Bool { player.current?.id == track.id && !player.isComparing }
    private var subtitle: String {
        [track.artist ?? "Unknown Artist", track.album].compactMap { $0 }.joined(separator: " · ")
    }
    private var facts: String {
        [track.durationSeconds.map(TakeFormat.time), track.rights.licence].compactMap { $0 }
            .joined(separator: " · ")
    }

    var body: some View {
        let added = model.inLibrary(track.id)
        HStack(spacing: 12) {
            Button(action: play) {
                HStack(spacing: 12) {
                    Artwork(title: track.title, url: track.artworkUrl, size: 44)
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 4) {
                            Text(track.title)
                                .foregroundStyle(isCurrent ? Color.accentColor : .primary)
                                .lineLimit(1)
                            if isCurrent && player.isPlaying {
                                Image(systemName: "speaker.wave.2.fill")
                                    .font(.caption)
                                    .foregroundStyle(.tint)
                                    .accessibilityLabel("Playing")
                            }
                        }
                        Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        Text(facts).font(.caption2).foregroundStyle(.tertiary).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(track.title), \(subtitle), \(facts)")
            .accessibilityHint("Plays it")
            .accessibilityIdentifier("discover.track")
            if added {
                Image(systemName: "checkmark.circle.fill")
                    .font(.title3)
                    .foregroundStyle(.secondary)
                    .frame(width: 32, height: 32)
                    .accessibilityLabel("In your library")
                    .accessibilityIdentifier("discover.inLibrary")
            } else {
                Button {
                    Task { await model.addToLibrary([track]) }
                } label: {
                    Image(systemName: "plus.circle")
                        .font(.title3)
                        .frame(width: 32, height: 32)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Add \(track.title) to Library")
                .accessibilityIdentifier("discover.add")
            }
        }
        .contextMenu {
            SongMenu(model: model, player: player, song: SongRef(track, model: model), identifier: "discover")
            if !added {
                Button("Add to Library", systemImage: "plus") {
                    Task { await model.addToLibrary([track]) }
                }
                .accessibilityIdentifier("discover.addToLibrary")
            }
            if let page = track.rights.pageUrl.flatMap(URL.init(string:)) {
                Link(destination: page) {
                    Label("Open on \(track.provider)", systemImage: "safari")
                }
                .accessibilityIdentifier("discover.open")
            }
        }
    }
}

// MARK: Jamendo client ID

/// The Jamendo client ID field, with where to get one. Shown on its own
/// before there is one, and in a sheet to change it.
struct JamendoClientIdForm: View {
    let model: ListenModel
    let done: (() -> Void)?

    @State private var clientId = ""
    @State private var problem: String?
    @State private var saving = false

    var body: some View {
        Form {
            Section {
                TextField("Client ID", text: $clientId)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.done)
                    .onSubmit(save)
                    .accessibilityLabel("Jamendo client ID")
                    .accessibilityIdentifier("discover.clientIdField")
                Button(saving ? "Saving…" : "Save", action: save)
                    .disabled(saving || clientId.trimmingCharacters(in: .whitespaces).isEmpty)
                    .accessibilityIdentifier("discover.clientIdSave")
            } header: {
                Text("Jamendo client ID")
            } footer: {
                Text("Soundscape searches Jamendo with your own free client ID, for non-commercial use.")
            }
            if let problem {
                Section { Text(problem).forgeForeground(.danger) }
            }
            Section("Getting one") {
                Text("Sign in at Jamendo's developer site, create an app, and paste its Client ID here.")
                if let site = URL(string: "https://devportal.jamendo.com") {
                    Link("Open devportal.jamendo.com", destination: site)
                        .accessibilityIdentifier("discover.devPortal")
                }
            }
            if model.jamendoClientId != nil {
                Section {
                    Button("Forget Client ID", role: .destructive) {
                        Task {
                            do {
                                try await model.setJamendoClientId("")
                                done?()
                            } catch {
                                problem = ListenModel.message(error)
                            }
                        }
                    }
                    .accessibilityIdentifier("discover.clientIdForget")
                } footer: {
                    Text("Jamendo songs already in your library stay.")
                }
            }
        }
        .onAppear { if clientId.isEmpty { clientId = model.jamendoClientId ?? "" } }
    }

    private func save() {
        saving = true
        problem = nil
        let typed = clientId
        Task {
            defer { saving = false }
            do {
                try await model.setJamendoClientId(typed)
                done?()
            } catch {
                problem = ListenModel.message(error)
            }
        }
    }
}

// MARK: Jamendo by artist and genre

/// Jamendo's genres, each opening its most popular tracks this week.
// MARK: Test catalogue

/// The fixed test catalogue: eight tracks Soundscape makes itself (short,
/// long, no artwork, no tags, two recordings of one work, the same
/// recording twice, stream only), each saying what it checks. Works with no
/// network and no Jamendo client ID.
struct TestCatalogueView: View {
    let model: ListenModel
    let player: TakePlayer

    @State private var tests: [TestTrack]?
    @State private var problem: String?
    @State private var attempt = 0

    private var tracks: [CatalogueTrack] { tests?.map(\.track) ?? [] }

    var body: some View {
        List {
            if let tests, !tests.isEmpty {
                Section {
                    PlayShuffleButtons(disabled: false) { shuffle in
                        player.play(model.queue(tracks), shuffle: shuffle)
                    }
                    let missing = tracks.filter { !model.inLibrary($0.id) }
                    Button {
                        Task { await model.addToLibrary(missing) }
                    } label: {
                        Label(missing.isEmpty ? "All in Your Library" : "Add All to Library",
                              systemImage: missing.isEmpty ? "checkmark.circle" : "plus.circle")
                    }
                    .disabled(missing.isEmpty)
                    .accessibilityIdentifier("testCatalogue.addAll")
                } footer: {
                    Text("Tones Soundscape writes itself, in WAV, AIFF and CAF, so every check sounds the same on every phone. They take about 18 MB.")
                }
                ForEach(Array(tests.enumerated()), id: \.element.track.id) { index, test in
                    Section {
                        CatalogueRow(model: model, player: player, track: test.track) {
                            player.play(model.queue(tracks, from: index))
                        }
                    } footer: {
                        Text(test.checks)
                    }
                }
            }
        }
        .navigationTitle("Test Catalogue")
        .navigationBarTitleDisplayMode(.inline)
        .task(id: attempt) {
            switch await model.testCatalogue() {
            case .success(let found):
                tests = found
                problem = nil
            case .failure(let failure):
                if !failure.stopped { problem = failure.message }
            }
        }
        .onChange(of: model.resumes) {
            // Making the files stopped by leaving the app starts again.
            if tests == nil && problem == nil { attempt += 1 }
        }
        .overlay {
            if let problem, tests == nil {
                ForgeStateView.failed(
                    "Can't Make the Test Catalogue", message: problem,
                    retry: ForgeStateAction("Try Again", identifier: "testCatalogue.retry") { attempt += 1 }
                )
            } else if tests == nil {
                ForgeStateView.loading("Making the Test Tracks")
            }
        }
        .forgeFeedback(model.feedback)
    }
}

struct JamendoGenresView: View {
    let model: ListenModel

    var body: some View {
        List(model.jamendoGenres, id: \.tag) { genre in
            NavigationLink(genre.name, value: ListenRoute.jamendoGenre(genre))
                .accessibilityIdentifier("genres.genre")
        }
        .navigationTitle("Genres")
    }
}

/// Jamendo tracks for one genre, or more like the song playing (by its
/// artist, or in its genre), to play or add.
struct JamendoTracksView: View {
    enum Ask: Hashable {
        case genre(JamendoGenre)
        case more(trackId: String, by: JamendoMore)
        case shelf(JamendoShelf)
    }

    let model: ListenModel
    let player: TakePlayer
    let ask: Ask

    @State private var tracks: [CatalogueTrack]?
    @State private var name: String?
    @State private var problem: String?
    @State private var attempt = 0

    private var title: String {
        switch ask {
        case .genre(let genre): return genre.name
        case .more(_, .artist): return name.map { "More by \($0)" } ?? "More by This Artist"
        case .more(_, .genre): return name.map { "More \($0)" } ?? "More in This Genre"
        case .shelf(let shelf): return jamendoShelfTitle(shelf: shelf)
        }
    }

    var body: some View {
        List {
            if let problem, tracks != nil {
                Section {
                    Label(problem, systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("jamendoTracks.problem")
                }
            }
            if let tracks, !tracks.isEmpty {
                Section {
                    PlayShuffleButtons(disabled: false) { shuffle in
                        player.play(model.queue(tracks), shuffle: shuffle)
                    }
                }
                Section {
                    ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                        CatalogueRow(model: model, player: player, track: track) {
                            player.play(model.queue(tracks, from: index))
                        }
                    }
                } header: {
                    if case .genre = ask { Text("Popular This Week") }
                } footer: {
                    Text("Music from Jamendo, each track under its artist's Creative Commons licence.")
                }
            }
        }
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
        .task(id: attempt) { await load() }
        .onChange(of: model.resumes) {
            // A lookup stopped by leaving the app starts again on return.
            if tracks == nil && problem == nil { attempt += 1 }
        }
        .refreshable { model.refreshJamendo(); attempt += 1 }
        .overlay {
            if let problem, tracks == nil {
                ForgeStateView.failed(
                    "Can't Ask Jamendo", message: problem,
                    retry: ForgeStateAction("Try Again", identifier: "jamendoTracks.retry") { attempt += 1 }
                )
            } else if tracks == nil {
                ForgeStateView.loading("Asking Jamendo")
            } else if tracks?.isEmpty == true && problem == nil {
                ForgeStateView.empty(
                    "Nothing More on Jamendo", systemImage: "music.note",
                    description: "Jamendo has no other songs here right now."
                )
            }
        }
        .forgeFeedback(model.feedback)
    }

    private func load() async {
        problem = nil
        switch ask {
        case .genre(let genre):
            switch await model.jamendoTracks(in: genre) {
            case .success(let found): tracks = found
            case .failure(let failure): if !failure.stopped { problem = failure.message }
            }
        case .more(let trackId, let by):
            switch await model.moreFromJamendo(trackId, by: by) {
            case .success(let found):
                name = found.name
                tracks = found.tracks
            case .failure(let failure): if !failure.stopped { problem = failure.message }
            }
        case .shelf(let shelf):
            switch await model.jamendoTracks(on: shelf) {
            case .success(let found): tracks = found
            case .failure(let failure): if !failure.stopped { problem = failure.message }
            }
        }
    }
}
#endif
