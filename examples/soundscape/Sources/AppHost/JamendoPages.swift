#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

// Exploring Jamendo (docs/sources.md#exploring-jamendo-as-built-rust):
// an artist's page as Albums, EPs and Singles, where singles that share a
// cover are one album (Rust groups them), each release's own page, and
// artists to discover.

extension ReleaseKind {
    var name: String {
        switch self {
        case .album: "Album"
        case .ep: "EP"
        case .single: "Single"
        }
    }

    var plural: LocalizedStringKey {
        switch self {
        case .album: "Albums"
        case .ep: "EPs"
        case .single: "Singles"
        }
    }
}

extension JamendoRelease {
    /// "2023", or nil without a date.
    var year: String? { released.map { String($0.prefix(4)) } }

    /// "EP · 2023".
    var summary: String { [kind.name, year].compactMap { $0 }.joined(separator: " · ") }
}

// MARK: An artist

/// A Jamendo artist: their picture, Play and Shuffle for everything, then
/// their albums, EPs and singles, newest first.
struct JamendoArtistView: View {
    let model: ListenModel
    let player: TakePlayer
    let artist: JamendoArtistRef

    @State private var page: JamendoArtistPage?
    @State private var problem: String?
    @State private var attempt = 0

    private var name: String { page?.artist.name ?? artist.name ?? "Artist" }
    private var allTracks: [CatalogueTrack] { page?.releases.flatMap(\.tracks) ?? [] }

    var body: some View {
        List {
            if let page {
                header(page.artist)
                PageActions(disabled: allTracks.isEmpty, play: { shuffle in
                    player.play(model.queue(allTracks), shuffle: shuffle)
                }, identifier: "jamendoArtist")
                ForEach([ReleaseKind.album, .ep], id: \.self) { kind in
                    let releases = page.releases.filter { $0.kind == kind }
                    Shelf(title: kind.plural, items: releases, identifier: "jamendoArtist.\(kind.name.lowercased())") { release in
                        NavigationLink(value: ListenRoute.jamendoRelease(release, artist: page.artist)) {
                            ArtworkTile(title: release.title, subtitle: release.summary, url: release.artworkUrl)
                                .forgeSurfaceSource(ListenRoute.jamendoRelease(release, artist: page.artist))
                        }
                        .buttonStyle(.plain)
                        .accessibilityHint(release.grouped ? "Singles that share a cover" : "Opens it")
                        .accessibilityIdentifier("jamendoArtist.release")
                    }
                    .shelfRow()
                }
                singles(page)
                Section {
                } footer: {
                    Text("Music from Jamendo, each track under its artist's Creative Commons licence.")
                }
            }
        }
        .listStyle(.plain)
        .artworkBackdrop(title: name, url: page?.artist.imageUrl)
        .navigationTitle(name)
        .navigationBarTitleDisplayMode(.inline)
        .task(id: attempt) { await load() }
        .onChange(of: model.resumes) {
            if page == nil && problem == nil { attempt += 1 }
        }
        .refreshable { attempt += 1 }
        .overlay {
            if let problem, page == nil {
                ForgeStateView.failed(
                    "Can't Ask Jamendo", message: problem,
                    retry: ForgeStateAction("Try Again", identifier: "jamendoArtist.retry") { attempt += 1 }
                )
            } else if page == nil {
                ForgeStateView.loading("Asking Jamendo")
            } else if allTracks.isEmpty {
                ForgeStateView.empty(
                    "No Songs on Jamendo", systemImage: "music.mic",
                    description: "Jamendo has no songs by this artist right now."
                )
            }
        }
        .forgeFeedback(model.feedback)
    }

    private func header(_ artist: JamendoArtist) -> some View {
        VStack(spacing: 8) {
            Artwork(title: artist.name, url: artist.imageUrl, size: 180, circle: true)
                .shadow(color: .black.opacity(0.25), radius: 18, y: 10)
                .padding(.bottom, 8)
            Text(artist.name)
                .font(.system(.title2, design: .rounded).weight(.bold))
                .multilineTextAlignment(.center)
                .accessibilityAddTraits(.isHeader)
            if let joined = artist.joined?.prefix(4) {
                Text("On Jamendo since \(String(joined))")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 8)
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    /// Singles as songs: a tap plays from there through every single.
    @ViewBuilder
    private func singles(_ page: JamendoArtistPage) -> some View {
        let tracks = page.releases.filter { $0.kind == .single }.flatMap(\.tracks)
        if !tracks.isEmpty {
            Section {
                ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                    CatalogueRow(model: model, player: player, track: track) {
                        player.play(model.queue(tracks, from: index))
                    }
                }
            } header: {
                Text("Singles")
                    .font(.shelfTitle)
                    .foregroundStyle(.primary)
                    .textCase(nil)
                    .accessibilityAddTraits(.isHeader)
            }
        }
    }

    private func load() async {
        problem = nil
        switch await model.jamendoArtist(artist) {
        case .success(let found): page = found
        case .failure(let failure): if !failure.stopped { problem = failure.message }
        }
    }
}

// MARK: A release

/// One release: its cover large, what it is and when, Play and Shuffle,
/// its songs, and Add All to Library. A grouped one says why.
struct JamendoReleaseView: View {
    let model: ListenModel
    let player: TakePlayer
    let release: JamendoRelease
    let artist: JamendoArtist

    var body: some View {
        let tracks = release.tracks
        let missing = tracks.filter { !model.inLibrary($0.id) }
        List {
            header
            PageActions(disabled: tracks.isEmpty, play: { shuffle in
                player.play(model.queue(tracks), shuffle: shuffle)
            }, identifier: "jamendoRelease")
            Section {
                ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                    CatalogueRow(model: model, player: player, track: track) {
                        player.play(model.queue(tracks, from: index))
                    }
                }
            } footer: {
                VStack(alignment: .leading, spacing: 6) {
                    Text(length)
                    if release.grouped {
                        Text("These singles share a cover, so Soundscape keeps them together. On Jamendo each is on its own.")
                    }
                }
                .font(.footnote)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("jamendoRelease.length")
            }
            if !missing.isEmpty {
                Section {
                    Button(missing.count == tracks.count ? "Add All to Library" : "Add the Rest to Library",
                           systemImage: "plus") {
                        Task { await model.addToLibrary(missing) }
                    }
                    .accessibilityIdentifier("jamendoRelease.addAll")
                }
            }
        }
        .listStyle(.plain)
        .artworkBackdrop(title: release.title, url: release.artworkUrl)
        .navigationTitle(release.title)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Menu("More", systemImage: "ellipsis.circle") {
                    Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                        player.add(model.queue(tracks), next: true)
                    }
                    .accessibilityIdentifier("jamendoRelease.playNext")
                    Button("Add to Queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                        player.add(model.queue(tracks), next: false)
                    }
                    .accessibilityIdentifier("jamendoRelease.addToQueue")
                }
                .accessibilityIdentifier("jamendoRelease.more")
            }
        }
        .forgeFeedback(model.feedback)
    }

    private var header: some View {
        VStack(spacing: 8) {
            Artwork(title: release.title, url: release.artworkUrl, size: 260)
                .shadow(color: .black.opacity(0.25), radius: 18, y: 10)
                .padding(.bottom, 8)
            Text(release.title)
                .font(.system(.title2, design: .rounded).weight(.bold))
                .multilineTextAlignment(.center)
                .accessibilityAddTraits(.isHeader)
            // Back to the artist: their page if it is already open
            // underneath, rather than a second copy of it.
            NavigationLink(value: ListenRoute.jamendoArtist(.id(artist.id, name: artist.name))) {
                Text(artist.name).font(.title3).foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
            .accessibilityHint("Opens the artist")
            .accessibilityIdentifier("jamendoRelease.artist")
            Text(release.summary.uppercased())
                .font(.caption.weight(.semibold))
                .tracking(0.6)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 8)
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    /// "5 songs, 18 minutes".
    private var length: String {
        let count = release.tracks.count
        let songs = "\(count) \(count == 1 ? "song" : "songs")"
        guard release.durationSeconds > 0 else { return songs }
        let minutes = Int((release.durationSeconds / 60).rounded())
        return "\(songs), \(minutes) \(minutes == 1 ? "minute" : "minutes")"
    }
}

// MARK: Artists to discover

/// A Jamendo artist as a round tile that opens their page.
struct JamendoArtistTile: View {
    let artist: JamendoArtist

    var body: some View {
        let route = ListenRoute.jamendoArtist(.id(artist.id, name: artist.name))
        NavigationLink(value: route) {
            ArtworkTile(title: artist.name, subtitle: nil, url: artist.imageUrl, circle: true)
                .forgeSurfaceSource(route)
        }
        .buttonStyle(.plain)
        .accessibilityHint("Opens the artist")
        .accessibilityIdentifier("discover.artist")
    }
}

/// Every artist in one of Jamendo's lists, in a grid.
struct JamendoArtistsView: View {
    let model: ListenModel
    let list: JamendoArtistList

    @State private var artists: [JamendoArtist]?
    @State private var problem: String?
    @State private var attempt = 0

    var body: some View {
        ScrollView {
            if let artists {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 140), spacing: 16)], spacing: 20) {
                    ForEach(artists) { artist in
                        NavigationLink(value: ListenRoute.jamendoArtist(.id(artist.id, name: artist.name))) {
                            ArtworkTile(title: artist.name, subtitle: nil, url: artist.imageUrl, size: nil, circle: true)
                        }
                        .buttonStyle(.plain)
                        .accessibilityHint("Opens the artist")
                        .accessibilityIdentifier("jamendoArtists.artist")
                    }
                }
                .padding(SoundscapeMetrics.screenEdge)
            }
        }
        .navigationTitle(list == .popular ? "Artists to Discover" : "New on Jamendo")
        .navigationBarTitleDisplayMode(.inline)
        .task(id: attempt) { await load() }
        .refreshable { attempt += 1 }
        .overlay {
            if let problem, artists == nil {
                ForgeStateView.failed(
                    "Can't Ask Jamendo", message: problem,
                    retry: ForgeStateAction("Try Again", identifier: "jamendoArtists.retry") { attempt += 1 }
                )
            } else if artists == nil {
                ForgeStateView.loading("Asking Jamendo")
            }
        }
    }

    private func load() async {
        problem = nil
        switch await model.jamendoArtists(list) {
        case .success(let found): artists = found
        case .failure(let failure): if !failure.stopped { problem = failure.message }
        }
    }
}
#endif
