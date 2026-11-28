#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

// L1 Live Radio (docs/podcasts-and-radio.md): real stations from around the
// world, from the Radio Browser directory, inside the four tabs: Discover's
// Live Radio, Search, and Library > Stations. A station plays through
// `LivePlayer`, never the queue.

/// Discover's Live Radio, before and while searching: browse by country,
/// language or genre, your saved stations, and the most played stations.
struct LiveRadioHome: View {
    let query: String

    @Environment(LiveRadioModel.self) private var live
    @State private var stations: [LiveStation] = []
    @State private var problem: String?
    @State private var loading = false
    @State private var attempt = 0

    var body: some View {
        List {
            if query.isEmpty {
                Section {
                    browse(.country, "Countries", systemImage: "globe.europe.africa", identifier: "live.countries")
                    browse(.language, "Languages", systemImage: "character.bubble", identifier: "live.languages")
                    browse(.genre, "Genres", systemImage: "music.quarternote.3", identifier: "live.genres")
                }
                if !live.kept.saved.isEmpty {
                    Section("Your Stations") {
                        ForEach(live.kept.saved.prefix(5)) { LiveStationRow(station: $0) }
                        if live.kept.saved.count > 5 {
                            NavigationLink(value: ListenRoute.liveSaved) { Text("All Your Stations") }
                                .accessibilityIdentifier("live.allSaved")
                        }
                    }
                }
            }
            Section {
                ForEach(stations) { LiveStationRow(station: $0) }
            } header: {
                if !stations.isEmpty { Text(query.isEmpty ? "Most Played Worldwide" : "Stations") }
            } footer: {
                if !stations.isEmpty { LiveRadioCredit() }
            }
        }
        .overlay {
            if let problem, stations.isEmpty {
                ForgeStateView.failed(
                    "Can't Reach Radio Browser", message: problem,
                    retry: ForgeStateAction("Try Again", identifier: "live.retry") { attempt += 1 }
                )
            } else if loading && stations.isEmpty {
                ForgeStateView.loading("Finding stations")
            } else if !loading && stations.isEmpty && !query.isEmpty {
                ForgeStateView.noResults(for: query)
            }
        }
        .task(id: "\(query)|\(attempt)") { await find() }
        .refreshable { attempt += 1 }
    }

    private func browse(_ kind: LiveCategoryKind, _ title: LocalizedStringKey, systemImage: String, identifier: String) -> some View {
        NavigationLink(value: ListenRoute.liveCategories(kind)) {
            ForgeRow(title, systemImage: systemImage, accent: .purple) { EmptyView() }
        }
        .accessibilityIdentifier(identifier)
    }

    private func find() async {
        if !query.isEmpty {
            try? await Task.sleep(for: .milliseconds(400))
            if Task.isCancelled { return }
        }
        loading = true
        defer { loading = false }
        problem = nil
        switch await live.stations(query.isEmpty ? .top : .search(text: query)) {
        case .success(let found): stations = found
        case .failure(let failure): if !failure.stopped { problem = failure.message }
        }
    }
}

/// One list of stations: a country's, a language's, a genre's, or a search.
struct LiveStationsView: View {
    let ask: LiveAsk
    let title: String

    @Environment(LiveRadioModel.self) private var live
    @State private var stations: [LiveStation]?
    @State private var problem: String?
    @State private var attempt = 0

    var body: some View {
        List {
            Section {
                ForEach(stations ?? []) { LiveStationRow(station: $0) }
            } footer: {
                if stations?.isEmpty == false { LiveRadioCredit() }
            }
        }
        .navigationTitle(title)
        .overlay {
            if let problem, stations?.isEmpty ?? true {
                ForgeStateView.failed(
                    "Can't Reach Radio Browser", message: problem,
                    retry: ForgeStateAction("Try Again", identifier: "live.retry") { attempt += 1 }
                )
            } else if stations == nil {
                ForgeStateView.loading("Finding stations")
            } else if stations?.isEmpty == true {
                // Radio Browser lists only what people have added.
                ForgeStateView.empty(
                    "No Stations Listed",
                    systemImage: "dot.radiowaves.left.and.right",
                    description: "Nobody has listed a station here in Radio Browser yet."
                )
            }
        }
        .task(id: attempt) {
            problem = nil
            switch await live.stations(ask) {
            case .success(let found): stations = found
            case .failure(let failure): if !failure.stopped { problem = failure.message }
            }
        }
        .refreshable { attempt += 1 }
    }
}

/// Countries, languages or genres, each with how many stations Radio
/// Browser lists, to pick one.
struct LiveCategoriesView: View {
    let kind: LiveCategoryKind

    @Environment(LiveRadioModel.self) private var live
    @State private var categories: [LiveCategory]?
    @State private var problem: String?
    @State private var filter = ""
    @State private var attempt = 0

    private var title: String {
        switch kind {
        case .country: "Countries"
        case .language: "Languages"
        case .genre: "Genres"
        }
    }

    private var shown: [LiveCategory] {
        let all = categories ?? []
        let text = filter.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return all }
        return all.filter { $0.name.localizedCaseInsensitiveContains(text) }
    }

    var body: some View {
        List {
            Section {
                ForEach(shown) { category in
                    NavigationLink(value: ListenRoute.liveStations(ask(category), title: category.name)) {
                        LabeledContent(category.name) {
                            Text(category.stationCount, format: .number)
                        }
                    }
                    .accessibilityLabel("\(category.name), \(category.stationCount) stations")
                    .accessibilityIdentifier("live.category")
                }
            } footer: {
                if categories?.isEmpty == false {
                    Text("Stations listed in Radio Browser, a community directory. It doesn't have every station in the world.")
                }
            }
        }
        .navigationTitle(title)
        .searchable(text: $filter, prompt: "Filter \(title)")
        .overlay {
            if let problem, categories == nil {
                ForgeStateView.failed(
                    "Can't Reach Radio Browser", message: problem,
                    retry: ForgeStateAction("Try Again", identifier: "live.retry") { attempt += 1 }
                )
            } else if categories == nil {
                ForgeStateView.loading("Asking Radio Browser")
            }
        }
        .task(id: attempt) {
            problem = nil
            switch await live.categories(kind) {
            case .success(let found): categories = found
            case .failure(let failure): if !failure.stopped { problem = failure.message }
            }
        }
    }

    private func ask(_ category: LiveCategory) -> LiveAsk {
        switch kind {
        case .country: .country(code: category.key)
        case .language: .language(name: category.key)
        case .genre: .genre(tag: category.key)
        }
    }
}

/// Library > Stations: the stations you saved, and the ones played lately.
struct LiveSavedView: View {
    @Environment(LiveRadioModel.self) private var live

    var body: some View {
        List {
            if !live.kept.saved.isEmpty {
                Section("Saved") {
                    ForEach(live.kept.saved) { LiveStationRow(station: $0) }
                }
            }
            if !live.kept.recent.isEmpty {
                Section {
                    ForEach(live.kept.recent) { LiveStationRow(station: $0) }
                } header: {
                    HStack {
                        Text("Recently Played")
                        Spacer()
                        Button("Clear") { Task { await live.clearRecent() } }
                            .textCase(nil)
                            .accessibilityLabel("Clear Recently Played Stations")
                            .accessibilityIdentifier("live.clearRecent")
                    }
                }
            }
        }
        .navigationTitle("Stations")
        .overlay {
            if live.kept.saved.isEmpty && live.kept.recent.isEmpty {
                ForgeStateView.empty(
                    "No Stations Yet",
                    systemImage: "dot.radiowaves.left.and.right",
                    description: "Find live radio in Discover, then save the stations you like."
                )
            }
        }
    }
}

/// A station: its logo, name and where it's from. Tap to listen; hold for
/// Save.
struct LiveStationRow: View {
    let station: LiveStation

    @Environment(LivePlayer.self) private var player
    @Environment(LiveRadioModel.self) private var live

    var body: some View {
        Button {
            Task { await player.tune(station) }
        } label: {
            HStack(spacing: 12) {
                Artwork(title: station.name, url: station.artworkUrl, size: 44)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 4) {
                        Text(station.name).lineLimit(1)
                        if live.isSaved(station.uuid) {
                            Image(systemName: "star.fill")
                                .font(.caption2)
                                .foregroundStyle(.tint)
                                .accessibilityLabel("Saved")
                        }
                    }
                    Text(station.summary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
                if player.station?.uuid == station.uuid {
                    Image(systemName: player.isPlaying ? "waveform" : "pause.fill")
                        .foregroundStyle(.tint)
                        .accessibilityLabel(player.isPlaying ? "Playing" : "Paused")
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityHint("Plays the station live")
        .accessibilityIdentifier("live.station")
        .contextMenu {
            LiveStationMenu(station: station)
        }
        .swipeActions(edge: .trailing) {
            Button {
                Task { await live.toggleSaved(station) }
            } label: {
                live.isSaved(station.uuid)
                    ? Label("Remove", systemImage: "star.slash")
                    : Label("Save", systemImage: "star")
            }
            .tint(.purple)
        }
    }
}

/// Save or remove, and the station's own website.
struct LiveStationMenu: View {
    let station: LiveStation

    @Environment(LiveRadioModel.self) private var live

    var body: some View {
        Button {
            Task { await live.toggleSaved(station) }
        } label: {
            live.isSaved(station.uuid)
                ? Label("Remove from Your Stations", systemImage: "star.slash")
                : Label("Save to Your Stations", systemImage: "star")
        }
        .accessibilityIdentifier("live.save")
        if let homepage = station.homepage, let url = URL(string: homepage) {
            Link(destination: url) {
                Label("Station Website", systemImage: "safari")
            }
            .accessibilityIdentifier("live.website")
        }
    }
}

/// Where the stations come from.
struct LiveRadioCredit: View {
    var body: some View {
        Text("Stations listed in Radio Browser, a community directory. Soundscape plays their streams; it doesn't record or keep them.")
    }
}

/// Now Playing for a live station: what it is, that it's live, and
/// play, stop and save. No scrubber or skip: it's a broadcast.
struct LiveNowPlayingView: View {
    @Environment(LivePlayer.self) private var player
    @Environment(LiveRadioModel.self) private var live
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            if let station = player.station {
                ScrollView {
                    VStack(spacing: 20) {
                        Artwork(title: station.name, url: station.artworkUrl, size: 240)
                            .shadow(radius: 12, y: 6)
                            .padding(.top, 24)
                        VStack(spacing: 6) {
                            Text(station.name)
                                .font(.title2.bold())
                                .multilineTextAlignment(.center)
                            LiveBadge()
                            if !station.country.isEmpty {
                                Text(station.country).foregroundStyle(.secondary)
                            }
                        }
                        if let failure = player.failure {
                            ForgeNotice(failure, systemImage: "antenna.radiowaves.left.and.right.slash")
                                .accessibilityIdentifier("live.failure")
                        }
                        HStack(spacing: 40) {
                            Button {
                                Task { await live.toggleSaved(station) }
                            } label: {
                                Image(systemName: live.isSaved(station.uuid) ? "star.fill" : "star")
                                    .font(.title2)
                                    .frame(width: 52, height: 52)
                            }
                            .accessibilityLabel(live.isSaved(station.uuid) ? "Remove from Your Stations" : "Save to Your Stations")
                            .accessibilityIdentifier("live.nowPlaying.save")
                            Button { player.toggle() } label: {
                                ZStack {
                                    Image(systemName: player.isPlaying ? "stop.fill" : "play.fill")
                                        .font(.system(size: 34))
                                        .contentTransition(.symbolEffect(.replace))
                                    if player.isConnecting {
                                        ProgressView().offset(y: 36)
                                    }
                                }
                                .frame(width: 72, height: 72)
                            }
                            .accessibilityLabel(player.isPlaying ? "Stop" : "Play Live")
                            .accessibilityIdentifier("live.nowPlaying.playStop")
                            Button {
                                player.stop()
                                dismiss()
                            } label: {
                                Image(systemName: "xmark.circle")
                                    .font(.title2)
                                    .frame(width: 52, height: 52)
                            }
                            .accessibilityLabel("Stop Live Radio")
                            .accessibilityHint("Goes back to your music")
                            .accessibilityIdentifier("live.nowPlaying.end")
                        }
                        details(station)
                    }
                    .padding(.horizontal, SoundscapeMetrics.screenEdge)
                    .padding(.bottom, 32)
                }
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Close", systemImage: "chevron.down") { dismiss() }
                            .accessibilityIdentifier("live.nowPlaying.close")
                    }
                }
            } else {
                Color.clear.onAppear { dismiss() }
            }
        }
    }

    private func details(_ station: LiveStation) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            if !station.tags.isEmpty {
                LabeledContent("Plays", value: station.tags.prefix(4).map(\.capitalized).joined(separator: ", "))
            }
            if !station.languages.isEmpty {
                LabeledContent("Language", value: station.languages.prefix(3).map(\.capitalized).joined(separator: ", "))
            }
            if !station.streamDescription.isEmpty {
                LabeledContent("Stream", value: station.streamDescription)
            }
            if !station.secure {
                Label("This stream isn't encrypted (plain http).", systemImage: "lock.open")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("live.notEncrypted")
            }
            if let homepage = station.homepage, let url = URL(string: homepage) {
                Link(destination: url) { Label("Station Website", systemImage: "safari") }
                    .accessibilityIdentifier("live.nowPlaying.website")
            }
            LiveRadioCredit()
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// "LIVE", on Now Playing and the mini-player.
struct LiveBadge: View {
    var body: some View {
        Text("LIVE")
            .font(.caption2.weight(.heavy))
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .foregroundStyle(.white)
            .background(Capsule().fill(.red))
            .accessibilityLabel("Live")
    }
}
#endif
